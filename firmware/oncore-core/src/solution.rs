//! The receiver's solution as any report gives it: the satellites giving
//! position, the fix type, each satellite's Doppler and estimate of the
//! second, the least-squares matrix, the velocity's axes, the clock's
//! terms and the magnetic variation.

use crate::channels::inverse;
use crate::channels::line_of_sight;
use crate::dop::UNKNOWNS;
use crate::dop::triangular;
use crate::gnss::Clock;
use crate::receiver::Receiver;
use crate::sky::Prn;
use crate::sky::Satellite;
use crate::units::Decidegrees;
use crate::units::Decimeters;
use crate::units::DecimetersPerSecond;
use crate::units::Ecef;
use crate::units::GpsSeconds;
use crate::units::Hertz;
use crate::units::Nanoseconds;
use crate::units::NanosecondsPerSecond;
use crate::units::Seconds;
use crate::units::TENTHS_PER_UNIT;
use crate::vector::AXES;
use crate::wmm;
use smartclock_ubx::nav::FixType;

/// The least-squares matrix's upper triangle, east, north, up and time.
pub(crate) const MATRIX_ELEMENTS: usize = triangular(UNKNOWNS);
/// The magnetic variation's range, tenths of a degree (VP p. 97).
const VARIATION_MAX: Decidegrees<f64> = Decidegrees::new(1800.0);
/// The clock's bias and the velocities, 0.1 m and 0.1 m/s, held to an
/// `i16` either side of zero, and its drift to a `u16` (VP pp. 97-98).
const BIAS_MAX: Decimeters<i64> = Decimeters::new(i16::MAX as i64);
const VELOCITY_MAX: DecimetersPerSecond<i32> = DecimetersPerSecond::new(i16::MAX as i32);
const DRIFT_MAX: DecimetersPerSecond<i64> = DecimetersPerSecond::new(u16::MAX as i64);

/// The receiver clock's terms as the Oncore gives them (VP p. 98).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockTerms {
    /// The bias, 0.1 m.
    pub(crate) bias: Decimeters<i16>,
    /// The drift's magnitude, 0.1 m/s: the manual has the oscillator
    /// offset unsigned (pp. 98, 137).
    pub(crate) drift: DecimetersPerSecond<u16>,
}

/// The satellites giving position, by channel.
pub(crate) fn position_satellites(receiver: &Receiver) -> impl Iterator<Item = Satellite> + '_ {
    let a = receiver.selector.assignment;
    (0..receiver.channels).filter(move |c| a.position & (1 << c) != 0).filter_map(move |c| {
        let s = receiver.sky.get(a.prns[c]).copied()?;
        s.positioning().then_some(s)
    })
}

/// The fix type, if the fix is OK.
pub(crate) fn fix_type(receiver: &Receiver) -> FixType {
    receiver.pvt.filter(|p| p.fix_ok).map_or(FixType::None, |p| p.fix_type)
}

/// Satellite `prn`'s Doppler, `s` its state: the module's measurement
/// less its clock drift for a tracked satellite, else computed from the
/// almanac at the counted time, as the Oncore computes it; `None` with
/// neither.
pub(crate) fn doppler(receiver: &Receiver, prn: Prn, s: &Satellite) -> Option<Hertz<i16>> {
    if s.measured {
        Some(satellite_doppler(s.doppler, receiver.clock.map(|c| c.drift)))
    } else {
        almanac_doppler(receiver, receiver.counted?, prn)
    }
}

/// A satellite's Doppler from its motion alone, Hz: the module's
/// measurement, made against its own oscillator, less that oscillator's
/// drift (NAV-CLOCK), as the Oncore's almanac Doppler has none.
fn satellite_doppler(measured: Hertz<i16>, drift: Option<NanosecondsPerSecond>) -> Hertz<i16> {
    let correction = drift.map_or(Hertz::new(0.0), NanosecondsPerSecond::l1_doppler);
    (Hertz::from(measured) + correction).round()
}

/// A satellite's Doppler from the almanac, seen from the module's
/// position at `time`, if both are known.
pub fn almanac_doppler(receiver: &Receiver, time: GpsSeconds, prn: Prn) -> Option<Hertz<i16>> {
    let almanac = receiver.nav.almanac[prn.index()].as_ref()?;
    let position = Ecef::from_centimeters(receiver.ecef?);
    Some(almanac.doppler(time.seconds_of_week().to_f64(), position).round())
}

/// Satellite `s`'s estimate of the fraction of the second, ns: the
/// epoch's fraction plus its pseudorange residual, for a satellite giving
/// position and used in the solution; `None` otherwise.
pub(crate) fn time_estimate(receiver: &Receiver, s: &Satellite) -> Option<Nanoseconds<u32>> {
    let fraction = receiver.pvt.map_or(Nanoseconds::new(0), |p| p.nano);
    (s.positioning() && s.used())
        .then(|| (Nanoseconds::<i64>::from(fraction) + s.residual.to_ns().round()).within_second())
}

/// The inverse normal matrix of the satellites giving position, east,
/// north, up and time; `None` with too few or a singular geometry.
pub(crate) fn inverse_matrix(receiver: &Receiver) -> Option<[[f32; UNKNOWNS]; UNKNOWNS]> {
    inverse(position_satellites(receiver).map(|s| line_of_sight(s.elevation, s.azimuth)))
}

/// The upper triangle of `q`, row by row (`[0][0]`, `[0][1]` ...
/// `[3][3]`), in tenths, each held to an `i16`.
pub(crate) fn matrix_tenths(q: &[[f32; UNKNOWNS]; UNKNOWNS]) -> [i16; MATRIX_ELEMENTS] {
    let mut matrix = [0; MATRIX_ELEMENTS];
    let upper = q.iter().enumerate().flat_map(|(i, row)| row[i..].iter());
    for (m, v) in matrix.iter_mut().zip(upper) {
        *m = (v * f32::from(TENTHS_PER_UNIT)).clamp(i16::MIN.into(), i16::MAX.into()) as i16;
    }
    matrix
}

/// The velocity north, east and up, 0.1 m/s, each held to an `i16`.
pub(crate) fn velocity_axes(receiver: &Receiver) -> Option<[DecimetersPerSecond<i16>; AXES]> {
    receiver.pvt.map(|p| {
        let [north, east, down] = p.velocity;
        [north, east, -down].map(|v| {
            DecimetersPerSecond::new(v.decimeters_per_second().clamp(-VELOCITY_MAX, VELOCITY_MAX).get() as i16)
        })
    })
}

/// The receiver clock's terms, from the last NAV-CLOCK.
pub(crate) fn clock_terms(receiver: &Receiver) -> Option<ClockTerms> {
    receiver.clock.map(|Clock { bias, drift, .. }| ClockTerms {
        bias: Decimeters::new(bias.decimeters().clamp(-BIAS_MAX, BIAS_MAX).get() as i16),
        drift: DecimetersPerSecond::new(drift.decimeters_per_second().min(DRIFT_MAX).get() as u16),
    })
}

/// The magnetic variation at the fix, tenths of a degree east, from the
/// World Magnetic Model at `time`'s UTC date; `None` without a fix.
pub fn magnetic_variation(receiver: &Receiver, time: GpsSeconds) -> Option<Decidegrees> {
    let fix = receiver.fix?;
    let leap = Seconds::<i64>::from(receiver.leap_seconds.unwrap_or_default());
    let year = (time.unix() - leap).decimal_year();
    let km = fix.height_gps.kilometers();
    let d = wmm::field(fix.latitude.degrees(), fix.longitude.degrees(), km, year).declination();
    Some(d.decidegrees().round_within(VARIATION_MAX))
}

#[cfg(test)]
mod tests {
    use super::satellite_doppler;
    use crate::units::Hertz;
    use crate::units::NanosecondsPerSecond;

    #[test]
    fn doppler_less_the_receivers_drift() {
        // apu-0's NEO-M8T, 2026-10-28: -840 ns/s, about -1323 Hz at L1;
        // two satellites' Doppler measured against it, and their own.
        const DRIFT: NanosecondsPerSecond = NanosecondsPerSecond::new(-840);
        const MEASURED_HZ: [(i16, i16); 2] = [(3861, 2538), (-1509, -2832)];
        const UNCORRECTED_HZ: i16 = 100;
        for (measured, own) in MEASURED_HZ {
            assert_eq!(satellite_doppler(Hertz::new(measured), Some(DRIFT)), Hertz::new(own));
        }
        let uncorrected = Hertz::new(UNCORRECTED_HZ);
        assert_eq!(satellite_doppler(uncorrected, None), uncorrected);
    }
}
