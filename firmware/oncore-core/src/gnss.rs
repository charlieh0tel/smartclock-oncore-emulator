//! What the engine takes from the module's UBX messages, framed and
//! decoded by smartclock-gps-ublox (u-blox 8 receiver description
//! UBX-13003221 R28): NAV-TIMEGPS (p. 397) for GPS time and leap seconds,
//! NAV-PVT (p. 375) for the fix and velocity, NAV-DOP (p. 361),
//! NAV-POSECEF (p. 374), NAV-CLOCK (p. 359), NAV-TIMELS (p. 398) for a
//! coming leap second, MON-HW (p. 342) for the antenna, TIM-TP (p. 432)
//! for the 1 PPS quantization error, NAV-EOE (p. 363) for the end of each
//! epoch, ACK-ACK and ACK-NAK (p. 179), RXM-SFRBX (p. 419) for the GPS
//! navigation message (`nav`), and NAV-SAT (p. 381) and RXM-RAWX
//! (p. 409) for the satellites and their pseudoranges and Doppler, which
//! are read a block at a time as they stream in and taken once their
//! checksum holds.

use smartclock_gps_ublox::record::MonHwRecord;
use smartclock_gps_ublox::record::NavPvtRecord;
use smartclock_ubx::frame::Block;
use smartclock_ubx::frame::Event;
use smartclock_ubx::frame::Header;
use smartclock_ubx::frame::MessageId;
use smartclock_ubx::frame::Streamer;
use smartclock_ubx::message::LONGEST_PART;
use smartclock_ubx::message::MESSAGES;
use smartclock_ubx::message::Message;
use smartclock_ubx::message::NAV_SAT;
use smartclock_ubx::message::Part;
use smartclock_ubx::message::PayloadLength;
use smartclock_ubx::message::RXM_RAWX;
use smartclock_ubx::message::SYSTEM_GPS;
use smartclock_ubx::nav::FixType;
use smartclock_ubx::nav::NavClock;
use smartclock_ubx::nav::NavPvt;
use smartclock_ubx::nav::NavTimeGps;
use smartclock_ubx::nav::NavTimeLs;
use smartclock_ubx::rxm::Measurement;
use smartclock_ubx::rxm::RxmSfrbx;

use crate::civil::SECONDS_PER_DAY;
use crate::module::Module;
use crate::receiver::Fix;
use crate::receiver::Receiver;
use crate::sky::PRNS;
use crate::sky::Prn;
use crate::sky::PrnSet;
use crate::sky::Satellite;
use crate::sky::Sky;
use crate::units::Centimeters;
use crate::units::Degrees1e5;
use crate::units::Degrees1e7;
use crate::units::GpsSeconds;
use crate::units::GpsWeek;
use crate::units::Hertz;
use crate::units::Meters;
use crate::units::MilliArcSeconds;
use crate::units::Millimeters;
use crate::units::MillimetersPerSecond;
use crate::units::Nanoseconds;
use crate::units::NanosecondsPerSecond;
use crate::units::Picoseconds;
use crate::units::Seconds;
use crate::units::TimeOfWeek;
use crate::vector::AXES;

/// GPS's gnssId, in NAV-SAT, RXM-RAWX and RXM-SFRBX (UBX-13003221 R28,
/// "Satellite Numbering").
pub const GPS: u8 = SYSTEM_GPS;

/// The longest payload the streamer holds: the longest message of fixed
/// length, NAV-PVT's, or a streamed message's part, if longer.  A GPS
/// subframe, RXM-SFRBX's 48 bytes, is shorter.
const LONGEST: usize = {
    let mut longest = LONGEST_PART;
    let mut index = 0;
    while index < MESSAGES.len() {
        if let (_, PayloadLength::Fixed(length)) = MESSAGES[index]
            && length > longest
        {
            longest = length;
        }
        index += 1;
    }
    longest
};

/// What frames the module's bytes: whole messages up to NAV-PVT's length,
/// NAV-SAT and RXM-RAWX a part at a time.
pub type ModuleStreamer = Streamer<LONGEST>;

/// A fix with a position: 2-D, 3-D, GNSS and dead reckoning, or time
/// only.
const fn has_position(fix_type: FixType) -> bool {
    matches!(fix_type, FixType::TwoD | FixType::ThreeD | FixType::WithDeadReckoning | FixType::TimeOnly)
}

/// A 3-D fix.  A module in a time mode (a survey done or a fixed
/// position, as a timing module behind gpsd runs) fixes time only, from
/// a 3-D position it holds.
pub(crate) const fn three_d(fix_type: FixType) -> bool {
    matches!(fix_type, FixType::ThreeD | FixType::WithDeadReckoning | FixType::TimeOnly)
}

/// GPS time, to the nearest second, once week and time of week are
/// valid.
fn gps_time(t: &NavTimeGps) -> Option<GpsSeconds> {
    (t.time_of_week_valid && t.week_valid).then(|| {
        GpsSeconds::from_week(
            GpsWeek::new(t.week_number.cast_unsigned()),
            TimeOfWeek::from_ms(t.time_of_week.0),
            Nanoseconds::new(t.fraction.0),
        )
    })
}

/// NAV-PVT's fix and velocity, in the engine's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pvt {
    /// The navigation epoch.
    pub(crate) tow: TimeOfWeek,
    /// The fraction of the second, -1e9 to 1e9 ns.
    pub(crate) nano: Nanoseconds,
    pub(crate) fix_type: FixType,
    pub(crate) fix_ok: bool,
    pub(crate) differential: bool,
    /// numSV: satellites used in the fix, not those tracked.
    pub(crate) satellites: u8,
    pub longitude: Degrees1e7,
    pub latitude: Degrees1e7,
    /// Above the ellipsoid, and above mean sea level.
    pub height: Millimeters,
    pub height_msl: Millimeters,
    /// North, east and down velocity.
    pub(crate) velocity: [MillimetersPerSecond; AXES],
    pub(crate) ground_speed: MillimetersPerSecond,
    pub(crate) heading: Degrees1e5,
}

impl Pvt {
    pub fn from_message(p: &NavPvt) -> Self {
        Self {
            tow: TimeOfWeek::from_ms(p.time_of_week.0),
            nano: Nanoseconds::new(p.date.nanoseconds.0),
            fix_type: p.fix_type,
            fix_ok: p.fix_ok,
            differential: p.differential,
            satellites: p.satellites,
            longitude: Degrees1e7::new(p.longitude.0),
            latitude: Degrees1e7::new(p.latitude.0),
            height: Millimeters::new(p.height.0),
            height_msl: Millimeters::new(p.mean_sea_level.0),
            velocity: [p.north, p.east, p.down].map(|v| MillimetersPerSecond::new(v.0)),
            ground_speed: MillimetersPerSecond::new(p.ground_speed.0),
            heading: Degrees1e5::new(p.motion_heading.0),
        }
    }

    /// The fix in the Oncore's units, if the module has a position.  A
    /// time-only fix (position hold) keeps the held position, which the
    /// module reports.
    pub(crate) fn fix(&self) -> Option<Fix> {
        (self.fix_ok && has_position(self.fix_type)).then(|| Fix {
            latitude: MilliArcSeconds::from_degrees_1e7(self.latitude),
            longitude: MilliArcSeconds::from_degrees_1e7(self.longitude),
            height_gps: self.height.centimeters(),
            height_msl: self.height_msl.centimeters(),
        })
    }
}

/// NAV-CLOCK: the receiver clock's bias and drift at a navigation epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// The epoch.
    pub(crate) tow: TimeOfWeek,
    pub(crate) bias: Nanoseconds,
    pub drift: NanosecondsPerSecond,
}

impl Clock {
    pub fn from_message(c: &NavClock) -> Self {
        Self {
            tow: TimeOfWeek::from_ms(c.time_of_week.0),
            bias: Nanoseconds::new(c.bias.0),
            drift: NanosecondsPerSecond::new(c.drift.0),
        }
    }
}

/// One RXM-RAWX epoch's GPS L1 C/A measurements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurements {
    /// The measurement time, receiver time of week.
    pub tow: Seconds,
    /// Pseudorange, and Doppler, Hz, positive approaching, by PRN.
    pub pseudorange: [Meters; PRNS],
    pub doppler: [Hertz; PRNS],
    /// The PRNs with a valid pseudorange.
    pub valid: PrnSet,
}

impl Measurements {
    /// None made.
    #[expect(
        clippy::new_without_default,
        reason = "too large to build and return by value: built in place (AGENTS.md, Stack and RAM)"
    )]
    pub const fn new() -> Self {
        Self {
            tow: Seconds::new(0.0),
            pseudorange: [Meters::new(0.0); PRNS],
            doppler: [Hertz::new(0.0); PRNS],
            valid: PrnSet::EMPTY,
        }
    }

    /// None made, in place: too large to build and copy through the
    /// stack (AGENTS.md, Stack and RAM).
    fn clear(&mut self) {
        self.tow = Seconds::new(0.0);
        self.pseudorange.fill(Meters::new(0.0));
        self.doppler.fill(Hertz::new(0.0));
        self.valid = PrnSet::EMPTY;
    }
}

/// A leap second the module has scheduled (NAV-TIMELS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LeapEvent {
    /// The GPS second it falls on: inserted, the one UTC labels 23:59:60;
    /// removed, the first after the one it removes, 23:59:59.
    pub(crate) at: GpsSeconds,
    /// GPS less UTC before it.
    pub(crate) before: Seconds<i8>,
    /// [`LEAP_INSERTED`] or [`LEAP_REMOVED`].
    pub(crate) change: Seconds<i8>,
}

/// A leap second's change to GPS less UTC: inserted, and removed.
pub(crate) const LEAP_INSERTED: Seconds<i8> = Seconds::new(1);
#[cfg(test)]
pub(crate) const LEAP_REMOVED: Seconds<i8> = Seconds::new(-1);

impl LeapEvent {
    /// The scheduled event, if NAV-TIMELS gives one still to come or
    /// happening now.  It falls at the end of GPS day DN of week WN, UTC
    /// being GPS time less the leap seconds (IS-GPS-200, 20.3.3.5.2.4).
    pub(crate) fn from_message(ls: &NavTimeLs) -> Option<Self> {
        let change = ls.change.signum();
        if !ls.event_valid || ls.until.0 < 0 || change == 0 {
            return None;
        }
        let before = Seconds::new(ls.current.0);
        let end_of_day = GpsSeconds::week_start(GpsWeek::new(ls.event_week))
            + Seconds::new(i64::from(ls.event_day) * SECONDS_PER_DAY)
            + Seconds::from(before);
        Some(Self {
            at: end_of_day + Seconds::new(i64::from(change.min(0))),
            before,
            change: Seconds::new(change),
        })
    }
}

/// ACK-ACK or ACK-NAK: whether the module took a configuration message,
/// and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack {
    pub accepted: bool,
    pub message: MessageId,
}

/// An RXM-RAWX measurement's GPS PRN, pseudorange, if valid, and Doppler.
fn gps_measurement(m: &Measurement) -> Option<(Prn, Option<Meters>, Hertz)> {
    let prn = Prn::new(m.id).filter(|_| m.system == GPS)?;
    let pseudorange = Meters::new(f64::from_bits(m.pseudorange.0));
    Some((prn, m.pseudorange_valid.then_some(pseudorange), Hertz::new(f32::from_bits(m.doppler.0))))
}

/// What the module's messages go to: a receiver, and what keeps its
/// seconds.
pub trait Epochs {
    /// The receiver the messages fill.
    fn receiver(&mut self) -> &mut Receiver;
    /// An epoch's end: its GPS time, if the module has one.
    fn second(&mut self, gps_time: Option<GpsSeconds>);
    /// The module's leap seconds, GPS less UTC.
    fn leap_seconds(&mut self, leap: Seconds<i8>);
}

impl Epochs for Receiver {
    fn receiver(&mut self) -> &mut Receiver {
        self
    }

    fn second(&mut self, gps_time: Option<GpsSeconds>) {
        Receiver::second(self, gps_time);
    }

    fn leap_seconds(&mut self, leap: Seconds<i8>) {
        Receiver::leap_seconds(self, leap);
    }
}

/// NAV-SAT and RXM-RAWX as they stream in, held until their checksum
/// vouches for them, and the last whole RXM-RAWX's Doppler.
#[derive(Debug)]
struct Streamed {
    /// The streamed message being read, once its header is.
    reading: Option<MessageId>,
    /// NAV-SAT being read.
    sky: Sky,
    /// RXM-RAWX Doppler being read, and the PRNs measured.
    doppler: [Hertz<i16>; PRNS],
    measured: PrnSet,
    /// The last whole RXM-RAWX's, for the next NAV-SAT.
    last_doppler: [Hertz<i16>; PRNS],
    last_measured: PrnSet,
    /// RXM-RAWX's time of week and measurements being read.
    rawx_tow: Seconds,
    measurements: Measurements,
}

impl Streamed {
    const fn new() -> Self {
        Self {
            reading: None,
            sky: Sky::new(),
            doppler: [Hertz::new(0); PRNS],
            measured: PrnSet::EMPTY,
            last_doppler: [Hertz::new(0); PRNS],
            last_measured: PrnSet::EMPTY,
            rawx_tow: Seconds::new(0.0),
            measurements: Measurements::new(),
        }
    }

    /// A streamed message's header: what it starts is read if the header
    /// decodes.
    fn header(&mut self, header: &Header<'_>) {
        self.reading = match Part::header(header) {
            Some(Part::NavSatHeader(_)) => {
                self.sky.clear();
                Some(NAV_SAT)
            }
            Some(Part::RxmRawxHeader(h)) => {
                (self.doppler, self.measured) = ([Hertz::new(0); PRNS], PrnSet::EMPTY);
                self.measurements.clear();
                self.rawx_tow = Seconds::new(f64::from_bits(h.receiver_time_of_week.0));
                Some(RXM_RAWX)
            }
            _ => None,
        };
    }

    fn block(&mut self, block: &Block<'_>) {
        if self.reading != Some(block.id) {
            return;
        }
        match Part::block(block) {
            Some(Part::Satellite(info)) => {
                if let Some((prn, s)) = Satellite::from_nav_sat(&info) {
                    self.sky.satellites[prn.index()] = s;
                }
            }
            Some(Part::Measurement(m)) => self.measurement(&m),
            _ => {}
        }
    }

    fn measurement(&mut self, m: &Measurement) {
        let Some((prn, pseudorange, hz)) = gps_measurement(m) else { return };
        let i = prn.index();
        self.doppler[i] = hz.truncate();
        self.measured.insert(prn);
        let m = &mut self.measurements;
        m.doppler[i] = hz;
        if let Some(pseudorange) = pseudorange {
            m.pseudorange[i] = pseudorange;
            m.valid.insert(prn);
        }
    }

    fn put_doppler(sky: &mut Sky, doppler: &[Hertz<i16>; PRNS], measured: PrnSet) {
        for (prn, s) in Prn::all().zip(sky.satellites.iter_mut()) {
            s.measured = measured.contains(prn);
            s.doppler = if s.measured { doppler[prn.index()] } else { Hertz::new(0) };
        }
    }

    /// The streamed message `id` whole, its checksum good: what it said
    /// to `receiver`.  `None` if it was not being read.
    fn end(&mut self, id: MessageId, receiver: &mut Receiver) -> Option<MessageId> {
        if self.reading.take() != Some(id) {
            return None;
        }
        if id == NAV_SAT {
            // field by field, not through a copy on the stack
            receiver.sky.satellites = self.sky.satellites;
            Self::put_doppler(&mut receiver.sky, &self.last_doppler, self.last_measured);
        } else {
            let m = receiver.measurements.get_or_insert_with(Measurements::new);
            *m = self.measurements;
            m.tow = self.rawx_tow;
            (self.last_doppler, self.last_measured) = (self.doppler, self.measured);
            Self::put_doppler(&mut receiver.sky, &self.doppler, self.measured);
        }
        Some(id)
    }
}

/// What whole messages, those not streamed, leave for later.
#[derive(Debug)]
struct Whole {
    /// The epoch's NAV-TIMEGPS, its time of week and GPS time, for
    /// NAV-EOE to apply.
    epoch_time: Option<(TimeOfWeek, GpsSeconds)>,
    /// The last ACK-ACK or ACK-NAK.
    ack: Option<Ack>,
}

/// Reads the module's messages a byte at a time and gives each complete
/// one to its [`Epochs`].
#[derive(Debug)]
pub struct Gnss {
    streamer: ModuleStreamer,
    streamed: Streamed,
    whole: Whole,
}

impl Gnss {
    /// Nothing read.
    #[expect(
        clippy::new_without_default,
        reason = "too large to build and return by value: built in place (AGENTS.md, Stack and RAM)"
    )]
    pub const fn new() -> Self {
        Self {
            streamer: ModuleStreamer::new(),
            streamed: Streamed::new(),
            whole: Whole { epoch_time: None, ack: None },
        }
    }

    /// The last ACK-ACK or ACK-NAK, taken by whoever waits for it.
    pub const fn take_ack(&mut self) -> Option<Ack> {
        self.whole.ack.take()
    }

    /// Forgets any message in progress: the module's bytes were cut off.
    pub fn reset(&mut self) {
        self.streamer.reset();
        self.streamed.reading = None;
    }

    /// Takes one byte; when it ends a message, gives the message to
    /// `target` and the module encoder and returns its class and ID.
    pub fn push(&mut self, byte: u8, target: &mut impl Epochs, module: &mut Module) -> Option<MessageId> {
        let event = match self.streamer.push(byte)? {
            Ok(event) => event,
            Err(_) => {
                self.streamed.reading = None;
                return None;
            }
        };
        match event {
            Event::Frame(frame) => {
                let id = frame.id();
                let message = Message::parse(frame)?;
                self.whole.apply(&message, target, module).then_some(id)
            }
            Event::Header(header) => {
                self.streamed.header(&header);
                None
            }
            Event::Block(block) => {
                self.streamed.block(&block);
                None
            }
            Event::End(id) => self.streamed.end(id, target.receiver()),
        }
    }
}

impl Whole {
    /// RXM-SFRBX: a GPS L1 C/A subframe to the navigation message.  Out
    /// of line, so its words are off the stack before the caller goes
    /// on: inlined, they joined `oncore_app::run`'s frame (`make stack`).
    #[inline(never)]
    fn subframe(subframe: &RxmSfrbx<'_>, receiver: &mut Receiver) {
        if let (Some(prn), Some(words)) = (Prn::new(subframe.id), subframe.gps_words()) {
            receiver.nav.subframe(prn, &words);
        }
    }

    /// A whole message not streamed, to `target` and the module encoder;
    /// whether it is one the engine takes.
    fn apply(&mut self, message: &Message<'_>, target: &mut impl Epochs, module: &mut Module) -> bool {
        // CFG-TMODE2's and CFG-TP5's poll answers: the board polls neither.
        if matches!(
            message,
            Message::NavVelEcef(_)
                | Message::NavSat(_)
                | Message::RxmRawx(_)
                | Message::CfgTmode2(_)
                | Message::CfgTp5(_)
        ) {
            return false;
        }
        match message {
            Message::NavTimeGps(t) => {
                if t.leap_seconds_valid {
                    target.leap_seconds(Seconds::new(t.gps_less_utc.0));
                }
                self.epoch_time = gps_time(t).map(|g| (TimeOfWeek::from_ms(t.time_of_week.0), g));
            }
            Message::NavPvt(p) => {
                let pvt = Pvt::from_message(p);
                let r = target.receiver();
                r.fix = pvt.fix();
                r.pvt = Some(pvt);
                if let Some(mode) = p.receiver_mode() {
                    r.mode = mode;
                }
                if let Some(f) = r.fix {
                    r.separation_learned |= module.separation.replace(f.height_gps - f.height_msl).is_none();
                }
            }
            Message::NavDop(d) => target.receiver().dop = Some(*d),
            Message::NavTimeLs(ls) => target.receiver().leap_event = LeapEvent::from_message(ls),
            Message::NavPosEcef(p) => {
                target.receiver().ecef = Some([p.x, p.y, p.z].map(|c| Centimeters::new(c.0)));
            }
            Message::NavClock(c) => target.receiver().clock = Some(Clock::from_message(c)),
            Message::MonHw(hw) => target.receiver().antenna = hw.antenna(),
            // The epoch's last message: its second, once, with its time only
            // if its NAV-PVT has a fix.  Without one the module reports its
            // RTC's time as valid, after its own restart too
            // (UBX-13003221 R28, "Time Validity"), so the engine counts on.
            Message::NavEoe(eoe) => {
                let tow = TimeOfWeek::from_ms(eoe.time_of_week.0);
                let fixed = target.receiver().pvt.is_some_and(|p| p.tow == tow && p.fix_ok);
                let gps = self.epoch_time.take().filter(|&(t, _)| t == tow && fixed);
                target.second(gps.map(|(_, g)| g));
            }
            Message::RxmSfrbx(s) => Self::subframe(s, target.receiver()),
            // Whether the module took a configuration message, and which.
            Message::AckAck(a) => self.ack = Some(Ack { accepted: true, message: a.message }),
            Message::AckNak(a) => self.ack = Some(Ack { accepted: false, message: a.message }),
            Message::TimTp(tp) => {
                target.receiver().q_err = tp.quantization_error.map(|q| Picoseconds::new(q.0))
            }
            // Not taken: returned above.
            Message::NavVelEcef(_)
            | Message::NavSat(_)
            | Message::RxmRawx(_)
            | Message::CfgTmode2(_)
            | Message::CfgTp5(_) => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use smartclock_gps::engine::identity::VP_SIX;
    use smartclock_gps::record::Antenna;
    use smartclock_gps::record::ReceiverMode;
    use smartclock_ubx::frame::MessageId;
    use smartclock_ubx::frame::OVERHEAD;
    use smartclock_ubx::frame::encode;
    use smartclock_ubx::message::MON_HW;
    use smartclock_ubx::message::NAV_EOE;
    use smartclock_ubx::message::NAV_PVT;
    use smartclock_ubx::message::NAV_SAT;
    use smartclock_ubx::message::NAV_TIMEGPS;
    use smartclock_ubx::message::RXM_RAWX;
    use smartclock_ubx::message::RXM_SFRBX;
    use smartclock_ubx::message::TIM_TP;
    use smartclock_ubx::nav::NavTimeGps;
    use smartclock_ubx::nav::NavTimeLs;
    use smartclock_ubx::units::Milliseconds;

    use super::GPS;
    use super::Gnss;
    use super::GpsSeconds;
    use super::GpsWeek;
    use super::LEAP_INSERTED;
    use super::LEAP_REMOVED;
    use super::LeapEvent;
    use super::Module;
    use super::Picoseconds;
    use super::SECONDS_PER_DAY;
    use super::Seconds;
    use super::gps_time;
    use crate::fixtures::HEIGHT_GPS;
    use crate::fixtures::HEIGHT_GPS_MM;
    use crate::fixtures::HEIGHT_MSL;
    use crate::fixtures::HEIGHT_MSL_MM;
    use crate::fixtures::LATITUDE;
    use crate::fixtures::LATITUDE_1E7;
    use crate::fixtures::LEAP_SECONDS;
    use crate::fixtures::LONGITUDE;
    use crate::fixtures::LONGITUDE_1E7;
    use crate::fixtures::PVT_SATELLITES;
    use crate::fixtures::SEPARATION;
    use crate::receiver::Receiver;
    use crate::sky::LOCKED;
    use crate::units::DecibelHertz;
    use crate::units::Decimeters;
    use crate::units::Degrees;
    use crate::units::Hertz;
    use crate::wire::UbxWriter;
    use smartclock_ubx::message::SYSTEM_GALILEO;
    use smartclock_ubx::message::SYSTEM_GLONASS;

    /// Payload lengths, bytes (UBX-13003221 R28): NAV-TIMEGPS, NAV-PVT,
    /// NAV-EOE, MON-HW and TIM-TP; NAV-SAT's header and blocks,
    /// RXM-RAWX's, and RXM-SFRBX's header.
    const TIMEGPS_LEN: usize = 16;
    const PVT_LEN: usize = 92;
    const EOE_LEN: usize = 4;
    const HW_LEN: usize = 60;
    const TP_LEN: usize = 16;
    const SAT_HEADER: usize = 8;
    const SAT_BLOCK: usize = 12;
    const RAWX_HEADER: usize = 16;
    const RAWX_BLOCK: usize = 32;
    const SFRBX_HEADER: usize = 8;
    /// Fields passed over, bytes, in the receiver description's order:
    /// NAV-PVT's iTOW, its year to second, valid, tAcc and nano; MON-HW's
    /// pinSel to agcCnt; TIM-TP's towMS and towSubMS, and its week;
    /// RXM-RAWX's prMes and cpMes.
    const ITOW_BYTES: usize = 4;
    const PVT_BEFORE_FIX_TYPE: usize = 20;
    const HW_BEFORE_ANTENNA: usize = 20;
    const TP_TIME: usize = 8;
    const TP_WEEK: usize = 2;
    const RAWX_BEFORE_DOPPLER: usize = 16;
    /// NAV-TIMEGPS's valid flags; NAV-PVT's gnssFixOK; its fixType for a
    /// 3-D fix and for time only; MON-HW's aStatus for a short and aPower
    /// off; TIM-TP's flag for qErr not valid.
    const TIMEGPS_ALL_VALID: u8 = 0x07;
    const PVT_FIX_OK: u8 = 1 << 0;
    const FIX_3D: u8 = 3;
    const FIX_TIME_ONLY: u8 = 5;
    const FIX_NONE: u8 = 0;
    const A_STATUS_SHORT: u8 = 3;
    const A_POWER_OFF: u8 = 0;
    const TP_Q_ERR_INVALID: u8 = 1 << 4;

    /// 400 us before the end of 172799.999 s into week 2390: rounds to
    /// 172800 s; and 100 ns before 172800 s, with only the time of week
    /// valid.
    const LATE_TOW_MS: u32 = 172_799_999;
    const LATE_FTOW_NS: i32 = 400_000;
    const ROUND_WEEK: i16 = 2390;
    const ROUNDED_S: i64 = 172_800;
    const ON_TOW_MS: u32 = 172_800_000;
    const EARLY_FTOW_NS: i32 = -100;
    /// A second, ms, and two, an epoch's seconds as UTC's second runs;
    /// another epoch's time, five seconds on.
    const SECOND_MS: u32 = 1000;
    const TWO_SECONDS_MS: u32 = SECOND_MS + SECOND_MS;
    const ANOTHER_EPOCH_MS: u32 = 5000;
    /// The subframe's hex: digits a byte, and their base.
    const HEX_DIGITS: usize = 2;
    const HEX: u32 = 16;

    /// Nothing heard from the module, for the six-channel VP.
    fn receiver() -> Receiver {
        Receiver::new(VP_SIX.model().channels())
    }

    /// NAV-EOE at `tow_ms`.
    fn eoe(tow_ms: u32) -> [u8; EOE_LEN] {
        let mut p = [0; EOE_LEN];
        UbxWriter::new(&mut p).u32(tow_ms);
        p
    }

    fn time_gps(tow_ms: u32, week: i16) -> [u8; TIMEGPS_LEN] {
        let mut p = [0; TIMEGPS_LEN];
        UbxWriter::new(&mut p).u32(tow_ms).i32(0).i16(week).i8(LEAP_SECONDS.get()).u8(TIMEGPS_ALL_VALID);
        p
    }

    /// A message framed.
    fn framed(id: MessageId, p: &[u8]) -> Vec<u8> {
        let mut out = vec![0; p.len() + OVERHEAD];
        encode(id, p, &mut out).unwrap();
        out
    }

    /// Bytes through the reader; returns the last message it reports.
    fn push_all(g: &mut Gnss, e: &mut Receiver, m: &mut Module, bytes: &[u8]) -> Option<MessageId> {
        bytes.iter().fold(None, |last, &b| g.push(b, e, m).or(last))
    }

    /// Frames a message and feeds it through the reader; returns what it
    /// reports.
    fn feed(g: &mut Gnss, e: &mut Receiver, m: &mut Module, id: MessageId, p: &[u8]) -> Option<MessageId> {
        push_all(g, e, m, &framed(id, p))
    }

    /// NAV-TIMEGPS of a time of week and fraction, in [`ROUND_WEEK`],
    /// with or without its week valid.
    fn decoded_time_gps(tow_ms: u32, fraction_ns: i32, week_valid: bool) -> NavTimeGps {
        NavTimeGps {
            time_of_week: Milliseconds(tow_ms),
            fraction: smartclock_ubx::units::Nanoseconds(fraction_ns),
            week_number: ROUND_WEEK,
            gps_less_utc: smartclock_ubx::units::Seconds(LEAP_SECONDS.get()),
            time_of_week_valid: true,
            week_valid,
            leap_seconds_valid: true,
            accuracy: smartclock_ubx::units::Nanoseconds(0),
        }
    }

    #[test]
    fn gps_time_rounds_to_the_second() {
        // 172799.999400 s into week 2390 rounds to 172800.
        assert_eq!(
            gps_time(&decoded_time_gps(LATE_TOW_MS, LATE_FTOW_NS, true)),
            Some(GpsSeconds::week_start(GpsWeek::new(ROUND_WEEK as u16)) + Seconds::new(ROUNDED_S))
        );
        assert_eq!(gps_time(&decoded_time_gps(ON_TOW_MS, EARLY_FTOW_NS, false)), None);
    }

    /// NAV-PVT at time of week `tow_ms` of `fix_type`, with `flags`.
    fn pvt_of(tow_ms: u32, fix_type: u8, flags: u8) -> [u8; PVT_LEN] {
        let mut p = [0u8; PVT_LEN];
        UbxWriter::new(&mut p)
            .u32(tow_ms)
            // year to second, valid, tAcc, nano
            .zeros(PVT_BEFORE_FIX_TYPE - ITOW_BYTES)
            .u8(fix_type)
            .u8(flags)
            // flags2
            .zeros(1)
            .u8(PVT_SATELLITES)
            .i32(LONGITUDE_1E7.get())
            .i32(LATITUDE_1E7.get())
            .i32(HEIGHT_GPS_MM.get())
            .i32(HEIGHT_MSL_MM.get());
        p
    }

    /// NAV-PVT at time of week `tow_ms`: a time-only fix if `fix_ok`.
    fn pvt(tow_ms: u32, fix_ok: bool) -> [u8; PVT_LEN] {
        pvt_of(tow_ms, FIX_TIME_ONLY, if fix_ok { PVT_FIX_OK } else { 0 })
    }

    /// 2026-10-27 00:00:30 GPS, week 2442, 172830 s, valid from the
    /// module.
    const TIME: (u32, i16) = (172_830_000, 2442);

    /// The GPS time `tow_ms` into [`TIME`]'s week.
    fn time_in_week(tow_ms: u32) -> GpsSeconds {
        GpsSeconds::week_start(GpsWeek::new(TIME.1 as u16)) + Seconds::new(i64::from(tow_ms / SECOND_MS))
    }

    /// One epoch at `tow` ms: its messages in `order`'s order, `P` NAV-PVT
    /// with a fix, `p` without one, `t` NAV-TIMEGPS, valid, and `e`
    /// NAV-EOE.
    fn epoch(g: &mut Gnss, e: &mut Receiver, m: &mut Module, tow: u32, order: &str) {
        for c in order.chars() {
            match c {
                'P' => feed(g, e, m, NAV_PVT, &pvt(tow, true)),
                'p' => feed(g, e, m, NAV_PVT, &pvt(tow, false)),
                't' => feed(g, e, m, NAV_TIMEGPS, &time_gps(tow, TIME.1)),
                'e' => feed(g, e, m, NAV_EOE, &eoe(tow)),
                _ => unreachable!(),
            };
        }
    }

    #[test]
    fn receiver_takes_time_leap_seconds_and_fix() {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        epoch(&mut g, &mut e, &mut m, TIME.0, "Pte");
        assert_eq!((e.time(), e.leap_seconds), (Some(time_in_week(TIME.0)), Some(LEAP_SECONDS)));
        let f = e.fix.unwrap();
        assert_eq!((f.latitude, f.longitude), (LATITUDE, LONGITUDE));
        assert_eq!((f.height_gps, f.height_msl), (HEIGHT_GPS, HEIGHT_MSL));
        assert_eq!(e.pvt.unwrap().satellites, PVT_SATELLITES);
        assert_eq!(m.separation, Some(SEPARATION));
        assert!(e.separation_learned);
        e.separation_learned = false;
        epoch(&mut g, &mut e, &mut m, TIME.0 + SECOND_MS, "Pte");
        assert!(!e.separation_learned);
    }

    #[test]
    fn time_without_a_fix_is_not_taken() {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        // The RTC's time, valid, with no fix: not taken.
        epoch(&mut g, &mut e, &mut m, TIME.0, "pte");
        assert_eq!(e.time(), None);
        // A fix: taken.
        epoch(&mut g, &mut e, &mut m, TIME.0 + SECOND_MS, "Pte");
        assert_eq!(e.time(), Some(time_in_week(TIME.0 + SECOND_MS)));
    }

    /// The epoch's time is applied at its end, whatever order its
    /// messages come in, and only its own.
    #[test]
    fn time_is_taken_at_the_epochs_end() {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        epoch(&mut g, &mut e, &mut m, TIME.0, "tP");
        assert_eq!(e.time(), None);
        epoch(&mut g, &mut e, &mut m, TIME.0, "e");
        assert_eq!(e.time(), Some(time_in_week(TIME.0)));
        // Another epoch's NAV-TIMEGPS: no time this second.
        epoch(&mut g, &mut e, &mut m, TIME.0 + ANOTHER_EPOCH_MS, "tP");
        epoch(&mut g, &mut e, &mut m, TIME.0 + SECOND_MS, "e");
        assert_eq!(e.time(), None);
        // An epoch with no NAV-TIMEGPS at all: none either.
        epoch(&mut g, &mut e, &mut m, TIME.0 + TWO_SECONDS_MS, "Pe");
        assert_eq!(e.time(), None);
    }

    /// A module restarted on its own reports its RTC's time as valid
    /// before its fix: a minute off, say.  It is not taken.
    #[test]
    fn without_a_fix_the_time_is_not_taken() {
        const RTC_ERROR_MS: u32 = 60_000;
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        epoch(&mut g, &mut e, &mut m, TIME.0, "Pte");
        assert_eq!(e.time(), Some(time_in_week(TIME.0)));
        epoch(&mut g, &mut e, &mut m, TIME.0 + RTC_ERROR_MS, "pte");
        assert_eq!(e.time(), None);
    }

    #[test]
    fn no_fix_without_fix_ok() {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        feed(&mut g, &mut e, &mut m, NAV_PVT, &pvt_of(TIME.0, FIX_3D, 0)).unwrap();
        assert_eq!(e.fix, None);
    }

    #[test]
    fn the_mode_is_the_last_fixed_epoch_s_until_the_module_is_lost() {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        assert_eq!(e.mode, ReceiverMode::Unknown);
        feed(&mut g, &mut e, &mut m, NAV_PVT, &pvt_of(TIME.0, FIX_TIME_ONLY, PVT_FIX_OK)).unwrap();
        assert_eq!(e.mode, ReceiverMode::Holding);
        feed(&mut g, &mut e, &mut m, NAV_PVT, &pvt_of(TIME.0, FIX_NONE, 0)).unwrap();
        assert_eq!(e.mode, ReceiverMode::Holding);
        feed(&mut g, &mut e, &mut m, NAV_PVT, &pvt_of(TIME.0, FIX_3D, PVT_FIX_OK)).unwrap();
        assert_eq!(e.mode, ReceiverMode::Navigating);
        e.lost();
        assert_eq!(e.mode, ReceiverMode::Unknown);
    }

    /// A GPS subframe from the trace (`testdata/`), PRN 17.
    const SUBFRAME: &str =
        "001100000a0a02003c8ac1222cea45156e76c509a558398c4bfd17b9bdc1cc042d13719e6ce8a283a24a7d03049b4290";

    #[test]
    fn a_short_subframe_is_not_read_past_its_end() {
        let sub: Vec<u8> = (0..SUBFRAME.len())
            .step_by(HEX_DIGITS)
            .map(|k| u8::from_str_radix(&SUBFRAME[k..k + HEX_DIGITS], HEX).unwrap())
            .collect();
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        assert_eq!(feed(&mut g, &mut e, &mut m, RXM_SFRBX, &sub), Some(RXM_SFRBX));
        assert_ne!(e.nav, receiver().nav);
        // Its header alone, claiming ten words, after the whole one.
        let mut e = receiver();
        assert_eq!(feed(&mut g, &mut e, &mut m, RXM_SFRBX, &sub[..SFRBX_HEADER]), None);
        assert_eq!(e.nav, receiver().nav);
    }

    #[test]
    fn antenna_status() {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        let mut hw = [0u8; HW_LEN];
        // pinSel to agcCnt, then aStatus and aPower
        UbxWriter::new(&mut hw).zeros(HW_BEFORE_ANTENNA).u8(A_STATUS_SHORT).u8(A_POWER_OFF);
        feed(&mut g, &mut e, &mut m, MON_HW, &hw).unwrap();
        assert_eq!(e.antenna, Antenna::Shorted);
    }

    #[test]
    fn quantization_error_while_valid() {
        const Q_ERR_PS: i32 = -4321;
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        let tp = |flags| {
            let mut tp = [0u8; TP_LEN];
            // towMS, towSubMS, qErr, week, flags
            UbxWriter::new(&mut tp).zeros(TP_TIME).i32(Q_ERR_PS).zeros(TP_WEEK).u8(flags);
            tp
        };
        feed(&mut g, &mut e, &mut m, TIM_TP, &tp(0)).unwrap();
        assert_eq!(e.q_err, Some(Picoseconds::new(Q_ERR_PS)));
        feed(&mut g, &mut e, &mut m, TIM_TP, &tp(TP_Q_ERR_INVALID)).unwrap();
        assert_eq!(e.q_err, None);
    }

    /// The leap second inserted at the end of 2016-12-31: GPS week 1929,
    /// day 7 (Saturday), 17 leap seconds before it.
    const LEAP_2016_WEEK: u16 = 1929;
    const LEAP_2016_DAY: u16 = 7;
    const LEAP_2016_BEFORE: Seconds<i8> = Seconds::new(17);

    /// NAV-TIMELS for that date, with `change`, `seconds_to_event` and
    /// whether the event is valid; GPS its source.
    fn time_ls(change: Seconds<i8>, seconds_to_event: Seconds<i32>, event_valid: bool) -> NavTimeLs {
        /// srcOfCurrLs and srcOfLsChange: GPS.
        const SOURCE_GPS: u8 = 2;
        NavTimeLs {
            time_of_week: Milliseconds(0),
            count_source: SOURCE_GPS,
            current: smartclock_ubx::units::Seconds(LEAP_2016_BEFORE.get()),
            change_source: SOURCE_GPS,
            change: change.get(),
            until: smartclock_ubx::units::Seconds(seconds_to_event.get()),
            event_week: LEAP_2016_WEEK,
            event_day: LEAP_2016_DAY,
            count_valid: true,
            event_valid,
        }
    }

    #[test]
    fn leap_second_coming() {
        /// None scheduled; a day to the event, none, and a second past it.
        const NO_CHANGE: Seconds<i8> = Seconds::new(0);
        const A_DAY: Seconds<i32> = Seconds::new(SECONDS_PER_DAY as i32);
        const NOW: Seconds<i32> = Seconds::new(0);
        const PAST: Seconds<i32> = Seconds::new(-1);
        let end_of_day = GpsSeconds::week_start(GpsWeek::new(LEAP_2016_WEEK))
            + Seconds::new(i64::from(LEAP_2016_DAY) * SECONDS_PER_DAY)
            + Seconds::from(LEAP_2016_BEFORE);
        let event = |change, to_event, valid| LeapEvent::from_message(&time_ls(change, to_event, valid));
        let inserted = event(LEAP_INSERTED, A_DAY, true).unwrap();
        assert_eq!(inserted, LeapEvent { at: end_of_day, before: LEAP_2016_BEFORE, change: LEAP_INSERTED });
        assert_eq!(event(LEAP_REMOVED, A_DAY, true).unwrap().at, end_of_day + Seconds::from(LEAP_REMOVED));
        assert_eq!(event(LEAP_INSERTED, NOW, true), Some(inserted), "happening now");
        assert_eq!(event(LEAP_INSERTED, PAST, true), None, "past");
        assert_eq!(event(NO_CHANGE, A_DAY, true), None, "none scheduled");
        assert_eq!(event(LEAP_INSERTED, A_DAY, false), None, "not valid");
    }

    /// A NAV-SAT block's fields.
    #[derive(Debug, Clone, Copy)]
    struct SatBlock {
        gnss: u8,
        sv: u8,
        cno: u8,
        elevation: i8,
        azimuth: i16,
        residual: i16,
        flags: u32,
    }

    impl SatBlock {
        /// A GPS satellite, 40 dB-Hz at 30 degrees, azimuth 90, no
        /// residual, code and carrier locked.
        const GPS: Self =
            Self { gnss: GPS, sv: 1, cno: 40, elevation: 30, azimuth: 90, residual: 0, flags: LOCKED as u32 };

        fn encode(self) -> [u8; SAT_BLOCK] {
            let mut r = [0; SAT_BLOCK];
            UbxWriter::new(&mut r)
                .u8(self.gnss)
                .u8(self.sv)
                .u8(self.cno)
                .i8(self.elevation)
                .i16(self.azimuth)
                .i16(self.residual)
                .u32(self.flags);
            r
        }
    }

    /// The mode NAV-SAT's best quality gives.
    const POSITION_MODE: u8 = 8;

    fn raw(gnss: u8, sv: u8, doppler: f32) -> [u8; RAWX_BLOCK] {
        let mut r = [0; RAWX_BLOCK];
        // prMes, cpMes, doMes, gnssId, svId; the rest, trkStat included, clear
        UbxWriter::new(&mut r).zeros(RAWX_BEFORE_DOPPLER).f32(doppler).u8(gnss).u8(sv);
        r
    }

    /// NAV-SAT's header: iTOW, version, numSvs and reserved1.
    fn sat_header(satellites: u8) -> Vec<u8> {
        const VERSION: u8 = 1;
        let mut h = vec![0; SAT_HEADER];
        UbxWriter::new(&mut h).u32(0).u8(VERSION).u8(satellites);
        h
    }

    /// RXM-RAWX's header: rcvTow, week, leapS, numMeas and the rest.
    fn rawx_header(measurements: u8) -> Vec<u8> {
        /// numMeas's place.
        const BEFORE_COUNT: usize = 11;
        let mut h = vec![0; RAWX_HEADER];
        UbxWriter::new(&mut h).zeros(BEFORE_COUNT).u8(measurements);
        h
    }

    /// A satellite in use as a module reported one, PRN 5; and PRN 32, in
    /// the almanac only; their Doppler, Hz, as RXM-RAWX gave, and as `@@Bb`
    /// takes it; another system's, left out.
    const USED_PRN: u8 = 5;
    const ALMANAC_PRN: u8 = 32;
    const DOPPLER_HZ: f32 = -1234.6;
    const DOPPLER_WHOLE_HZ: i16 = -1234;
    const OTHER_HZ: f32 = 99.0;
    /// As a module sends them: quality 7, used, every orbit source,
    /// ephemeris and almanac; the almanac alone.
    const IN_USE: u32 = 0x1f0f;
    const ALMANAC_ONLY: u32 = 0x1000;
    /// The used satellite's block, and the almanac's satellite's place.
    const USED: SatBlock = SatBlock {
        sv: USED_PRN,
        cno: 44,
        elevation: 61,
        azimuth: 123,
        residual: -12,
        flags: IN_USE,
        ..SatBlock::GPS
    };
    const ALMANAC_ELEVATION: i8 = 8;
    const ALMANAC_AZIMUTH: i16 = 300;
    /// NAV-SAT's blocks: the used satellite, GLONASS's, left out, and the
    /// almanac's.
    const SAT_BLOCKS: usize = 3;

    /// An engine fed a NAV-SAT of [`USED`], a GLONASS satellite and PRN
    /// [`ALMANAC_PRN`] from the almanac.
    fn fed_a_sky() -> (Gnss, Receiver, Module) {
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        let mut p = sat_header(SAT_BLOCKS as u8);
        p.extend(USED.encode());
        p.extend(SatBlock { gnss: SYSTEM_GLONASS, ..USED }.encode());
        p.extend(
            SatBlock {
                sv: ALMANAC_PRN,
                cno: 0,
                elevation: ALMANAC_ELEVATION,
                azimuth: ALMANAC_AZIMUTH,
                flags: ALMANAC_ONLY,
                ..SatBlock::GPS
            }
            .encode(),
        );
        assert_eq!(feed(&mut g, &mut e, &mut m, NAV_SAT, &p), Some(NAV_SAT));
        (g, e, m)
    }

    #[test]
    fn nav_sat_gives_the_gps_satellites() {
        let (_, e, _) = fed_a_sky();
        let s5 = e.sky.get(USED_PRN).unwrap();
        assert_eq!(
            (s5.cno, s5.elevation, s5.azimuth, s5.residual),
            (
                DecibelHertz::new(USED.cno),
                Degrees::new(USED.elevation),
                Degrees::new(USED.azimuth),
                Decimeters::new(USED.residual)
            )
        );
        assert_eq!(
            (s5.quality, s5.used(), s5.ephemeris, s5.mode()),
            (LOCKED, true, true, Some(POSITION_MODE))
        );
        assert!(e.sky.get(ALMANAC_PRN).unwrap().located());
        // The GPS ones.
        assert_eq!(e.sky.iter().count(), SAT_BLOCKS - 1);
    }

    #[test]
    fn rxm_rawx_gives_the_gps_doppler() {
        /// RAWX's measurements: GPS's and Galileo's.
        const MEASUREMENTS: u8 = 2;
        let (mut g, mut e, mut m) = fed_a_sky();
        let mut p = rawx_header(MEASUREMENTS);
        p.extend(raw(GPS, USED_PRN, DOPPLER_HZ));
        p.extend(raw(SYSTEM_GALILEO, USED_PRN, OTHER_HZ));
        assert_eq!(feed(&mut g, &mut e, &mut m, RXM_RAWX, &p), Some(RXM_RAWX));
        let s5 = e.sky.get(USED_PRN).unwrap();
        assert_eq!((s5.doppler, s5.measured), (Hertz::new(DOPPLER_WHOLE_HZ), true));
        assert!(!e.sky.get(ALMANAC_PRN).unwrap().measured);
    }

    #[test]
    fn a_bad_checksum_keeps_the_old_sky() {
        /// The satellite in the sky, and one in a NAV-SAT that fails.
        const KEPT: u8 = 7;
        const LOST: u8 = 9;
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        let mut p = sat_header(1);
        p.extend(SatBlock { sv: KEPT, ..SatBlock::GPS }.encode());
        feed(&mut g, &mut e, &mut m, NAV_SAT, &p).unwrap();
        let mut q = sat_header(1);
        q.extend(SatBlock { sv: LOST, ..SatBlock::GPS }.encode());
        let mut out = framed(NAV_SAT, &q);
        *out.last_mut().unwrap() ^= 1;
        assert_eq!(push_all(&mut g, &mut e, &mut m, &out), None);
        assert!(e.sky.get(KEPT).is_some());
        assert!(e.sky.get(LOST).is_none());
    }

    /// A NAV-SAT cut off by a reset is dropped, and the next read whole.
    #[test]
    fn a_reset_drops_the_message_cut_off() {
        const KEPT: u8 = 7;
        const LOST: u8 = 9;
        let (mut g, mut e, mut m) = (Gnss::new(), receiver(), Module::new());
        let mut q = sat_header(1);
        q.extend(SatBlock { sv: LOST, ..SatBlock::GPS }.encode());
        let cut = framed(NAV_SAT, &q);
        assert_eq!(push_all(&mut g, &mut e, &mut m, &cut[..cut.len() - 1]), None);
        g.reset();
        let mut p = sat_header(1);
        p.extend(SatBlock { sv: KEPT, ..SatBlock::GPS }.encode());
        assert_eq!(feed(&mut g, &mut e, &mut m, NAV_SAT, &p), Some(NAV_SAT));
        assert!(e.sky.get(KEPT).is_some());
        assert!(e.sky.get(LOST).is_none());
    }
}
