//! The receiver's state as a smartclock-gps [`Record`]: one second of what
//! the module has told it, for an engine that speaks the Oncore from it.
//! A record is filled in place: it is too large for the stack (AGENTS.md,
//! Stack and RAM).

use smartclock_gps::engine::settings::Setting;
use smartclock_gps::engine::settings::SettingSet;
use smartclock_gps::record::Almanac;
use smartclock_gps::record::Antenna;
use smartclock_gps::record::Correction;
use smartclock_gps::record::Corrections;
use smartclock_gps::record::DopKind;
use smartclock_gps::record::Dops;
use smartclock_gps::record::Ecef;
use smartclock_gps::record::Fix;
use smartclock_gps::record::GpsTime;
use smartclock_gps::record::Health;
use smartclock_gps::record::Position;
use smartclock_gps::record::Raim;
use smartclock_gps::record::RaimAbility;
use smartclock_gps::record::RaimSolution;
use smartclock_gps::record::Record;
use smartclock_gps::record::Satellite;
use smartclock_gps::record::SatelliteFlag;
use smartclock_gps::record::SatelliteFlags;
use smartclock_gps::record::Satellites;
use smartclock_gps::record::SelfTest;
use smartclock_gps::record::Solver;
use smartclock_gps::record::Timing;
use smartclock_gps::record::Tracking;
use smartclock_gps::record::Ura;
use smartclock_gps::record::Velocity;
use smartclock_gps::record::VelocityAxes;
use smartclock_gps::record::XDOP_SATELLITES;
use smartclock_gps::record::XdopCombination;
use smartclock_gps::record::XdopTable;
use smartclock_gps::record::field::Declaration;
use smartclock_gps::record::field::FieldSet;
use smartclock_gps::record::field::RecordField;
use smartclock_gps::time;
use smartclock_gps::units;

use crate::channels::DOP_MAX;
use crate::channels::dop_index;
use crate::channels::dop_tenths;
use crate::channels::dops;
use crate::channels::line_of_sight;
use crate::gnss::LEAP_INSERTED;
use crate::gnss::three_d;
use crate::nav::Broadcast;
use crate::nav::Subframes;
use crate::receiver::MODULE_SILENT;
use crate::receiver::Receiver;
use crate::receiver::ReceiverSettings;
use crate::sky;
use crate::sky::PRNS;
use crate::sky::Prn;
use crate::sky::PrnSet;
use crate::solution::clock_terms;
use crate::solution::doppler;
use crate::solution::fix_type;
use crate::solution::inverse_matrix;
use crate::solution::magnetic_variation;
use crate::solution::matrix_tenths;
use crate::solution::position_satellites;
use crate::solution::time_estimate;
use crate::solution::velocity_axes;
use crate::traim::Capability;
use crate::traim::Solution;
use crate::units::CentimetersPerSecond;
use crate::units::DEGREES_PER_CIRCLE;
use crate::units::Decidegrees;
use crate::units::Degrees;
use crate::units::GpsSeconds;
use crate::units::GpsWeek;
use crate::units::Nanoseconds;
use crate::units::TENTHS_PER_CIRCLE;
use smartclock_ubx::nav::FixType;

/// What [`fill`] fills: every field but the corrections' age, which
/// NAV-PVT does not give; and the engine's settings the receiver, the
/// channels and the module act on (`ReceiverSettings`, `Control`).  The
/// settings the images set that it does not act on are the owner's
/// exceptions, `docs/gps-setting-exceptions.tsv`.
pub const DECLARED: Declaration = Declaration {
    name: "oncore-core",
    fills: FieldSet::of(&[
        RecordField::Time,
        RecordField::Subsecond,
        RecordField::UtcOffset,
        RecordField::Leap,
        RecordField::Fix,
        RecordField::Differential,
        RecordField::Latitude,
        RecordField::Longitude,
        RecordField::EllipsoidHeight,
        RecordField::MeanSeaLevelHeight,
        RecordField::Speed,
        RecordField::Heading,
        RecordField::GeometricDop,
        RecordField::PositionDop,
        RecordField::HorizontalDop,
        RecordField::VerticalDop,
        RecordField::TimeDop,
        RecordField::Sawtooth,
        RecordField::ReceiverMode,
        RecordField::Sigma,
        RecordField::Satellites,
        RecordField::SatelliteElevation,
        RecordField::SatelliteAzimuth,
        RecordField::SatelliteSignal,
        RecordField::SatelliteTracking,
        RecordField::SatelliteUsed,
        RecordField::SatelliteHealth,
        RecordField::SatelliteEphemeris,
        RecordField::SatelliteDoppler,
        RecordField::SatelliteTimeError,
        RecordField::Almanac,
        RecordField::Antenna,
        RecordField::SelfTest,
        RecordField::Ecef,
        RecordField::Xdops,
        RecordField::Corrections,
        RecordField::RaimSolution,
        RecordField::RaimAbility,
        RecordField::RaimIsolated,
        RecordField::SatelliteFlags,
        RecordField::SatelliteUra,
        RecordField::MagneticVariation,
        RecordField::VelocityAxes,
        RecordField::Solver,
    ]),
    acts: SettingSet::of(&[
        Setting::MaskAngle,
        Setting::IgnoreList,
        Setting::SatelliteSelect,
        Setting::ManualSatellites,
        Setting::FixAlgorithm,
        Setting::DopKind,
        Setting::XdopHysteresis,
        Setting::PositionHold,
        Setting::HeldPosition,
        Setting::AltitudeHold,
        Setting::AltitudeHoldHeight,
        Setting::HdopThreshold,
        Setting::RaimAlgorithm,
        Setting::RaimAlarmLimit,
        Setting::Application,
        Setting::InitialPosition,
        Setting::TimeBase,
        Setting::CableDelay,
        Setting::PulseOffset,
        Setting::PulseMode,
    ]),
    // The module's pulse, shifted by cable_delay and pulse_offset.
    makes_pulse: true,
};

/// The record's azimuth circle and heading circle.
const CIRCLE: Degrees = Degrees::new(DEGREES_PER_CIRCLE);
const HEADING_CIRCLE: Decidegrees<i32> = Decidegrees::new(TENTHS_PER_CIRCLE);
/// The record's ground speed range.
const SPEED_MAX: CentimetersPerSecond<i32> = CentimetersPerSecond::new(u16::MAX as i32);
/// The lowest elevation a satellite off the channels is given at: the
/// horizon, as the UT Plus counts visible (UT p. 6.23).
const HORIZON: Degrees<i8> = Degrees::new(0);

/// What a self-test would find: the module tells of no hardware fault,
/// and no channel can track while it is silent.
fn self_test(receiver: &Receiver) -> SelfTest {
    let mut t = SelfTest::PASS;
    if receiver.silent >= MODULE_SILENT {
        t.channels = (1 << receiver.channels) - 1;
    }
    t
}

/// Fills `r` with the receiver's second, as `settings` have it.  The
/// satellites go in the order to take them up: the channels' first, in
/// channel order, then the rest in view, the highest first.
pub fn fill(receiver: &Receiver, settings: &ReceiverSettings, r: &mut Record) {
    let rx = receiver;
    let pvt = rx.pvt;
    r.time = rx.time.map(|t| GpsTime {
        seconds: gps_seconds(t),
        subsecond: units::Nanoseconds(pvt.map_or(0, |p| p.nano.max(Nanoseconds::new(0)).get() as u32)),
    });
    r.utc_offset = rx.leap_seconds.map(|l| units::Seconds(l.get()));
    r.leap = rx.leap_event.map(|l| time::LeapEvent {
        at: gps_seconds(l.at),
        change: if l.change == LEAP_INSERTED { time::LeapChange::Insert } else { time::LeapChange::Remove },
    });
    r.receiver_mode = rx.mode;
    r.differential = pvt.is_some_and(|p| p.differential);
    // NAV-PVT gives no age for the corrections the module uses.
    r.differential_age = None;
    r.position = rx.fix.map(|f| Position {
        latitude: units::MilliArcSeconds(f.latitude.get()),
        longitude: units::MilliArcSeconds(f.longitude.get()),
        ellipsoid: units::Centimeters(f.height_gps.get()),
        mean_sea_level: units::Centimeters(f.height_msl.get()),
    });
    r.velocity = pvt.map(|p| Velocity {
        speed: units::CentimetersPerSecond(
            p.ground_speed.centimeters_per_second().clamp(CentimetersPerSecond::new(0), SPEED_MAX).get()
                as u16,
        ),
        heading: units::TenthDegrees(p.heading.decidegrees().rem_euclid(HEADING_CIRCLE).get() as u16),
    });
    r.dops = dops(position_satellites(receiver).map(|s| line_of_sight(s.elevation, s.azimuth))).map(|d| {
        let tenths = |kind: DopKind| units::Tenths(dop_tenths(d[dop_index(kind)], DOP_MAX).get());
        Dops {
            geometric: tenths(DopKind::Geometric),
            position: tenths(DopKind::Position),
            horizontal: tenths(DopKind::Horizontal),
            vertical: tenths(DopKind::Vertical),
            time: tenths(DopKind::Time),
        }
    });
    r.fix = fix(receiver, settings, r.dops.map(|d| d.horizontal));
    let mut timing = Timing::ZERO;
    timing.sawtooth = units::Nanoseconds(rx.q_err.map_or(0, |q| (-q.round_ns()).saturating_i8().get()));
    timing.sigma = units::Nanoseconds(rx.traim.map_or(0, |t| t.sigma.round_u16().get()));
    r.timing = Some(timing);
    satellites(receiver, &mut r.satellites);
    r.almanac = almanac(receiver);
    r.antenna = match rx.antenna {
        Antenna::Open | Antenna::Shorted => rx.antenna,
        Antenna::Unknown | Antenna::Connected => Antenna::Connected,
    };
    r.self_test = self_test(receiver);
    let d = &mut r.derived;
    d.ecef = rx.ecef.map(|[x, y, z]| Ecef {
        x: units::Centimeters(x.get()),
        y: units::Centimeters(y.get()),
        z: units::Centimeters(z.get()),
    });
    xdops(receiver, settings, &mut d.xdops);
    corrections(receiver, &mut d.corrections);
    d.raim = raim(receiver);
    d.magnetic_variation =
        rx.counted.and_then(|t| magnetic_variation(receiver, t)).map(|v| units::TenthDegreesEast(v.get()));
    d.velocity = velocity_axes(receiver).map(|[north, east, up]| VelocityAxes {
        north: units::DecimetersPerSecond(north.get()),
        east: units::DecimetersPerSecond(east.get()),
        up: units::DecimetersPerSecond(up.get()),
    });
    d.solver = solver(receiver);
}

fn gps_seconds(t: GpsSeconds) -> time::GpsSeconds {
    time::GpsSeconds::from_epoch(units::Seconds(t.get()))
}

/// None in a second without the module's fixed epoch; else time only in
/// position hold, else the fix's dimension; propagated for
/// the module's dead reckoning, which the LEA-M8T never does (CFG-NAV5's
/// drLimit is reserved, UBX-13003221 p. 232).  With the altitude held, an
/// HDOP over the threshold gives the fix up (VP p. 15); `hdop` is the
/// channels' satellites'.
fn fix(receiver: &Receiver, settings: &ReceiverSettings, hdop: Option<units::Tenths>) -> Fix {
    if receiver.time.is_none() {
        return Fix::None;
    }
    if settings.position_hold {
        return Fix::TimeOnly;
    }
    if settings.altitude_hold && hdop.is_some_and(|h| h.0 > settings.hdop_threshold.get()) {
        return Fix::None;
    }
    match fix_type(receiver) {
        t if three_d(t) => Fix::ThreeD,
        FixType::TwoD => Fix::TwoD,
        FixType::DeadReckoning => Fix::Propagated,
        _ => Fix::None,
    }
}

/// The satellites in view into `out`: the channels' first, then the rest
/// whose place in the sky is known, above the horizon, the highest
/// first.
fn satellites(receiver: &Receiver, out: &mut Satellites) {
    let a = &receiver.selector.assignment;
    let channels = &a.prns[..receiver.channels];
    *out = Satellites::EMPTY;
    for (c, &prn) in channels.iter().enumerate() {
        if let Some(prn) = Prn::new(prn)
            && let Some(s) = satellite(receiver, prn, a.position & (1 << c) != 0)
        {
            out.push(s);
        }
    }
    // The rest, highest first: an insertion sort, for at most 32.
    let mut rest = [(Degrees::new(i8::MIN), Prn::from_index(0)); PRNS];
    let mut n = 0;
    for (prn, s) in receiver.sky.iter() {
        if s.present && s.located() && s.elevation >= HORIZON && !channels.contains(&prn.get()) {
            let mut j = n;
            while j > 0 && rest[j - 1].0 < s.elevation {
                rest[j] = rest[j - 1];
                j -= 1;
            }
            rest[j] = (s.elevation, prn);
            n += 1;
        }
    }
    for &(_, prn) in &rest[..n] {
        if let Some(s) = satellite(receiver, prn, false) {
            out.push(s);
        }
    }
}

/// Satellite `prn`, `positioning` if its channel gives position.
fn satellite(receiver: &Receiver, prn: Prn, positioning: bool) -> Option<Satellite> {
    let s = receiver.sky.get(prn.get())?;
    let id = units::Prn::new(prn.get())?;
    let azimuth = s.azimuth.rem_euclid(CIRCLE).get() as u16;
    let mut out = Satellite::new(id, units::Degrees(s.elevation.get()), units::Degrees(azimuth));
    let nav = &receiver.nav;
    out.signal = s.mode().map(|_| units::DecibelHertz(s.cno.get()));
    out.tracking = tracking(s, &nav.ephemeris[prn.index()]);
    out.used = positioning && s.positioning();
    let broadcast = nav.broadcast[prn.index()];
    out.health = if broadcast.health != 0 { Health::Unhealthy } else { s.health };
    out.ephemeris = s.ephemeris;
    out.doppler = doppler(receiver, prn, s).map(|h| units::Hertz(h.get()));
    out.time_error = time_estimate(receiver, s).map(|ns| units::Nanoseconds(ns.get() as i32));
    out.flags = flags(&broadcast);
    out.ura = broadcast.ura.and_then(Ura::new);
    Some(out)
}

/// How far the module has got with satellite `s`, `subframes` its
/// ephemeris subframes, by NAV-SAT's quality indicator (UBX-13003221
/// p. 397): code locked or better is "time synchronized", its time
/// known; then its ephemeris read from, then held.  Nothing the module
/// says marks carrier lock, bit sync or message sync.
fn tracking(s: &sky::Satellite, subframes: &Subframes) -> Tracking {
    match s.quality {
        sky::NO_SIGNAL | sky::SEARCHING => Tracking::Searching,
        sky::ACQUIRED => Tracking::CodeLock,
        sky::UNUSABLE => Tracking::AgcSet,
        _ if s.ephemeris => Tracking::Position,
        _ if subframes.started() => Tracking::Ephemeris,
        _ => Tracking::FrameSync,
    }
}

/// What a satellite's subframes flag of it.  The module passes on only
/// subframes whose parity holds, so none has a parity error.
fn flags(b: &Broadcast) -> SatelliteFlags {
    [
        (b.alert, SatelliteFlag::MomentumAlert),
        (b.anti_spoof, SatelliteFlag::AntiSpoof),
        (b.inaccurate(), SatelliteFlag::Inaccurate),
    ]
    .into_iter()
    .filter(|&(set, _)| set)
    .fold(SatelliteFlags::EMPTY, |flags, (_, flag)| flags.with(flag))
}

/// The least-squares solution's inside: the matrix of the satellites
/// giving position, and the clock's terms; `None` without either.
fn solver(receiver: &Receiver) -> Option<Solver> {
    let q = inverse_matrix(receiver);
    let clock = clock_terms(receiver);
    if q.is_none() && clock.is_none() {
        return None;
    }
    let mut solver = Solver::ZERO;
    if let Some(q) = q {
        solver.matrix = matrix_tenths(&q);
    }
    if let Some(c) = clock {
        solver.clock_bias = units::Decimeters(c.bias.get());
        solver.oscillator_offset = units::DecimetersPerSecond(c.drift.get());
    }
    Some(solver)
}

/// The almanac held: its week, from page 25, and the satellites it
/// covers; `None` before page 25 or any satellite's almanac.
fn almanac(receiver: &Receiver) -> Option<Almanac> {
    let nav = &receiver.nav;
    let held: PrnSet = Prn::all().zip(&nav.almanac).filter(|(_, a)| a.is_some()).map(|(p, _)| p).collect();
    let (toa, week) = nav.almanac_reference.filter(|_| held != PrnSet::EMPTY)?;
    let now = GpsWeek::containing(receiver.counted?)?;
    Some(Almanac::new(
        time::GpsWeek::new(now.nearest(week).get()),
        toa,
        units::PrnSet::from_bits(held.bits()),
    ))
}

/// The best combinations of the channel selection, their xDOP the one
/// `@@Ag` selects.
fn xdops(receiver: &Receiver, settings: &ReceiverSettings, out: &mut Option<XdopTable>) {
    let kind = settings.dop_kind;
    let per_combination = receiver.selector.per_combination.min(XDOP_SATELLITES);
    let mut table = XdopTable::new(kind);
    for (dop, prns) in receiver.selector.best.iter().flatten() {
        let mut c = XdopCombination::EMPTY;
        c.dop = units::Tenths(dop.get());
        for (slot, &prn) in c.satellites.iter_mut().zip(&prns[..per_combination]) {
            *slot = units::Prn::new(prn);
        }
        table.push(c);
    }
    *out = (!table.is_empty()).then_some(table);
}

/// Each channel's pseudorange correction from the last whole set, its
/// rate held to the record's range.
fn corrections(receiver: &Receiver, out: &mut Option<Corrections>) {
    let Some(latest) = receiver.differential.latest() else {
        *out = None;
        return;
    };
    let mut c = Corrections::new(units::Deciseconds(latest.time.tenths_of_week().get()));
    let channels = &receiver.selector.assignment.prns[..receiver.channels];
    for (slot, &prn) in c.satellites.iter_mut().zip(channels) {
        *slot = Prn::new(prn).and_then(|p| {
            let k = latest.corrections[p.index()].as_ref()?;
            let rate = k.rate.millimeters_per_second().round().get();
            Some(Correction {
                prn: units::Prn::new(prn)?,
                range: units::Centimeters(k.range.centimeters().round().get() as i32),
                rate: units::MillimetersPerSecond(rate.clamp(i16::MIN.into(), i16::MAX.into()) as i16),
                ephemeris_issue: k.iode,
            })
        });
    }
    *out = Some(c);
}

/// Time RAIM's last evaluation, with the satellites it isolated.
fn raim(receiver: &Receiver) -> Option<Raim> {
    let t = receiver.traim?;
    let solution = match t.solution {
        Solution::Ok => RaimSolution::Ok,
        Solution::Alarm => RaimSolution::Alarm,
        Solution::Unknown => RaimSolution::Unknown,
    };
    let ability = match t.capability {
        Capability::DetectAndIsolate => RaimAbility::Isolation,
        Capability::DetectOnly => RaimAbility::Detection,
        Capability::Neither => RaimAbility::Neither,
    };
    let mut raim = Raim::new(solution, ability);
    let channels = &receiver.selector.assignment.prns[..receiver.channels];
    for (c, &prn) in channels.iter().enumerate() {
        if receiver.traim_isolated & (1 << c) != 0
            && let Some(id) = units::Prn::new(prn)
        {
            raim.isolated = raim.isolated.with(id);
        }
    }
    Some(raim)
}

#[cfg(test)]
mod tests {
    use smartclock_gps::engine::identity::VP_SIX;
    use smartclock_gps::engine::settings::CHANNELS_MOST;
    use smartclock_gps::engine::settings::SettingSet;
    use smartclock_gps::record::Fix;
    use smartclock_gps::record::Health;
    use smartclock_gps::record::ReceiverMode;
    use smartclock_gps::record::Record;
    use smartclock_gps::record::SatelliteFlag;
    use smartclock_gps::record::SatelliteFlags;
    use smartclock_gps::record::SelfTest;
    use smartclock_gps::record::Tracking;
    use smartclock_gps::units;

    use smartclock_gps::record::field::FieldSet;
    use smartclock_gps::record::field::UNFILLED;
    use smartclock_gps::record::field::changed;

    use super::DECLARED;
    use super::fill;
    use crate::civil::DateTime;
    use crate::differential::Correction;
    use crate::fixtures::HEIGHT_GPS;
    use crate::fixtures::HEIGHT_GPS_MM;
    use crate::fixtures::HEIGHT_MSL;
    use crate::fixtures::HEIGHT_MSL_MM;
    use crate::fixtures::LATITUDE;
    use crate::fixtures::LEAP_SECONDS;
    use crate::fixtures::LONGITUDE;
    use crate::fixtures::PVT_SATELLITES;
    use crate::fixtures::prn;
    use crate::gnss::Clock;
    use crate::gnss::LEAP_INSERTED;
    use crate::gnss::LeapEvent;
    use crate::gnss::Pvt;
    use crate::nav::Almanac;
    use crate::nav::Subframes;
    use crate::receiver::Fix as ModuleFix;
    use crate::receiver::MODULE_SILENT;
    use crate::receiver::Receiver;
    use crate::receiver::ReceiverSettings;
    use crate::sky::ACQUIRED;
    use crate::sky::CODE_LOCKED;
    use crate::sky::LOCKED;
    use crate::sky::NO_SIGNAL;
    use crate::sky::SEARCHING;
    use crate::sky::Satellite;
    use crate::sky::UNUSABLE;
    use crate::solution::MATRIX_ELEMENTS;
    use crate::solution::inverse_matrix;
    use crate::solution::matrix_tenths;
    use crate::traim::Capability;
    use crate::traim::Result as TraimResult;
    use crate::traim::Solution;
    use crate::units::Centimeters;
    use crate::units::DecibelHertz;
    use crate::units::Decimeters;
    use crate::units::Degrees;
    use crate::units::Degrees1e5;
    use crate::units::Degrees1e7;
    use crate::units::GpsSeconds;
    use crate::units::GpsWeek;
    use crate::units::Hertz;
    use crate::units::Meters;
    use crate::units::MetersPerSecond;
    use crate::units::MillimetersPerSecond;
    use crate::units::Nanoseconds;
    use crate::units::NanosecondsPerSecond;
    use crate::units::Picoseconds;
    use crate::units::Seconds;
    use crate::units::Tenths;
    use crate::units::TimeOfWeek;
    use crate::vector::AXES;
    use smartclock_ubx::nav::FixType;

    /// The tests' GPS time.
    const GPS_TIME: DateTime = DateTime { year: 2026, month: 10, day: 27, hour: 1, minute: 2, second: 21 };
    /// A satellite giving position: locked, used, healthy, with an
    /// ephemeris.
    const POSITIONING: Satellite = Satellite {
        quality: LOCKED,
        used: true,
        health: Health::Healthy,
        ephemeris: true,
        orbit: true,
        ..Satellite::new()
    };
    const CNO: DecibelHertz = DecibelHertz::new(40);
    /// On channels 0 and 1, the first giving position: PRN and elevation.
    const CHANNELS: [(u8, i8); 2] = [(12, 20), (3, 70)];
    /// In view on no channel, PRN and elevation: not by elevation.
    const OFF_CHANNEL: [(u8, i8); 2] = [(5, 10), (30, 50)];
    /// An almanac's toa, raw, and the PRNs it covers.
    const TOA: u8 = 144;
    const ALMANAC_PRNS: [u8; 2] = [1, 32];
    /// NAV-PVT's velocity north, east and down, and the record's north,
    /// east and up, 0.1 m/s, truncated.
    const PVT_VELOCITY: [MillimetersPerSecond; AXES] =
        [MillimetersPerSecond::new(1234), MillimetersPerSecond::new(-567), MillimetersPerSecond::new(890)];
    const VELOCITY_AXES: [i16; AXES] = [12, -5, -8];
    /// NAV-CLOCK: 1000 ns behind, drifting -20 ns/s; the record's clock
    /// bias, 0.1 m, and the drift's magnitude, 0.1 m/s.
    const CLOCK: Clock = Clock {
        tow: TimeOfWeek::from_ms(0),
        bias: Nanoseconds::new(1000),
        drift: NanosecondsPerSecond::new(-20),
    };
    const CLOCK_BIAS: i16 = 2998;
    const OSCILLATOR_OFFSET: u16 = 59;
    /// A URA index over 16 m, by IS-GPS-200N 20.3.3.3.1.3.
    const URA_INACCURATE: u8 = 7;
    /// Four satellites giving position, PRN, elevation and azimuth.
    const GIVING_POSITION: [(u8, i8, i16); 4] = [(2, 80, 0), (9, 30, 90), (14, 25, 200), (27, 40, 300)];

    fn gps_time() -> GpsSeconds {
        GpsSeconds::from_unix(GPS_TIME.to_unix())
    }

    /// Nothing heard from the module, for the six-channel VP.
    fn power_up() -> Receiver {
        Receiver::new(VP_SIX.model().channels())
    }

    /// The module's time heard.
    fn receiver() -> Receiver {
        let mut e = power_up();
        e.second(Some(gps_time()));
        e
    }

    /// `satellite`, in view at `elevation`.
    fn put(e: &mut Receiver, number: u8, elevation: i8, satellite: Satellite) {
        e.sky.satellites[prn(number).index()] =
            Satellite { present: true, elevation: Degrees::new(elevation), cno: CNO, ..satellite };
    }

    /// For the receiver every field is filled from: a fraction of the
    /// second, a ground speed and heading, a leap second's offset from
    /// now, an ECEF position, cm, the quantization error, ps, Time RAIM's
    /// sigma, ns, a measured Doppler, a residual, an xDOP, a correction's
    /// range, m, rate, m/s, and issue, and the corrections' time, s.
    const NANO: Nanoseconds = Nanoseconds::new(250);
    const GROUND_SPEED: MillimetersPerSecond = MillimetersPerSecond::new(1500);
    const HEADING: Degrees1e5 = Degrees1e5::new(9_000_000);
    const LEAP_IN: Seconds<i64> = Seconds::new(86_400);
    const ECEF: [Centimeters; AXES] =
        [Centimeters::new(-268_960_000), Centimeters::new(-429_100_000), Centimeters::new(385_400_000)];
    const Q_ERR: Picoseconds = Picoseconds::new(-3000);
    const SIGMA: Nanoseconds<f32> = Nanoseconds::new(5.0);
    const MEASURED: Hertz<i16> = Hertz::new(-1234);
    const RESIDUAL: Decimeters = Decimeters::new(30);
    const XDOP: Tenths = Tenths::new(15);
    const RANGE: Meters = Meters::new(1.25);
    const RATE: MetersPerSecond = MetersPerSecond::new(-0.5);
    const IODE: u8 = 42;
    const CORRECTIONS_TIME: Seconds = Seconds::new(100.0);

    /// NAV-PVT of `fix_type`, OK, moving at [`PVT_VELOCITY`].
    fn pvt(fix_type: FixType) -> Pvt {
        Pvt {
            tow: TimeOfWeek::from_ms(0),
            nano: Nanoseconds::new(0),
            fix_type,
            fix_ok: true,
            differential: false,
            satellites: PVT_SATELLITES,
            longitude: Degrees1e7::new(0),
            latitude: Degrees1e7::new(0),
            height: HEIGHT_GPS_MM,
            height_msl: HEIGHT_MSL_MM,
            velocity: PVT_VELOCITY,
            ground_speed: MillimetersPerSecond::new(0),
            heading: Degrees1e5::new(0),
        }
    }

    fn filled(e: &Receiver) -> Box<Record> {
        filled_as(e, &ReceiverSettings::POWER_UP)
    }

    fn filled_as(e: &Receiver, settings: &ReceiverSettings) -> Box<Record> {
        let mut r = Box::new(Record::EMPTY);
        fill(e, settings, &mut r);
        r
    }

    #[test]
    fn nothing_before_the_module() {
        let r = filled(&power_up());
        assert_eq!(r.time, None);
        assert_eq!(r.fix, Fix::None);
        assert!(r.satellites.is_empty());
        assert_eq!((r.position, r.velocity, r.dops, r.almanac), (None, None, None, None));
        assert_eq!((r.derived.xdops, r.derived.corrections, r.derived.raim), (None, None, None));
        assert_eq!(
            (r.derived.magnetic_variation, r.derived.velocity, r.derived.solver, r.differential_age),
            (None, None, None, None)
        );
    }

    #[test]
    fn a_silent_modules_channels_fail_the_self_test() {
        /// Every channel of the six-channel VP.
        const SIX_CHANNELS: u16 = 0b11_1111;
        let mut e = power_up();
        assert_eq!(filled(&e).self_test, SelfTest::PASS);
        e.silent = MODULE_SILENT;
        let t = filled(&e).self_test;
        assert_eq!((t.hardware, t.channels), (SelfTest::PASS.hardware, SIX_CHANNELS));
    }

    #[test]
    fn the_time_once_set() {
        let e = receiver();
        assert_eq!(filled(&e).time.map(|t| t.seconds.since_epoch().0), Some(gps_time().get()));
    }

    #[test]
    fn channels_first_then_the_highest() {
        let mut e = receiver();
        for (c, (number, elevation)) in CHANNELS.into_iter().enumerate() {
            put(&mut e, number, elevation, POSITIONING);
            e.selector.assignment.prns[c] = number;
        }
        e.selector.assignment.position = 1 << 0;
        for (number, elevation) in OFF_CHANNEL {
            put(&mut e, number, elevation, POSITIONING);
        }
        let r = filled(&e);
        let order: Vec<u8> = r.satellites.iter().map(|s| s.prn.get()).collect();
        assert_eq!(order, [CHANNELS[0].0, CHANNELS[1].0, OFF_CHANNEL[1].0, OFF_CHANNEL[0].0]);
        let used: Vec<bool> = r.satellites.iter().map(|s| s.used).collect();
        assert_eq!(used, [true, false, false, false]);
        for s in r.satellites.iter() {
            assert_eq!(
                (s.tracking, s.health, s.signal.map(|c| c.0)),
                (Tracking::Position, Health::Healthy, Some(CNO.get()))
            );
        }
    }

    #[test]
    fn health_unknown_until_reported() {
        let mut e = receiver();
        put(&mut e, CHANNELS[0].0, CHANNELS[0].1, Satellite { quality: LOCKED, ..Satellite::new() });
        e.selector.assignment.prns[0] = CHANNELS[0].0;
        let r = filled(&e);
        let s = r.satellites.iter().next().expect("in view");
        assert_eq!((s.health, s.tracking, s.used), (Health::Unknown, Tracking::FrameSync, false));
    }

    #[test]
    fn position_hold_is_time_only() {
        let settings = ReceiverSettings { position_hold: true, ..ReceiverSettings::POWER_UP };
        assert_eq!(filled_as(&receiver(), &settings).fix, Fix::TimeOnly);
    }

    /// A missed second, or an epoch whose NAV-PVT had no fix, keeps the
    /// last NAV-PVT but gives no fix, holding or not.
    #[test]
    fn no_fix_in_a_second_without_a_fixed_epoch() {
        let mut e = receiver();
        e.pvt = Some(pvt(FixType::ThreeD));
        assert_eq!(filled(&e).fix, Fix::ThreeD);
        let holding = ReceiverSettings { position_hold: true, ..ReceiverSettings::POWER_UP };
        e.missed();
        assert_eq!((filled(&e).fix, filled_as(&e, &holding).fix), (Fix::None, Fix::None));
        e.second(Some(gps_time()));
        e.second(None);
        assert_eq!((filled(&e).fix, filled_as(&e, &holding).fix), (Fix::None, Fix::None));
    }

    #[test]
    fn the_almanac_week_in_full() {
        let mut e = receiver();
        let almanac = Almanac::zero_at(TOA);
        for number in ALMANAC_PRNS {
            e.nav.almanac[prn(number).index()] = Some(almanac);
        }
        let week = GpsWeek::containing(gps_time()).expect("a week").previous();
        assert_eq!(filled(&e).almanac, None);
        e.nav.almanac_reference = Some((TOA, week.almanac_week()));
        let a = filled(&e).almanac.expect("an almanac");
        assert_eq!((a.week.get(), a.time_of_almanac), (week.get(), TOA));
        assert_eq!(
            a.held,
            ALMANAC_PRNS
                .into_iter()
                .fold(units::PrnSet::EMPTY, |set, p| set.with(units::Prn::new(p).unwrap()))
        );
    }

    /// The almanac stays through seconds without a fix, missed or not,
    /// as a VP keeps its own.
    #[test]
    fn the_almanac_outlasts_the_fix() {
        let mut e = receiver();
        e.nav.almanac[prn(ALMANAC_PRNS[0]).index()] = Some(Almanac::zero_at(TOA));
        let week = GpsWeek::containing(gps_time()).expect("a week");
        e.nav.almanac_reference = Some((TOA, week.almanac_week()));
        e.second(None);
        e.missed();
        assert_eq!(filled(&e).almanac.map(|a| a.week.get()), Some(week.get()));
        assert_eq!(power_up().counted, None);
    }

    #[test]
    fn tracking_by_quality() {
        // NAV-SAT's quality, whether it holds the ephemeris and has read
        // any of its subframes, and how far the record says it has got.
        const CASES: [(u8, bool, bool, Tracking); 8] = [
            (NO_SIGNAL, false, false, Tracking::Searching),
            (SEARCHING, false, false, Tracking::Searching),
            (ACQUIRED, false, false, Tracking::CodeLock),
            (UNUSABLE, false, false, Tracking::AgcSet),
            (CODE_LOCKED, false, false, Tracking::FrameSync),
            (LOCKED, false, false, Tracking::FrameSync),
            (LOCKED, false, true, Tracking::Ephemeris),
            (CODE_LOCKED, true, true, Tracking::Position),
        ];
        let (number, elevation) = CHANNELS[0];
        for (quality, ephemeris, started, want) in CASES {
            let mut e = receiver();
            put(
                &mut e,
                number,
                elevation,
                Satellite { quality, ephemeris, orbit: ephemeris, ..Satellite::new() },
            );
            if started {
                e.nav.ephemeris[prn(number).index()] = Subframes::first();
            }
            e.selector.assignment.prns[0] = number;
            let r = filled(&e);
            let s = r.satellites.iter().next().expect("on a channel");
            assert_eq!(s.tracking, want, "quality {quality}, ephemeris {ephemeris}, started {started}");
        }
    }

    #[test]
    fn flags_and_ura_from_the_subframes() {
        let mut e = receiver();
        for (c, (number, elevation)) in CHANNELS.into_iter().enumerate() {
            put(&mut e, number, elevation, POSITIONING);
            e.selector.assignment.prns[c] = number;
        }
        let b = &mut e.nav.broadcast[prn(CHANNELS[0].0).index()];
        (b.alert, b.anti_spoof, b.ura) = (true, true, Some(URA_INACCURATE));
        let r = filled(&e);
        let [flagged, plain]: [_; 2] =
            core::array::from_fn(|c| *r.satellites.iter().nth(c).expect("on a channel"));
        assert_eq!(
            flagged.flags,
            SatelliteFlags::EMPTY
                .with(SatelliteFlag::MomentumAlert)
                .with(SatelliteFlag::AntiSpoof)
                .with(SatelliteFlag::Inaccurate)
        );
        assert_eq!(flagged.ura.map(|u| u.get()), Some(URA_INACCURATE));
        assert_eq!((plain.flags, plain.ura), (SatelliteFlags::EMPTY, None));
    }

    #[test]
    fn propagated_for_dead_reckoning() {
        let mut e = receiver();
        e.pvt = Some(pvt(FixType::DeadReckoning));
        assert_eq!(filled(&e).fix, Fix::Propagated);
    }

    /// With the altitude held, an HDOP over the threshold gives the fix
    /// up; one at it keeps it.
    #[test]
    fn the_hdop_threshold_with_the_altitude_held() {
        let e = everything();
        let hdop = filled(&e).dops.expect("dops").horizontal.0;
        let fix = |threshold: u16| {
            let settings = ReceiverSettings {
                altitude_hold: true,
                hdop_threshold: Tenths::new(threshold),
                ..ReceiverSettings::POWER_UP
            };
            filled_as(&e, &settings).fix
        };
        assert_eq!(fix(hdop - 1), Fix::None);
        assert_eq!(fix(hdop), filled(&e).fix);
        assert_ne!(fix(hdop), Fix::None);
    }

    #[test]
    fn velocity_axes_and_clock_terms() {
        let mut e = receiver();
        e.pvt = Some(pvt(FixType::ThreeD));
        e.clock = Some(CLOCK);
        let r = filled(&e);
        let v = r.derived.velocity.expect("a velocity");
        assert_eq!([v.north.0, v.east.0, v.up.0], VELOCITY_AXES);
        let solver = r.derived.solver.expect("a solver");
        assert_eq!((solver.clock_bias.0, solver.oscillator_offset.0), (CLOCK_BIAS, OSCILLATOR_OFFSET));
    }

    #[test]
    fn the_matrix_of_the_satellites_giving_position() {
        let mut e = receiver();
        for (c, (number, elevation, azimuth)) in GIVING_POSITION.into_iter().enumerate() {
            put(&mut e, number, elevation, POSITIONING);
            e.sky.satellites[prn(number).index()].azimuth = Degrees::new(azimuth);
            e.selector.assignment.prns[c] = number;
            e.selector.assignment.position |= 1 << c;
        }
        let q = inverse_matrix(&e).expect("four satellites");
        let solver = filled(&e).derived.solver.expect("a solver");
        assert_eq!(solver.matrix, matrix_tenths(&q));
        assert_ne!(solver.matrix, [0; MATRIX_ELEMENTS]);
    }

    /// A receiver with something for every field [`DECLARED`] declares.
    fn everything() -> Receiver {
        let mut e = receiver();
        e.leap_seconds(LEAP_SECONDS);
        let leap_at = gps_time() + LEAP_IN;
        e.leap_event = Some(LeapEvent { at: leap_at, before: LEAP_SECONDS, change: LEAP_INSERTED });
        e.pvt = Some(Pvt {
            nano: NANO,
            differential: true,
            ground_speed: GROUND_SPEED,
            heading: HEADING,
            ..pvt(FixType::ThreeD)
        });
        e.fix = Some(ModuleFix {
            latitude: LATITUDE,
            longitude: LONGITUDE,
            height_gps: HEIGHT_GPS,
            height_msl: HEIGHT_MSL,
        });
        e.ecef = Some(ECEF);
        e.clock = Some(CLOCK);
        e.q_err = Some(Q_ERR);
        e.mode = ReceiverMode::Holding;
        let mut combination = [0; CHANNELS_MOST];
        for (c, (number, elevation, azimuth)) in GIVING_POSITION.into_iter().enumerate() {
            put(&mut e, number, elevation, POSITIONING);
            let s = &mut e.sky.satellites[prn(number).index()];
            (s.azimuth, s.residual) = (Degrees::new(azimuth), RESIDUAL);
            e.selector.assignment.prns[c] = number;
            e.selector.assignment.position |= 1 << c;
            combination[c] = number;
        }
        let first = prn(GIVING_POSITION[0].0);
        let s = &mut e.sky.satellites[first.index()];
        (s.doppler, s.measured) = (MEASURED, true);
        let b = &mut e.nav.broadcast[first.index()];
        (b.alert, b.ura) = (true, Some(URA_INACCURATE));
        e.selector.best[0] = Some((XDOP, combination));
        e.selector.per_combination = GIVING_POSITION.len();
        e.differential.set_latest(
            CORRECTIONS_TIME,
            first,
            Correction { range: RANGE, rate: RATE, iode: IODE },
        );
        e.traim = Some(TraimResult {
            solution: Solution::Ok,
            capability: Capability::DetectAndIsolate,
            sigma: SIGMA,
            offset: Nanoseconds::new(0.0),
            removed: 0,
        });
        e.traim_isolated = 1 << 0;
        let almanac = Almanac::zero_at(TOA);
        e.nav.almanac[first.index()] = Some(almanac);
        let week = GpsWeek::containing(gps_time()).expect("a week");
        e.nav.almanac_reference = Some((TOA, week.almanac_week()));
        e
    }

    /// The owner's exceptions to the settings the images set.
    const SETTING_EXCEPTIONS: &str = include_str!("../../../docs/gps-setting-exceptions.tsv");

    #[test]
    fn the_declaration_holds() {
        let receivers: [fn() -> Receiver; 3] = [power_up, receiver, everything];
        let mut shown = FieldSet::EMPTY;
        for make in receivers {
            let e = make();
            let mut r = Box::new(UNFILLED);
            fill(&e, &ReceiverSettings::POWER_UP, &mut r);
            assert_eq!(DECLARED.undeclared(&UNFILLED, &r), FieldSet::EMPTY);
            shown = shown.union(changed(&UNFILLED, &r));
        }
        assert_eq!(DECLARED.unshown(shown), FieldSet::EMPTY);
        assert_eq!(DECLARED.shortfall(""), FieldSet::EMPTY);
    }

    /// Every setting the images set is acted on or excused, and no
    /// excuse is for one acted on or one no image sets.
    #[test]
    fn the_settings_are_acted_on_or_excused() {
        assert_eq!(DECLARED.unacted(SETTING_EXCEPTIONS), SettingSet::EMPTY);
        assert_eq!(DECLARED.needless(SETTING_EXCEPTIONS), SettingSet::EMPTY);
    }
}
