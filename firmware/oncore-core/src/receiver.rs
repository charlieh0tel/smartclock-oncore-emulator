//! The settings the receiver acts on for itself, from those the engine
//! holds: the choice of the channels' satellites (VP pp. 27 to 42 and
//! 52), Time RAIM (pp. 102 to 106) and the fix the record gives (pp. 15,
//! 56 and 60).  What the module acts on goes to it through
//! [`control`](crate::control).

use smartclock_gps::engine::settings::FixAlgorithm;
use smartclock_gps::engine::settings::SatelliteSelect;
use smartclock_gps::engine::settings::Settings;
use smartclock_gps::record::Antenna;
use smartclock_gps::record::DopKind;
use smartclock_gps::record::ReceiverMode;
use smartclock_ubx::nav::NavDop;

use crate::channels::Selector;
use crate::differential::Differential;
use crate::gnss::Clock;
use crate::gnss::LeapEvent;
use crate::gnss::Measurements;
use crate::gnss::Pvt;
use crate::nav::Nav;
use crate::sky::PrnSet;
use crate::sky::Sky;
use crate::traim;
use crate::traim::Hold;
use crate::traim::Raim;
use crate::traim::Solution;
use crate::units::Centimeters;
use crate::units::Degrees;
use crate::units::GpsSeconds;
use crate::units::HundredNanoseconds;
use crate::units::MilliArcSeconds;
use crate::units::Nanoseconds;
use crate::units::Picoseconds;
use crate::units::Seconds;
use crate::units::Tenths;
use crate::vector::AXES;
use smartclock_gps::engine::settings::CHANNELS_MOST;

/// The settings' values at power-up and after `@@Cf` (VP pp. 15, 27, 37
/// and 102).
pub(crate) const DEFAULT_MASK_ANGLE: Degrees<u8> = Degrees::new(10);
#[cfg(test)]
const DEFAULT_DOP_HYSTERESIS: Tenths = Tenths::new(10);
#[cfg(test)]
const DEFAULT_THRESHOLD_2D_0D: Tenths = Tenths::new(120);
#[cfg(test)]
const DEFAULT_TRAIM_ALARM: HundredNanoseconds = HundredNanoseconds::new(10);

/// The settings the receiver acts on for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverSettings {
    /// The elevation a satellite must reach to be chosen (`@@Ag`).
    pub(crate) mask_angle: Degrees<u8>,
    /// The satellites never chosen (`@@Am`).
    pub(crate) ignore: PrnSet,
    /// How the satellites are chosen (`@@Ah`).
    pub(crate) satellite_select: SatelliteSelect,
    /// The satellite on each channel in manual selection, 0 for none
    /// (`@@Ai`).
    pub(crate) manual: [u8; CHANNELS_MOST],
    /// The best four or all in view (`@@Ar`).
    pub(crate) fix_algorithm: FixAlgorithm,
    /// The DOP the choice minimizes (`@@Aj`).
    pub(crate) dop_kind: DopKind,
    /// How much better another choice's DOP must be (`@@Ak`).
    pub(crate) xdop_hysteresis: Tenths,
    /// The position is held (`@@At`).
    pub(crate) position_hold: bool,
    /// The altitude is held (`@@Av`).
    pub(crate) altitude_hold: bool,
    /// The HDOP above which a fix with the altitude held is given up
    /// (`@@AC`).
    pub(crate) hdop_threshold: Tenths,
    /// Time RAIM on or off, and its alarm limit (`@@Bn`, `@@En`).
    pub(crate) traim: Raim,
    pub(crate) traim_alarm: HundredNanoseconds,
}

impl ReceiverSettings {
    /// Those of `settings` the receiver acts on.
    pub fn of(settings: &Settings) -> Self {
        Self {
            mask_angle: Degrees::new(settings.mask_angle.0),
            ignore: PrnSet::from_bits(settings.ignore_list.bits()),
            satellite_select: settings.satellite_select,
            manual: settings.manual_satellites,
            fix_algorithm: settings.fix_algorithm,
            dop_kind: settings.dop_kind,
            xdop_hysteresis: Tenths::new(settings.xdop_hysteresis.0),
            position_hold: settings.position_hold,
            altitude_hold: settings.altitude_hold,
            hdop_threshold: Tenths::new(settings.hdop_threshold.0),
            traim: if settings.raim_algorithm { Raim::On } else { Raim::Off },
            traim_alarm: HundredNanoseconds::new(settings.raim_alarm_limit.0),
        }
    }

    /// At power-up and after `@@Cf`.
    #[cfg(test)]
    pub(crate) const POWER_UP: Self = Self {
        mask_angle: DEFAULT_MASK_ANGLE,
        ignore: PrnSet::EMPTY,
        satellite_select: SatelliteSelect::BestGeometry,
        manual: [0; CHANNELS_MOST],
        fix_algorithm: FixAlgorithm::AllInView,
        dop_kind: DopKind::Position,
        xdop_hysteresis: DEFAULT_DOP_HYSTERESIS,
        position_hold: false,
        altitude_hold: false,
        hdop_threshold: DEFAULT_THRESHOLD_2D_0D,
        traim: Raim::Off,
        traim_alarm: DEFAULT_TRAIM_ALARM,
    };
}

/// The receiver's state: what the module has told it, and what is worked
/// out from that, the channels' satellites, the corrections and Time RAIM.
#[derive(Debug, Clone)]
pub struct Receiver {
    /// GPS less UTC, once known.
    pub(crate) leap_seconds: Option<Seconds<i8>>,
    /// The last fix's position, kept when the module is lost.
    pub fix: Option<Fix>,
    /// The module's last NAV-PVT.
    pub(crate) pvt: Option<Pvt>,
    /// Holding or navigating, as the module's last NAV-PVT with a fix
    /// showed it; kept through epochs without one, unknown before the
    /// first and once the module is lost.
    pub(crate) mode: ReceiverMode,
    /// The module's last NAV-DOP.
    pub(crate) dop: Option<NavDop>,
    pub sky: Sky,
    /// The leap second scheduled, if one is.
    pub(crate) leap_event: Option<LeapEvent>,
    /// Seconds on the timer since the module's last epoch.
    pub(crate) silent: Seconds<u8>,
    /// The first fix has given the geoid separation: the holds the
    /// module withheld for it are asked for.  Taken by
    /// `Control::second`.
    pub(crate) separation_learned: bool,
    /// ECEF position (NAV-POSECEF).
    pub(crate) ecef: Option<[Centimeters; AXES]>,
    pub clock: Option<Clock>,
    /// The module's last GPS measurements (RXM-RAWX).
    pub(crate) measurements: Option<Measurements>,
    /// The antenna supervisor's status (MON-HW).
    pub(crate) antenna: Antenna,
    /// The next 1 PPS's quantization error (TIM-TP).
    pub(crate) q_err: Option<Picoseconds>,
    /// The GPS navigation message (RXM-SFRBX).
    pub nav: Nav,
    /// The module's GPS time at its last epoch, if it has one.
    pub(crate) time: Option<GpsSeconds>,
    /// The GPS time of the last fixed epoch, counted on a second at a
    /// time through the seconds since, as a VP's clock runs without a
    /// fix: for what needs the date, not the fix (the almanac's week).
    pub(crate) counted: Option<GpsSeconds>,
    /// The channels the emulated engine has.
    pub(crate) channels: usize,
    /// The channels' satellites.
    pub selector: Selector,
    /// The pseudorange corrections.
    pub differential: Differential,
    /// The last Time RAIM evaluation and the channels whose satellites it
    /// isolated, a bit each.
    pub traim: Option<traim::Result>,
    pub(crate) traim_isolated: u8,
}

impl Receiver {
    /// The channels the emulated engine has.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The module's GPS time at its last epoch, if it has one.
    pub fn time(&self) -> Option<GpsSeconds> {
        self.time
    }

    /// The module lost: what it measured each second and its time go, so
    /// nothing reports a fix, signals or a 1 PPS solution from before; the
    /// last position, the leap seconds and the navigation data stay.
    pub fn lost(&mut self) {
        self.pvt = None;
        self.mode = ReceiverMode::Unknown;
        self.dop = None;
        self.sky.clear();
        self.clock = None;
        self.measurements = None;
        self.q_err = None;
        self.time = None;
    }

    /// The measurements Time RAIM takes: each satellite the module used,
    /// all of which make its pulse, on a channel or not, its PRN and its
    /// residual.
    fn traim_measurements(&self) -> ([Nanoseconds<f32>; traim::MAX], [u8; traim::MAX], usize) {
        let (mut m, mut prns, mut n) = ([Nanoseconds::new(0.0); traim::MAX], [0u8; traim::MAX], 0);
        for (prn, s) in self.sky.iter().filter(|(_, s)| s.used()) {
            m[n] = s.residual.to_ns();
            prns[n] = prn.get();
            n += 1;
        }
        (m, prns, n)
    }

    /// Evaluates Time RAIM on the satellites the module used.  The 1 PPS
    /// is the module's, from them all, so a satellite isolated here is
    /// still in the pulse's solution: an alarm, not OK, until the
    /// module's own RAIM drops it (NAV-SAT svUsed clear) and it leaves
    /// these measurements (UBX-13003221 R28, 31.2.3, NMEA GBS).
    pub(crate) fn evaluate_traim(&mut self, settings: &ReceiverSettings) {
        let (m, prns, n) = self.traim_measurements();
        let hold = if settings.position_hold { Hold::Position } else { Hold::Navigating };
        let mut r = traim::evaluate(&m[..n], settings.traim, settings.traim_alarm.nanoseconds(), hold);
        let isolated = |prn: u8| (0..n).any(|i| r.removed & (1 << i) != 0 && prns[i] == prn);
        let a = &self.selector.assignment;
        self.traim_isolated = (0..self.channels).filter(|&c| isolated(a.prns[c])).fold(0, |b, c| b | 1 << c);
        if r.removed != 0 {
            r.solution = Solution::Alarm;
        }
        self.traim = Some(r);
    }

    /// An epoch's second: the module's GPS time at it, if it has a fix.
    pub fn second(&mut self, gps_time: Option<GpsSeconds>) {
        self.differential.second();
        self.time = gps_time;
        self.count_on();
        self.silent = Seconds::new(0);
    }

    /// The counted time: the fixed epoch's, else a second on.
    fn count_on(&mut self) {
        self.counted = self.time.or(self.counted.map(|t| t + Seconds::new(1)));
    }

    /// A second on the timer, with no epoch; after [`MODULE_SILENT`] of
    /// them in a row the module is lost.
    pub fn missed(&mut self) {
        self.differential.second();
        self.time = None;
        self.count_on();
        self.silent = self.silent.saturating_increment();
        if self.silent == MODULE_SILENT {
            self.lost();
        }
    }

    /// The module's leap seconds.
    pub(crate) const fn leap_seconds(&mut self, leap: Seconds<i8>) {
        self.leap_seconds = Some(leap);
    }

    /// Starts the pseudorange corrections of the last measurements, if
    /// none are under way.
    pub(crate) fn start_corrections(&mut self) {
        self.differential.start(self.measurements.as_ref(), self.clock, self.ecef);
    }

    /// Nothing heard from the module, for an engine of `channels`.
    pub(crate) const fn new(channels: usize) -> Self {
        Self {
            leap_seconds: None,
            fix: None,
            pvt: None,
            mode: ReceiverMode::Unknown,
            dop: None,
            sky: Sky::new(),
            leap_event: None,
            silent: Seconds::new(0),
            separation_learned: false,
            ecef: None,
            clock: None,
            measurements: None,
            antenna: Antenna::Unknown,
            q_err: None,
            nav: Nav::new(),
            time: None,
            counted: None,
            channels,
            selector: Selector::new(),
            differential: Differential::new(),
            traim: None,
            traim_isolated: 0,
        }
    }
}

/// A position fix with both heights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fix {
    pub latitude: MilliArcSeconds,
    pub longitude: MilliArcSeconds,
    /// Above the WGS-84 ellipsoid.
    pub(crate) height_gps: Centimeters,
    /// Above mean sea level.
    pub(crate) height_msl: Centimeters,
}

/// Seconds without a message from the module before the self test fails
/// its channels.
pub(crate) const MODULE_SILENT: Seconds<u8> = Seconds::new(3);

/// The ranges of latitude, longitude and height (pp. 18, 22, 26).
#[cfg(test)]
mod tests {
    use smartclock_gps::engine::Engine;
    use smartclock_gps::engine::Options;
    use smartclock_gps::engine::identity::VP_SIX;

    use super::Fix;
    use super::MODULE_SILENT;
    use super::Receiver;
    use super::ReceiverSettings;
    use crate::fixtures::LEAP_SECONDS;
    use crate::gnss::Clock;
    use crate::gnss::Measurements;
    use crate::sky::CODE_LOCKED;
    use crate::sky::LOCKED;
    use crate::sky::PRNS;
    use crate::sky::Satellite;
    use crate::traim::Raim;
    use crate::traim::Solution;
    use crate::units::Centimeters;
    use crate::units::Decimeters;
    use crate::units::GpsSeconds;
    use crate::units::HundredNanoseconds;
    use crate::units::MilliArcSeconds;
    use crate::units::Nanoseconds;
    use crate::units::NanosecondsPerSecond;
    use crate::units::Picoseconds;
    use crate::units::Seconds;
    use crate::units::TimeOfWeek;
    use crate::vector::AXES;

    /// The sky's slot, PRN 7's, the tests give a satellite tracked but
    /// not yet used.
    const TRACKED_SLOT: usize = 6;
    /// Measurements 100 s into the week, and NAV-CLOCK's time for them;
    /// a receiver at the north pole, ECEF, cm.
    const MEASURED: Seconds = Seconds::new(100.0);
    const MEASURED_MS: u32 = 100_000;
    const NORTH_POLE_CM: [i32; AXES] = [0, 0, 637_800_000];

    /// Nothing heard from the module, for the six-channel VP.
    fn receiver() -> Receiver {
        Receiver::new(VP_SIX.model().channels())
    }

    /// NAV-CLOCK at `tow_ms`, no bias or drift.
    fn clock(tow_ms: u32) -> Clock {
        Clock {
            tow: TimeOfWeek::from_ms(tow_ms),
            bias: Nanoseconds::default(),
            drift: NanosecondsPerSecond::default(),
        }
    }

    #[test]
    fn power_up_as_smartclock_gps_has_it() {
        let engine = Engine::new(&VP_SIX, Options::DEFAULT).expect("the factory VP");
        assert_eq!(ReceiverSettings::of(&engine.settings()), ReceiverSettings::POWER_UP);
    }

    #[test]
    fn a_silent_module_leaves_no_live_data() {
        /// Any fix, kept; a quantization error, lost.
        const FIX: Fix = Fix {
            latitude: MilliArcSeconds::new(1),
            longitude: MilliArcSeconds::new(2),
            height_gps: Centimeters::new(3),
            height_msl: Centimeters::new(4),
        };
        const Q_ERR: Picoseconds = Picoseconds::new(-12_400);
        let mut r = receiver();
        r.fix = Some(FIX);
        r.sky.satellites[TRACKED_SLOT].present = true;
        r.sky.satellites[TRACKED_SLOT].quality = CODE_LOCKED;
        r.measurements = Some(Measurements { tow: MEASURED, ..Measurements::new() });
        r.clock = Some(clock(MEASURED_MS));
        r.q_err = Some(Q_ERR);
        r.leap_seconds(LEAP_SECONDS);
        for _ in 1..MODULE_SILENT.get() {
            r.missed();
        }
        assert_eq!(r.sky.iter().count(), 1);
        r.missed();
        assert_eq!(r.sky.iter().count(), 0);
        assert!(r.measurements.is_none() && r.clock.is_none() && r.q_err.is_none() && r.pvt.is_none());
        assert_eq!((r.fix, r.leap_seconds), (Some(FIX), Some(LEAP_SECONDS)));
    }

    /// The counted time takes each fixed epoch's and counts on a second
    /// at a time without one.
    #[test]
    fn the_counted_time_runs_without_a_fix() {
        /// A fixed epoch's GPS time, s.
        const FIXED: GpsSeconds = GpsSeconds::new(1_000_000);
        let mut r = receiver();
        r.second(Some(FIXED));
        r.second(None);
        r.missed();
        assert_eq!(r.counted, Some(FIXED + Seconds::new(2)));
        r.second(Some(FIXED));
        assert_eq!(r.counted, Some(FIXED));
    }

    /// Only an epoch ends the module's silence: an epoch without a fix
    /// does, any other message does not.
    #[test]
    fn epochs_alone_end_the_silence() {
        let mut r = receiver();
        for _ in 1..MODULE_SILENT.get() {
            r.missed();
        }
        r.second(None);
        assert_eq!(r.silent, Seconds::new(0));
        for _ in 0..MODULE_SILENT.get() {
            r.missed();
        }
        assert_eq!(r.silent, MODULE_SILENT);
    }

    /// Started on new measurements, stepped a satellite at a time, and not
    /// started again on the same ones.
    #[test]
    fn corrections_start_once_an_epoch() {
        let mut r = receiver();
        r.measurements = Some(Measurements { tow: MEASURED, ..Measurements::new() });
        r.clock = Some(clock(MEASURED_MS));
        r.ecef = Some(NORTH_POLE_CM.map(Centimeters::new));
        r.start_corrections();
        assert!(r.differential.busy());
        assert!(!r.differential.step(&r.nav, PRNS - 1));
        assert!(r.differential.step(&r.nav, 1));
        r.start_corrections();
        assert!(!r.differential.busy());
    }

    /// Six satellites the module uses, channel 2's 2.5 us off: isolated
    /// here, but in the module's solution, so an alarm until the module
    /// drops it itself.
    #[test]
    fn a_satellite_isolated_but_used_by_the_module_is_an_alarm() {
        /// The satellites' residuals: 3.6 m, and the isolated one's
        /// 749.5 m, 2.5 us.  An alarm limit of 1000 ns, in its unit.
        const ISOLATED: usize = 2;
        const RESIDUAL: Decimeters = Decimeters::new(36);
        const ISOLATED_RESIDUAL: Decimeters = Decimeters::new(7_495);
        const ALARM: HundredNanoseconds = HundredNanoseconds::new(10);
        let settings = ReceiverSettings {
            traim: Raim::On,
            traim_alarm: ALARM,
            position_hold: true,
            ..ReceiverSettings::POWER_UP
        };
        let mut r = receiver();
        for c in 0..r.channels() {
            r.selector.assignment.prns[c] = c as u8 + 1;
            r.selector.assignment.position |= 1 << c;
            // Code and carrier locked, used, with an ephemeris.
            r.sky.satellites[c] = Satellite {
                present: true,
                quality: LOCKED,
                used: true,
                ephemeris: true,
                orbit: true,
                residual: if c == ISOLATED { ISOLATED_RESIDUAL } else { RESIDUAL },
                ..Satellite::new()
            };
        }
        r.evaluate_traim(&settings);
        let t = r.traim.unwrap();
        assert_eq!(
            (t.solution, t.removed, r.traim_isolated),
            (Solution::Alarm, 1 << ISOLATED, 1 << ISOLATED)
        );
        // The module's own RAIM drops it.
        r.sky.satellites[ISOLATED].used = false;
        r.evaluate_traim(&settings);
        assert_eq!((r.traim.unwrap().solution, r.traim_isolated), (Solution::Ok, 0));
    }

    /// A satellite off the channels the module uses makes the pulse too:
    /// Time RAIM checks it, and its isolation marks no channel.
    #[test]
    fn a_used_satellite_off_the_channels_is_checked() {
        /// The channels' satellites' residuals, 3.6 m, and an off-channel
        /// one's, 749.5 m; an alarm limit of 1000 ns, in its unit.
        const RESIDUAL: Decimeters = Decimeters::new(36);
        const OFF_RESIDUAL: Decimeters = Decimeters::new(7_495);
        const ALARM: HundredNanoseconds = HundredNanoseconds::new(10);
        let settings = ReceiverSettings {
            traim: Raim::On,
            traim_alarm: ALARM,
            position_hold: true,
            ..ReceiverSettings::POWER_UP
        };
        let used = |residual| Satellite {
            present: true,
            quality: LOCKED,
            used: true,
            ephemeris: true,
            orbit: true,
            residual,
            ..Satellite::new()
        };
        let mut r = receiver();
        let off = r.channels();
        for c in 0..off {
            r.selector.assignment.prns[c] = c as u8 + 1;
            r.selector.assignment.position |= 1 << c;
            r.sky.satellites[c] = used(RESIDUAL);
        }
        r.sky.satellites[off] = used(OFF_RESIDUAL);
        r.evaluate_traim(&settings);
        let t = r.traim.unwrap();
        assert_eq!((t.solution, t.removed, r.traim_isolated), (Solution::Alarm, 1 << off, 0));
    }
}
