//! Pseudorange corrections as a differential reference station makes
//! them (VP reference pp. 89-90, `@@Bh`, answered by `@@Ce`): for each
//! satellite, the range from the receiver's position to it, from its
//! ephemeris, less the module's pseudorange with both clocks taken out,
//! and the same for their rates.  What remains is what a nearby
//! receiver's pseudorange shares: the ionosphere, the troposphere and the
//! broadcast orbit's and clock's errors.

use crate::gnss::Clock;
use crate::gnss::Measurements;
use crate::nav::Ephemeris;
use crate::nav::Nav;
use crate::nav::OMEGA_E;
use crate::nav::Transmitter;
use crate::sky::PRNS;
use crate::sky::Prn;
use crate::units::Centimeters;
use crate::units::Ecef;
use crate::units::Meters;
use crate::units::MetersPerSecond;

use crate::units::Seconds;

use crate::vector::AXES;
use crate::vector::Direction;

/// The longest a measurement epoch is from NAV-CLOCK's, s; the transit
/// time a satellite's signal starts from, s; a millisecond, s.
const EPOCH_MATCH: Seconds = Seconds::new(0.5);
const NOMINAL_TRANSIT: Seconds = Seconds::new(0.075);
const MILLISECOND: Seconds = Seconds::new(1e-3);
/// The transit time's passes: the second leaves under a micrometer.
const TRANSIT_PASSES: usize = 2;

/// One satellite's corrections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correction {
    /// Pseudorange, and its rate.
    pub range: Meters,
    pub rate: MetersPerSecond,
    /// The ephemeris' issue of data.
    pub iode: u8,
}

/// The measurements' GPS time of week: the navigation epoch's,
/// NAV-CLOCK's, less its clock bias, if the measurements are that
/// epoch's.  Against the trace (`testdata/`), the corrections' spread
/// about an elevation model is least at this time, 1.4 m, against 5.5 m
/// at rcvTow, 15 ms before the epoch, less the bias.
pub fn gps_time(m: &Measurements, clock: &Clock) -> Option<Seconds> {
    let tow = clock.tow.seconds();
    if m.tow.since(tow).abs() >= EPOCH_MATCH {
        return None;
    }
    Some((tow - clock.bias.seconds()).within_week())
}

/// Satellite `prn`'s range less its pseudorange, m, and range rate less
/// pseudorange rate, m/s, received at GPS time `received` at `receiver`
/// (ECEF, m), with the satellite's clock out and the receiver's still in.
fn residual(
    e: &Ephemeris,
    m: &Measurements,
    prn: Prn,
    received: Seconds,
    receiver: Ecef,
) -> Option<Correction> {
    let i = prn.index();
    if !m.valid.contains(prn) || !e.fits(received) {
        return None;
    }
    let g = geometry(e, received, receiver);
    let pseudorange = m.pseudorange[i] + g.transmitter.clock.range();
    let pseudorange_rate = m.doppler[i].range_rate() + g.transmitter.drift.range_rate();
    Some(Correction { range: g.range - pseudorange, rate: g.range_rate - pseudorange_rate, iode: e.iode })
}

/// A satellite as a receiver sees it, its signal received at GPS time of
/// week `received`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// The geometric range, the Earth's rotation during transit included.
    pub range: Meters,
    /// Its rate for a stationary receiver.
    pub range_rate: MetersPerSecond,
    /// The unit vector from the receiver to the satellite, ECEF.
    pub line_of_sight: Direction,
    /// The satellite at transmission.
    pub(crate) transmitter: Transmitter,
}

/// The geometry of ephemeris `e`'s satellite seen from `receiver` (ECEF,
/// m) at `received`: the transit time from the geometric range, from a
/// nominal 75 ms; the second pass leaves under a micrometer.
pub fn geometry(e: &Ephemeris, received: Seconds, receiver: Ecef) -> Geometry {
    let (mut transit, mut range, mut los) = (NOMINAL_TRANSIT, Meters::new(0.0), Direction::default());
    let mut transmitter = e.transmitter(received);
    for _ in 0..TRANSIT_PASSES {
        transmitter = e.transmitter(received - transit);
        let d = transmitter.state.position.earth_rotated(OMEGA_E.angle(transit)) - receiver;
        range = d.norm();
        los = d.direction();
        transit = range.light_time();
    }
    let range_rate = transmitter.state.velocity.earth_rotated(OMEGA_E.angle(transit)).toward(los);
    Geometry { range, range_rate, line_of_sight: los, transmitter }
}

/// One epoch's corrections, every satellite's by PRN, and the GPS time
/// of week of the measurements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Set {
    pub(crate) time: Seconds,
    pub corrections: [Option<Correction>; PRNS],
}

/// What a computation works from, taken when it starts: the
/// measurements, their GPS time, the receiver clock's bias in the
/// pseudoranges, and the receiver's position, ECEF, m.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Input {
    measurements: Measurements,
    received: Seconds,
    bias: Meters,
    receiver: Ecef,
}

/// The corrections, computed a satellite at a time between I/O, as
/// channel selection is, from each epoch's measurements; [`Self::latest`]
/// is the last whole set.
///
/// The receiver's clock comes out of the pseudoranges as rcvTow less the
/// GPS time, to the whole millisecond the corrections' mean puts it at,
/// since the module steps its clock in whole milliseconds; and out of
/// the rates as their mean, the drift NAV-CLOCK gives only to the whole
/// ns/s, 0.3 m/s.
#[derive(Debug, Clone, PartialEq)]
pub struct Differential {
    input: Option<Input>,
    /// The next PRN to compute, by its index, and the set so far, the
    /// receiver's clock still in.
    next: usize,
    set: [Option<Correction>; PRNS],
    /// The measurements' receiver time of week of the last set started,
    /// so an epoch is computed once.
    started: Option<Seconds>,
    /// The last whole set, kept in place: it is too large to move
    /// through the stack.
    latest: Set,
    /// Whether `latest` holds one yet, and the seconds since it was made.
    complete: bool,
    age: Seconds<u8>,
}

/// The oldest set `@@Ce` gives, s: a set is made each second, so this
/// many missed means the measurements or the ephemerides have stopped.
const MAX_AGE: Seconds<u8> = Seconds::new(10);

impl Differential {
    /// No input, no corrections.
    #[expect(
        clippy::large_stack_arrays,
        reason = "a const fn: the firmware builds it at compile time, in oncore-app's static State"
    )]
    pub(crate) const fn new() -> Self {
        Self {
            input: None,
            next: 0,
            set: [None; PRNS],
            started: None,
            latest: Set { time: Seconds::new(0.0), corrections: [None; PRNS] },
            complete: false,
            age: Seconds::new(0),
        }
    }

    /// The last whole set, while it is recent.
    pub fn latest(&self) -> Option<&Set> {
        (self.complete && self.age <= MAX_AGE).then_some(&self.latest)
    }

    /// A last whole set, just made at `time`, of `correction` for `prn`
    /// alone, for tests.
    #[cfg(test)]
    pub(crate) fn set_latest(&mut self, time: Seconds, prn: Prn, correction: Correction) {
        self.latest.time = time;
        self.latest.corrections.fill(None);
        self.latest.corrections[prn.index()] = Some(correction);
        (self.complete, self.age) = (true, Seconds::new(0));
    }

    /// A second since the last whole set.
    pub(crate) fn second(&mut self) {
        self.age = self.age.saturating_increment();
    }

    pub fn busy(&self) -> bool {
        self.input.is_some()
    }

    /// Starts on the receiver's measurements, unless busy, they are the
    /// last ones started, or there are no measurements, clock or position.
    pub(crate) fn start(
        &mut self,
        measurements: Option<&Measurements>,
        clock: Option<Clock>,
        ecef: Option<[Centimeters; AXES]>,
    ) {
        let (Some(m), Some(clock), Some(ecef)) = (measurements, clock, ecef) else { return };
        if self.busy() || self.started == Some(m.tow) {
            return;
        }
        let Some(received) = gps_time(m, &clock) else { return };
        self.input = Some(Input {
            measurements: *m,
            received,
            bias: m.tow.since(received).range(),
            receiver: Ecef::from_centimeters(ecef),
        });
        (self.next, self.started) = (0, Some(m.tow));
        self.set.fill(None);
    }

    /// Computes up to `budget` satellites; returns true once the set is
    /// done, or none was started.
    pub fn step(&mut self, nav: &Nav, budget: usize) -> bool {
        // borrowed, and the set corrected in place: each is too large to
        // copy through the stack (AGENTS.md, Stack and RAM)
        let Some(input) = &self.input else { return true };
        for _ in 0..budget {
            if self.next >= PRNS {
                break;
            }
            let prn = Prn::from_index(self.next);
            self.next += 1;
            let Some(e) = nav.ephemeris[prn.index()].ephemeris() else { continue };
            let Some(x) = residual(&e, &input.measurements, prn, input.received, input.receiver) else {
                continue;
            };
            self.set[prn.index()] = Some(Correction { range: x.range + input.bias, ..x });
        }
        if self.next < PRNS {
            return false;
        }
        let received = input.received;
        self.input = None;
        let (n, range_sum, rate_sum) = self
            .set
            .iter()
            .flatten()
            .fold((0.0, Meters::new(0.0), MetersPerSecond::new(0.0)), |(n, a, b), c| {
                (n + 1.0, a + c.range, b + c.rate)
            });
        // A set with no satellite is no set: the last one stays, aging.
        if n > 0.0 {
            self.age = Seconds::new(0);
            let whole = (range_sum / n).round_to(MILLISECOND.range());
            let rate = rate_sum / n;
            self.complete = true;
            self.latest.time = received;
            self.latest.corrections = self.set;
            for c in self.latest.corrections.iter_mut().flatten() {
                (c.range, c.rate) = (c.range - whole, c.rate - rate);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Differential;
    use super::MAX_AGE;

    #[test]
    fn a_set_is_given_only_while_recent() {
        let mut d = Differential::new();
        assert!(d.latest().is_none());
        d.complete = true;
        for _ in 0..MAX_AGE.get() {
            d.second();
        }
        assert!(d.latest().is_some());
        d.second();
        assert!(d.latest().is_none());
    }
}
