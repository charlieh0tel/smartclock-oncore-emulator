//! Fifteen minutes of a real NEO-M8T (`../testdata/neo-m8t-2026-10-28.ubx`)
//! through smartclock-gps-ublox's streamer, the module reader and the
//! emulator, smartclock-gps's engine answering, each epoch ending with the
//! emulator's second, as `oncore-app` runs them.  The trace
//! is replayed once ([`REPLAY`]); each test checks one aspect of what it
//! left.

use core::ops::Range;
use smartclock_gps::engine::ReceiverControl;
use std::sync::LazyLock;

use oncore_core::emulator::Configuration;
use oncore_core::emulator::Emulator;
use oncore_core::emulator::SELECT_EPOCHS;
use oncore_core::gnss::Gnss;
use oncore_core::identity::Identities;
use oncore_core::module::Module;
use oncore_core::record::fill;
use oncore_core::sky::MODE_POSITION;
use oncore_core::sky::Prn;
use oncore_core::solution::almanac_doppler;
use oncore_core::solution::magnetic_variation;
use oncore_core::traim::Solution;
use oncore_core::units::Decidegrees;
use oncore_core::units::GpsSeconds;
use oncore_core::units::GpsWeek;
use oncore_core::units::Hertz;
use oncore_core::units::MINUTE;
use oncore_core::units::Meters;
use oncore_core::units::MetersPerSecond;
use oncore_core::units::MilliArcSeconds;
use oncore_core::units::SquareMeters;
use oncore_core::units::TENTHS_PER_SECOND;
use oncore_core::wire::OncoreReader;
use smartclock_gps::Direction;
use smartclock_gps::engine::Output;
use smartclock_gps::oncore::BA_POSITION_STATUS;
use smartclock_gps::oncore::BD_ALMANAC_STATUS;
use smartclock_gps::oncore::BH_CORRECTION_OUTPUT;
use smartclock_gps::oncore::BN_TIME_RAIM;
use smartclock_gps::oncore::CE_PSEUDORANGE_CORRECTION;
use smartclock_gps::oncore::CG_FIX_MODE;
use smartclock_gps::oncore::MAX_COMMAND;
use smartclock_gps::oncore::MessageId;
use smartclock_gps::oncore::data_length;
use smartclock_gps::oncore::encode;
use smartclock_gps::record::Fix;
use smartclock_gps::record::Record;
use smartclock_gps::units::Milliseconds;
use smartclock_ubx::message::NAV_EOE;

const TRACE: &[u8] = include_bytes!("../../testdata/neo-m8t-2026-10-28.ubx");
/// The day it was recorded, as `@@Ba` gives it, and its GPS week.
const RECORDED: Date = Date { month: 10, day: 28, year: 2026 };
const RECORDED_WEEK: GpsWeek = GpsWeek::new(2442);
/// Epochs before every satellite's ephemeris is in, and the almanacs
/// there are by the end, all but a few of the 32.
const WARM_UP: usize = 60;
const ALMANACS: usize = 28;
/// Seconds in a minute, as `@@Ba`'s second wraps.
const SECONDS_PER_MINUTE: u8 = MINUTE.get() as u8;
/// The epochs in the trace, and those before the channels settle.
const EPOCHS: usize = 900;
const SETTLING: usize = 5;
/// Commands' data: once a second (`@@Ba`, `@@Bh`), polled (`@@Bd`),
/// fixing (`@@Cg` 1, VP p. 118), and `@@Bn` as the Z380x start-up sends
/// it: each second, Time RAIM on, a 1 us alarm limit, the pulse on, one
/// pulse a second (pp. 102-106).
const EACH_SECOND: [u8; 1] = [1];
const POLLED: [u8; 1] = [0];
const FIXING: [u8; 1] = [1];
const Z380X_TIME_RAIM: [u8; 15] = [1, 1, 0, 10, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
/// An epoch's length, as the engine's clock reads it.
const EPOCH_MS: u64 = 1000;
/// A frame's ID's place, and its data's, after `@@`; the checksum and
/// CR LF after the data.
const ID: core::ops::Range<usize> = 2..4;
const DATA_START: usize = 4;
const TRAILER: usize = 3;
/// The least epochs of the trace with all six channels available for
/// position, all six corrected, and Time RAIM OK: a satellite selected
/// from the almanac can still be acquiring (mode 2), as on a VP, for a
/// few epochs.
const TRACKED_EPOCHS: usize = 880;
const CORRECTED_EPOCHS: usize = 820;
const TRAIM_OK_EPOCHS: usize = 890;
/// Every satellite's correction: the atmosphere's delay, with multipath
/// low in the sky.
const CORRECTION: Range<Meters> = Meters::new(-60.0)..Meters::new(10.0);
/// A satellite's Doppler less the drift against its almanac's, and the
/// satellites compared at least.
const DOPPLER: Hertz = Hertz::new(5.0);
const COMPARED: usize = 6;
/// The magnetic variation at the trace's position, WMM2025, east.
const VARIATION: Range<Decidegrees> = Decidegrees::new(120)..Decidegrees::new(140);
/// `@@Ba`'s channel status bit: the satellite gives position (VP p. 73).
const CHANNEL_POSITION: u8 = 1 << 7;
/// Page 18 during the trace: GPS less UTC, s, with no change scheduled;
/// the last change at the end of 2016-12-31, day 7 of week 1929,
/// broadcast modulo 256; the fraction's bound, ns.
const LEAP_SECONDS: i8 = 18;
const LAST_LEAP_WEEK: u16 = 1929;
const LAST_LEAP_DAY: u8 = 7;
const WEEK_MODULUS: u16 = 256;
const FRACTION_LIMIT_NS: i32 = 10;
/// The `SmartClock`'s default position, which the trace was moved to
/// (`testdata/README.md`): latitude and longitude.
const POSITION: (MilliArcSeconds, MilliArcSeconds) =
    (MilliArcSeconds::new(134_372_100), MilliArcSeconds::new(-439_192_200));
/// The rates, m/s: the median within 0.01, the 1st and 99th percentiles
/// within 0.3, the atmosphere's changes mm/s, the rest the Doppler's
/// noise.
const MEDIAN: f64 = 0.5;
const LOW: f64 = 0.01;
const HIGH: f64 = 0.99;
const MEDIAN_RATE: MetersPerSecond = MetersPerSecond::new(0.01);
const RATE_SPREAD: MetersPerSecond = MetersPerSecond::new(0.3);
/// At the last epoch: the corrections at least, and their spread about a
/// bias plus an elevation mapping (1 / sin), m.
const LAST_CORRECTIONS: usize = 10;
const RMS: Meters = Meters::new(2.0);

/// What the engine sends: each message's ID and data.
#[derive(Default)]
struct Out {
    sent: Vec<(MessageId, Vec<u8>)>,
}

impl Output for Out {
    fn send(&mut self, frame: &[u8]) {
        let id = MessageId::new([frame[ID.start], frame[ID.start + 1]]);
        self.sent.push((id, frame[DATA_START..frame.len() - TRAILER].to_vec()));
    }
}

impl Out {
    /// The data of the message `id` sent, if it was.
    fn data(&self, id: MessageId) -> Option<&[u8]> {
        self.sent.iter().find(|(sent, _)| *sent == id).map(|(_, d)| d.as_slice())
    }
}

/// A date, as `@@Ba` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Date {
    month: u8,
    day: u8,
    year: u16,
}

/// `@@Ba`'s date and time, and each channel's mode (VP reference pp. 71-72).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Position {
    date: Date,
    second: u8,
    modes: Vec<u8>,
    /// Each channel's satellite, 0 none, and whether it gives position.
    satellites: Vec<(u8, bool)>,
}

impl Position {
    fn decode(d: &[u8], channels: usize) -> Self {
        let mut r = OncoreReader::new(d);
        let date = Date { month: r.u8(), day: r.u8(), year: r.u16() };
        let (_hour, _minute, second) = (r.u8(), r.u8(), r.u8());
        let (_fraction, _latitude, _longitude, _height_gps, _height_msl) =
            (r.u32(), r.i32(), r.i32(), r.i32(), r.i32());
        let (_speed, _heading, _dop) = (r.u16(), r.u16(), r.u16());
        let (_dop_type, _visible, _tracked) = (r.u8(), r.u8(), r.u8());
        let (modes, satellites) = (0..channels)
            .map(|_| {
                let (prn, mode, _signal, status) = (r.u8(), r.u8(), r.u8(), r.u8());
                (mode, (prn, status & CHANNEL_POSITION != 0))
            })
            .unzip();
        Self { date, second, modes, satellites }
    }
}

/// `@@Ce`'s time and each channel's satellite (p. 90).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Corrections {
    /// 0.1 s of the week.
    tenths: u32,
    /// Each channel's PRN, 0 none.
    prns: Vec<u8>,
}

impl Corrections {
    fn decode(d: &[u8], channels: usize) -> Self {
        let mut r = OncoreReader::new(d);
        let tenths = r.u24();
        let prns = (0..channels)
            .map(|_| {
                let (prn, _range, _rate, _iode) = (r.u8(), r.u24(), r.u16(), r.u8());
                prn
            })
            .collect();
        Self { tenths, prns }
    }
}

/// One of `@@Bd`'s almanacs: its valid flag, week, count and the
/// satellites in it, a bit each (pp. 78-79).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Almanac {
    valid: u8,
    week: u8,
    count: u8,
    set: u32,
}

/// `@@Bd`'s almanac in RAM, and the one in EEPROM.
fn almanacs(d: &[u8]) -> [Almanac; 2] {
    let mut r = OncoreReader::new(d);
    core::array::from_fn(|_| {
        let (valid, week, _toa, count, set) = (r.u8(), r.u8(), r.u8(), r.u8(), r.u32());
        Almanac { valid, week, count, set }
    })
}

/// One epoch of the replay, as the engine reported it.
struct Epoch {
    /// `@@Ba`'s data length and its fields, if sent.
    position: Option<(usize, Position)>,
    /// Whether a `@@Bn` of its length was sent.
    traim_sent: bool,
    /// Whether Time RAIM found the solution OK.
    traim_ok: bool,
    /// `@@Ce`'s data length and its fields, if sent.
    corrections: Option<(usize, Corrections)>,
    /// Each satellite's correction, its range, once warmed up.
    ranges: Vec<(Prn, Meters)>,
    /// Each satellite's correction's rate, once warmed up.
    rates: Vec<MetersPerSecond>,
}

/// The whole trace replayed: the emulator at its end, each epoch, and
/// `@@Bd` asked after the last.
struct Replay {
    emulator: Box<Emulator>,
    epochs: Vec<Epoch>,
    almanac_status: Vec<u8>,
}

impl Replay {
    /// The epochs after the first `n`, with their numbers from 1.
    fn after(&self, n: usize) -> impl Iterator<Item = (usize, &Epoch)> {
        (1..).zip(&self.epochs).skip(n)
    }
}

/// The trace replayed once, for every test.
static REPLAY: LazyLock<Replay> = LazyLock::new(replay);

/// Sends `id` with `data` to the emulator at `now`.
fn command(emulator: &mut Emulator, id: MessageId, data: &[u8], now: Milliseconds<u64>, out: &mut Out) {
    let mut frame = [0u8; MAX_COMMAND];
    let n = encode(id, data, &mut frame).expect("a frame fits");
    emulator.command(&frame[..n], now, out, &mut |_| {});
}

/// Runs the trace through the emulator, as `oncore-app` does: a VP
/// fixing, continuous position and Time RAIM, Time RAIM on, as the Z380x
/// start-up asks, and pseudorange corrections each second.
fn replay() -> Replay {
    let mut emulator =
        Box::new(Emulator::new(Identities::VP, Configuration::FACTORY, ReceiverControl::Commanded));
    let (mut gnss, mut module) = (Gnss::new(), Module::new());
    let mut out = Out::default();
    let start = Milliseconds(0);
    command(&mut emulator, CG_FIX_MODE, &FIXING, start, &mut out);
    command(&mut emulator, BA_POSITION_STATUS, &EACH_SECOND, start, &mut out);
    command(&mut emulator, BN_TIME_RAIM, &Z380X_TIME_RAIM, start, &mut out);
    command(&mut emulator, BH_CORRECTION_OUTPUT, &EACH_SECOND, start, &mut out);
    let channels = emulator.receiver().channels();
    let mut epochs = vec![];
    for &b in TRACE {
        if gnss.push(b, &mut *emulator, &mut module) != Some(NAV_EOE) {
            continue;
        }
        let epoch = epochs.len() + 1;
        let now = Milliseconds(epoch as u64 * EPOCH_MS);
        // The alert and channel selection every `SELECT_EPOCHS` epochs, as
        // `oncore-app` runs them, here whole.
        if epoch % SELECT_EPOCHS as usize == 1 {
            emulator.alert();
            while emulator.background() {}
        }
        out.sent.clear();
        emulator.second(now, &mut out, &mut |_| {});
        // The epoch's corrections, here whole, where `oncore-app` steps
        // them a satellite at a time between I/O.
        while emulator.background() {}
        let receiver = emulator.receiver();
        let (mut ranges, mut rates) = (vec![], vec![]);
        if epoch > WARM_UP {
            let set = receiver.differential.latest().expect("corrections");
            for (prn, k) in Prn::all().zip(set.corrections.iter()) {
                let Some(k) = k else { continue };
                ranges.push((prn, k.range));
                rates.push(k.rate);
            }
        }
        epochs.push(Epoch {
            position: out.data(BA_POSITION_STATUS).map(|d| (d.len(), Position::decode(d, channels))),
            traim_sent: out.sent.iter().any(|(id, d)| *id == BN_TIME_RAIM && Some(d.len()) == response(*id)),
            traim_ok: receiver.traim.is_some_and(|r| r.solution == Solution::Ok),
            corrections: out
                .data(CE_PSEUDORANGE_CORRECTION)
                .map(|d| (d.len(), Corrections::decode(d, channels))),
            ranges,
            rates,
        });
    }
    out.sent.clear();
    let after = Milliseconds((epochs.len() as u64 + 1) * EPOCH_MS);
    command(&mut emulator, BD_ALMANAC_STATUS, &POLLED, after, &mut out);
    let almanac_status = out.data(BD_ALMANAC_STATUS).expect("@@Bd answered").to_vec();
    Replay { emulator, epochs, almanac_status }
}

/// The length of `id`'s data from the engine.
fn response(id: MessageId) -> Option<usize> {
    data_length(id, Direction::FromEngine)
}

/// The module's time at the trace's end.
fn gps_time(r: &Replay) -> GpsSeconds {
    r.emulator.receiver().time().expect("the module's time")
}

/// `n` is at least `least`.
fn assert_at_least(n: usize, least: usize, what: &str) {
    assert!(n >= least, "{what}: {n}, under {least}");
}

/// Each epoch's `@@Ba`, sent and its length.
fn positions(r: &Replay) -> impl Iterator<Item = (usize, &Position)> {
    (1..).zip(&r.epochs).map(|(epoch, e)| {
        let (len, p) = e.position.as_ref().unwrap_or_else(|| panic!("epoch {epoch}: no Ba"));
        assert_eq!(Some(*len), response(BA_POSITION_STATUS), "epoch {epoch}");
        (epoch, p)
    })
}

#[test]
fn every_epoch_is_replayed() {
    assert_eq!(REPLAY.epochs.len(), EPOCHS);
}

#[test]
fn position_date_and_second() {
    let mut last_second = None;
    for (epoch, p) in positions(&REPLAY) {
        // The module's date, no week rollover.
        assert_eq!(p.date, RECORDED, "epoch {epoch}");
        // The time advances a second an epoch.
        if let Some(last) = last_second {
            assert_eq!(p.second, (last + 1) % SECONDS_PER_MINUTE, "epoch {epoch}");
        }
        last_second = Some(p.second);
    }
}

#[test]
fn page_18_utc_parameters() {
    // Subframe 4 page 18, broadcast every 12.5 minutes: 18 leap
    // seconds, none scheduled; the last change, 2016-12-31, at the end
    // of week 1929's day 7, the week modulo 256; the fraction a few ns.
    let utc = REPLAY.emulator.receiver().nav.utc.expect("page 18");
    let u = utc.broadcast();
    assert_eq!((u.gps_less_utc.0, u.gps_less_utc_after.0), (LEAP_SECONDS, LEAP_SECONDS));
    assert!(!utc.change_scheduled());
    assert_eq!((u.leap_week, u.leap_day), ((LAST_LEAP_WEEK % WEEK_MODULUS) as u8, LAST_LEAP_DAY));
    let fraction = utc.fraction(gps_time(&REPLAY)).get();
    assert!(fraction.abs() < FRACTION_LIMIT_NS, "GPS - UTC fraction {fraction} ns");
}

#[test]
fn position_channels_available_for_position() {
    // Six channels, each a satellite available for position, nearly
    // always.
    let tracked = positions(&REPLAY)
        .skip(SETTLING)
        .filter(|(_, p)| p.modes.iter().all(|&m| m == MODE_POSITION))
        .count();
    assert_at_least(tracked, TRACKED_EPOCHS, "epochs with all six tracked");
}

#[test]
fn position_fix_at_the_default_position() {
    let fix = REPLAY.emulator.receiver().fix.expect("a fix");
    assert_eq!((fix.latitude, fix.longitude), POSITION);
}

#[test]
fn time_raim_sent_and_ok() {
    for (epoch, e) in (1..).zip(&REPLAY.epochs) {
        assert!(e.traim_sent, "epoch {epoch}: no Bn");
    }
    // A healthy sky: Time RAIM finds the solution within its limit.
    let ok = REPLAY.after(SETTLING).filter(|(_, e)| e.traim_ok).count();
    assert_at_least(ok, TRAIM_OK_EPOCHS, "epochs with Time RAIM OK");
}

#[test]
fn corrections_each_second_on_the_second() {
    let mut corrected = 0;
    for (epoch, e) in (1..).zip(&REPLAY.epochs) {
        let (len, c) = e.corrections.as_ref().unwrap_or_else(|| panic!("epoch {epoch}: no Ce"));
        assert_eq!(Some(*len), response(CE_PSEUDORANGE_CORRECTION), "epoch {epoch}");
        if epoch > WARM_UP {
            // The measurement time, the epoch's, and every channel.
            assert_eq!(c.tenths % TENTHS_PER_SECOND, 0, "epoch {epoch}: {}", c.tenths);
            corrected += usize::from(c.prns.iter().all(|&p| p != 0));
        }
    }
    assert_at_least(corrected, CORRECTED_EPOCHS, "epochs with all six corrected");
}

#[test]
fn corrections_within_the_atmospheric_delay() {
    // Every satellite's corrections, every epoch, once the ephemerides
    // are in.
    for (epoch, e) in REPLAY.after(WARM_UP) {
        for &(prn, range) in &e.ranges {
            assert!(CORRECTION.contains(&range), "epoch {epoch} PRN {prn}: {} m", range.get());
        }
    }
}

#[test]
fn correction_rates_about_zero() {
    let mut rates: Vec<MetersPerSecond> =
        REPLAY.epochs.iter().flat_map(|e| e.rates.iter().copied()).collect();
    rates.sort_by(MetersPerSecond::total_cmp);
    let q = |f: f64| rates[((rates.len() - 1) as f64 * f) as usize];
    assert!(
        q(MEDIAN).abs() < MEDIAN_RATE && q(LOW) > -RATE_SPREAD && q(HIGH) < RATE_SPREAD,
        "{:?} {:?} {:?}",
        q(LOW),
        q(MEDIAN),
        q(HIGH)
    );
}

#[test]
fn corrections_fit_a_bias_and_an_elevation_mapping() {
    // At the last epoch, within a couple of meters of a bias plus an
    // elevation mapping (1 / sin), larger low.
    let receiver = REPLAY.emulator.receiver();
    let all = receiver.differential.latest().unwrap().corrections;
    let mut rows = vec![];
    for (prn, s) in receiver.sky.iter() {
        let Some(k) = all[prn.index()] else { continue };
        println!(
            "PRN {prn:2} el {:3}: {:7.2} m, {:7.3} m/s, IODE {}",
            s.elevation.get(),
            k.range.get(),
            k.rate.get(),
            k.iode
        );
        rows.push((1.0 / s.elevation.sin(), k.range));
    }
    assert_at_least(rows.len(), LAST_CORRECTIONS, "corrections at the last epoch");
    let n = rows.len() as f64;
    let (mx, my) = (rows.iter().map(|r| r.0).sum::<f64>() / n, rows.iter().map(|r| r.1).sum::<Meters>() / n);
    let slope = rows.iter().map(|&(x, y)| (x - mx) * (y - my)).sum::<Meters>()
        / rows.iter().map(|(x, _)| (x - mx) * (x - mx)).sum::<f64>();
    let residuals: Vec<Meters> = rows.iter().map(|&(x, y)| y - my - slope * (x - mx)).collect();
    let rms = root_mean_square(&residuals);
    assert!(slope < Meters::new(0.0) && rms < RMS, "slope {slope:?}, rms {rms:?}");
}

/// The root of the mean of the squares.
fn root_mean_square(v: &[Meters]) -> Meters {
    (v.iter().map(|&r| r * r).sum::<SquareMeters>() / v.len() as f64).sqrt()
}

#[test]
fn almanac_complete_and_reported() {
    // The almanac is complete after a 12.5 minute cycle.
    let almanac_count = REPLAY.emulator.receiver().nav.almanac.iter().flatten().count();
    assert_at_least(almanac_count, ALMANACS, "almanacs");
    // Its status, as `@@Bd` answers: valid, week 2442's low 8 bits less
    // at most one, every almanac counted, the same in EEPROM.
    let [ram, eeprom] = almanacs(&REPLAY.almanac_status);
    assert_eq!(ram.valid, 1);
    let weeks = [RECORDED_WEEK, RECORDED_WEEK.previous()].map(GpsWeek::almanac_week);
    assert!(weeks.contains(&ram.week), "week {}", ram.week);
    assert_eq!(usize::from(ram.count), almanac_count);
    assert_eq!(ram.set.count_ones(), ram.count.into());
    assert_eq!(eeprom, ram);
}

#[test]
fn doppler_matches_the_almanac() {
    // Each tracked satellite's measured Doppler, less the module's clock
    // drift, against the Doppler computed from its almanac.
    let receiver = REPLAY.emulator.receiver();
    let drift = receiver.clock.map(|c| c.drift);
    let mut compared = 0;
    for (prn, s) in receiver.sky.iter() {
        let Some(computed) = almanac_doppler(receiver, gps_time(&REPLAY), prn) else { continue };
        if !s.measured {
            continue;
        }
        let measured = Hertz::from(s.doppler) + drift.unwrap().l1_doppler();
        let diff = measured - Hertz::from(computed);
        println!(
            "PRN {prn:2}: measured less drift {:8.1} Hz, almanac {:6} Hz, diff {:6.1}",
            measured.get(),
            computed.get(),
            diff.get()
        );
        assert!(diff.abs() < DOPPLER, "PRN {prn}: {} Hz", diff.get());
        compared += 1;
    }
    assert_at_least(compared, COMPARED, "satellites compared");
}

/// The record the trace ends on: a 3-D fix, the time, the almanac, and
/// the last `@@Ba`'s channel satellites first, in channel order, used
/// where it gives them position.
#[test]
fn the_record_matches_the_last_position_status() {
    let (_, last) = REPLAY.epochs.iter().rev().find_map(|e| e.position.as_ref()).expect("a @@Ba");
    let mut r = Box::new(Record::EMPTY);
    fill(REPLAY.emulator.receiver(), &REPLAY.emulator.receiver_settings(), &mut r);
    assert_eq!(r.fix, Fix::ThreeD);
    assert!(r.time.is_some() && r.almanac.is_some() && r.dops.is_some());
    let channels: Vec<(u8, bool)> = last.satellites.iter().copied().filter(|&(prn, _)| prn != 0).collect();
    let first: Vec<(u8, bool)> =
        r.satellites.iter().take(channels.len()).map(|s| (s.prn.get(), s.used)).collect();
    assert_eq!(first, channels);
}

#[test]
fn magnetic_variation_from_the_model() {
    let variation = magnetic_variation(REPLAY.emulator.receiver(), gps_time(&REPLAY)).expect("a variation");
    assert!(VARIATION.contains(&variation), "magnetic variation {}", variation.get());
}
