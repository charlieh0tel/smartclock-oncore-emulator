//! Moves a UBX recording of a fixed timing receiver to another antenna
//! position, so it no longer gives the original away:
//!
//! ```text
//! move_trace IN OUT LATITUDE_MAS LONGITUDE_MAS HEIGHT_MSL_CM
//! ```
//!
//! Every message that depends on where the antenna is follows it:
//! NAV-POSECEF and NAV-PVT take the new position (the ellipsoid height
//! keeps the recording's geoid separation); RXM-RAWX's GPS pseudoranges
//! and carrier phases move by each satellite's change of geometric range,
//! and its Dopplers by the change of range rate (`differential::geometry`,
//! the ephemerides from the recording's own subframes, decoded first);
//! NAV-SAT's GPS elevations and azimuths are computed again, its
//! residuals kept; NAV-DOP is computed again from the GPS satellites
//! used.  Other systems' records, and a GPS satellite's without an
//! ephemeris, are dropped from RXM-RAWX and NAV-SAT: they would still
//! locate the original.  RXM-RAWX's geometry is at GPS time, its rcvTow
//! less NAV-CLOCK's bias.  The messages reviewed as not locating the
//! antenna are copied as they are (`UNMOVED`); any other message is an
//! error, as is a destination where a satellite's elevation moves by
//! more than a degree, since its recorded signal and tracking must still
//! fit.  NAV-PVT's satellite count stays the receiver's, every system's.

use std::collections::HashMap;
use std::process::ExitCode;

use oncore_core::differential::geometry;
use oncore_core::differential::gps_time;
use oncore_core::dop::Dops;
use oncore_core::dop::inverse;
use oncore_core::gnss::Clock;
use oncore_core::gnss::GPS;
use oncore_core::gnss::Measurements;
use oncore_core::gnss::Pvt;
use oncore_core::nav::Ephemeris;
use oncore_core::nav::Nav;
use oncore_core::sky::PRNS;
use oncore_core::sky::Prn;
use oncore_core::units::Centimeters;
use oncore_core::units::DEGREES_PER_CIRCLE;
use oncore_core::units::Degrees;
use oncore_core::units::Degrees1e7;
use oncore_core::units::Ecef;
use oncore_core::units::Hertz;
use oncore_core::units::Meters;
use oncore_core::units::MilliArcSeconds;
use oncore_core::units::Millimeters;
use oncore_core::units::Radians;
use oncore_core::units::Seconds;
use oncore_core::units::TimeOfWeek;
use oncore_core::vector::AXES;
use oncore_core::vector::Direction;
use oncore_core::vector::Enu;
use oncore_core::wire::UbxReader;
use oncore_core::wire::UbxWriter;
use smartclock_ubx::frame::Deframer;
use smartclock_ubx::frame::MessageId;
use smartclock_ubx::frame::OVERHEAD;
use smartclock_ubx::frame::encode;
use smartclock_ubx::message::MON_HW;
use smartclock_ubx::message::Message;
use smartclock_ubx::message::NAV_CLOCK;
use smartclock_ubx::message::NAV_DOP;
use smartclock_ubx::message::NAV_EOE;
use smartclock_ubx::message::NAV_POSECEF;
use smartclock_ubx::message::NAV_PVT;
use smartclock_ubx::message::NAV_SAT;
use smartclock_ubx::message::NAV_TIMEGPS;
use smartclock_ubx::message::NAV_TIMELS;
use smartclock_ubx::message::NAV_VELECEF;
use smartclock_ubx::message::RXM_RAWX;
use smartclock_ubx::message::RXM_SFRBX;
use smartclock_ubx::message::TIM_TP;
use smartclock_ubx::nav::NavSat;
use smartclock_ubx::nav::nav_sat_length;
use smartclock_ubx::rxm::RxmRawx;
use smartclock_ubx::rxm::rxm_rawx_length;

/// The messages copied as they are, reviewed as not locating the
/// antenna: NAV-VELECEF (a fixed antenna's zero velocity), NAV-TIMEGPS,
/// NAV-CLOCK, NAV-TIMELS, NAV-EOE, MON-HW, TIM-TP, and RXM-SFRBX (the
/// satellites' own broadcasts).  Any other is an error.
const UNMOVED: [MessageId; 8] =
    [NAV_VELECEF, NAV_TIMEGPS, NAV_CLOCK, NAV_TIMELS, NAV_EOE, MON_HW, TIM_TP, RXM_SFRBX];
/// The most a satellite's elevation may move.
const ELEVATION_CHANGE: Degrees<i32> = Degrees::new(1);

/// The fields passed over to reach the ones moved, bytes, in each
/// message's order (UBX-13003221 R28): NAV-PVT's iTOW to numSV, before lon;
/// NAV-POSECEF's and NAV-DOP's iTOW; NAV-SAT's iTOW and version, before
/// numSvs, and a record's gnssId, svId and cno, before elev; RXM-RAWX's
/// rcvTow, week and leapS, before numMeas.
const PVT_BEFORE_POSITION: usize = 24;
const ITOW_BYTES: usize = 4;
const SAT_BEFORE_NUM_SVS: usize = 5;
const SAT_RECORD_BEFORE_ELEVATION: usize = 3;
const RAWX_BEFORE_NUM_MEAS: usize = 11;
/// NAV-DOP's DOPs, 0.01 each: gDOP, pDOP, tDOP, vDOP, hDOP, nDOP, eDOP.
const DOPS: usize = 7;
const DOP_SCALE: f64 = 100.0;
/// NAV-SAT's and RXM-RAWX's headers and records, bytes.
const SAT_HEADER: usize = nav_sat_length(0);
const SAT_RECORD: usize = nav_sat_length(1) - SAT_HEADER;
const RAWX_HEADER: usize = rxm_rawx_length(0);
const RAWX_RECORD: usize = rxm_rawx_length(1) - RAWX_HEADER;
/// The longest payload a frame can carry.
const LONGEST_PAYLOAD: usize = u16::MAX as usize;
/// A DOP when fewer than four satellites are used.
const DOP_NONE: u16 = 9999;
/// A pivot this small or less is a singular geometry: in `f64`, apart
/// from the firmware's `f32` one, as sharing that would change the DOPs
/// written.
const SINGULAR_PIVOT: f64 = f64::EPSILON;

/// A destination height an `i32` of mm does not hold.
const OUT_OF_RANGE: &str = "a height out of the module's range";
/// NAV-SAT's azimuth is 0 to 359 degrees.
const FULL_CIRCLE: Degrees<f64> = Degrees::new(DEGREES_PER_CIRCLE as f64);

/// One UBX message as recorded: class and ID, and payload.
struct Recorded {
    id: MessageId,
    payload: Vec<u8>,
}

/// Finds frames in a recording and decodes recorded messages: a deframer
/// that holds any payload.
struct Decoder(Box<Deframer<LONGEST_PAYLOAD>>);

impl Decoder {
    fn new() -> Self {
        Self(Box::new(Deframer::new()))
    }

    /// The recording's messages, in order; an error at the first byte that
    /// is not part of a whole message with a good checksum.
    fn messages(&mut self, data: &[u8]) -> Result<Vec<Recorded>, String> {
        self.0.reset();
        let mut found = Vec::new();
        let mut start = 0;
        for (at, &b) in data.iter().enumerate() {
            match self.0.push(b) {
                None => {}
                Some(Err(e)) => return Err(format!("{e} at byte {at}")),
                Some(Ok(f)) => {
                    let end = at + 1;
                    if end - (f.payload().len() + OVERHEAD) != start {
                        return Err(format!("no UBX message at byte {start}"));
                    }
                    found.push(Recorded { id: f.id(), payload: f.payload().to_vec() });
                    start = end;
                }
            }
        }
        if start == data.len() { Ok(found) } else { Err(format!("a short message at byte {start}")) }
    }

    /// A recorded message, decoded; `None` for one the decoders do not
    /// take.
    fn decode(&mut self, m: &Recorded) -> Option<Message<'_>> {
        let framed = frame(m.id, &m.payload).ok()?;
        let (&last, rest) = framed.split_last()?;
        self.0.reset();
        for &b in rest {
            // A frame ends at its last byte, not before.
            let _ = self.0.push(b);
        }
        Message::parse(self.0.push(last)?.ok()?)
    }
}

/// Where the antenna is: ECEF, m, and the east, north and up unit
/// vectors there.
struct Site {
    ecef: Ecef,
    east: Direction,
    north: Direction,
    up: Direction,
}

impl Site {
    /// The site at geodetic `latitude` and `longitude` and ellipsoid
    /// `height`.
    fn geodetic(latitude: Degrees<f64>, longitude: Degrees<f64>, height: Meters) -> Self {
        let (phi, lambda) = (latitude.radians(), longitude.radians());
        Self::at(Ecef::from_geodetic(phi, lambda, height), phi, lambda)
    }

    /// The site at `ecef`, whose geodetic latitude and longitude are `phi`
    /// and `lambda`.
    fn at(ecef: Ecef, phi: Radians, lambda: Radians) -> Self {
        let (sin_phi, cos_phi, sin_lambda, cos_lambda) = (phi.sin(), phi.cos(), lambda.sin(), lambda.cos());
        Self {
            ecef,
            east: Direction::new([-sin_lambda, cos_lambda, 0.0]),
            north: Direction::new([-sin_phi * cos_lambda, -sin_phi * sin_lambda, cos_phi]),
            up: Direction::new([cos_phi * cos_lambda, cos_phi * sin_lambda, sin_phi]),
        }
    }

    /// `los`, an ECEF unit vector, as east, north and up.
    fn local(&self, los: Direction) -> Enu<f64> {
        Enu { east: los.dot(self.east), north: los.dot(self.north), up: los.dot(self.up) }
    }
}

/// A navigation message's GPS time of week, its first field.
fn itow(p: &[u8]) -> TimeOfWeek {
    TimeOfWeek::from_ms(UbxReader::new(p).u32())
}

/// The message `id` with `payload`, framed.
fn frame(id: MessageId, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = vec![0; payload.len() + OVERHEAD];
    encode(id, payload, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Each GPS satellite's last whole ephemeris in the recording's
/// subframes, by PRN less one.
fn ephemerides(messages: &[Recorded], decoder: &mut Decoder) -> Vec<Option<Ephemeris>> {
    let mut nav = Nav::new();
    let mut found = vec![None; PRNS];
    for m in messages.iter().filter(|m| m.id == RXM_SFRBX) {
        let Some(Message::RxmSfrbx(subframe)) = decoder.decode(m) else { continue };
        let (Some(prn), Some(words)) = (Prn::new(subframe.id), subframe.gps_words()) else { continue };
        nav.subframe(prn, &words);
        if let Some(e) = nav.ephemeris[prn.index()].ephemeris() {
            found[prn.index()] = Some(e);
        }
    }
    found
}

/// A GPS record's satellite's ephemeris, if it has one.
fn ephemeris_of(ephemerides: &[Option<Ephemeris>], gnss: u8, sv: u8) -> Option<&Ephemeris> {
    if gnss != GPS {
        return None;
    }
    ephemerides.get(Prn::new(sv)?.index())?.as_ref()
}

/// The new site in NAV-POSECEF's and NAV-PVT's fields: ECEF; longitude
/// and latitude; the heights above the ellipsoid and mean sea level.
struct Placement {
    ecef: [Centimeters; AXES],
    longitude_latitude: [Degrees1e7; 2],
    height: Millimeters,
    height_msl: Millimeters,
}

/// The move: from one site to the other, with the recording's
/// ephemerides.
struct Move {
    from: Site,
    to: Site,
    /// The new site as NAV-POSECEF and NAV-PVT carry it.
    at: Placement,
    ephemerides: Vec<Option<Ephemeris>>,
    /// The recording's NAV-CLOCKs, for RXM-RAWX's GPS time.
    clocks: Vec<Clock>,
    /// Records dropped from RXM-RAWX and NAV-SAT.
    dropped: usize,
}

impl Move {
    /// A message's payload at the new site.
    fn payload(&mut self, m: &Recorded, decoder: &mut Decoder) -> Result<Vec<u8>, String> {
        Ok(match m.id {
            NAV_POSECEF => {
                let mut p = m.payload.clone();
                let [x, y, z] = self.at.ecef;
                UbxWriter::new(&mut p).skip(ITOW_BYTES).i32(x.get()).i32(y.get()).i32(z.get());
                p
            }
            NAV_PVT => {
                let mut p = m.payload.clone();
                let [lon, lat] = self.at.longitude_latitude;
                UbxWriter::new(&mut p)
                    .skip(PVT_BEFORE_POSITION)
                    .i32(lon.get())
                    .i32(lat.get())
                    .i32(self.at.height.get())
                    .i32(self.at.height_msl.get());
                p
            }
            NAV_SAT => match decoder.decode(m) {
                Some(Message::NavSat(sat)) => self.sat(&m.payload, &sat)?,
                _ => return Err("a NAV-SAT that does not decode".into()),
            },
            RXM_RAWX => match decoder.decode(m) {
                Some(Message::RxmRawx(rawx)) => self.rawx(&m.payload, &rawx)?,
                _ => return Err("an RXM-RAWX that does not decode".into()),
            },
            // computed again below, from the moved NAV-SAT
            NAV_DOP => m.payload.clone(),
            id if UNMOVED.contains(&id) => m.payload.clone(),
            id => {
                let MessageId { class, id } = id;
                return Err(format!("message {class:#04x} {id:#04x} not reviewed: might locate the antenna"));
            }
        })
    }
    /// RXM-RAWX at the new site, its geometry at GPS time: rcvTow is the
    /// receiver's, NAV-CLOCK's bias away (`differential::gps_time`).
    /// `p` is its payload, `rawx` the same decoded.
    fn rawx(&mut self, p: &[u8], rawx: &RxmRawx<'_>) -> Result<Vec<u8>, String> {
        let tow = Seconds::new(f64::from_bits(rawx.header.receiver_time_of_week.0));
        let measured = Measurements { tow, ..Measurements::new() };
        let received = self
            .clocks
            .iter()
            .find_map(|c| gps_time(&measured, c))
            .ok_or(format!("no NAV-CLOCK for RXM-RAWX at {} s", measured.tow.get()))?;
        let mut out = p[..RAWX_HEADER].to_vec();
        let mut kept = 0u8;
        for (record, m) in p[RAWX_HEADER..].as_chunks::<RAWX_RECORD>().0.iter().zip(rawx.measurements()) {
            let Some(e) = ephemeris_of(&self.ephemerides, m.system, m.id) else {
                self.dropped += 1;
                continue;
            };
            let (old, new) = (geometry(e, received, self.from.ecef), geometry(e, received, self.to.ecef));
            let range = new.range - old.range;
            // A carrier phase of 0 is none.
            let (pseudorange, phase) = (f64::from_bits(m.pseudorange.0), f64::from_bits(m.carrier_phase.0));
            let phase = if phase != 0.0 { phase + range.l1_cycles() } else { phase };
            let shift = (new.range_rate - old.range_rate).doppler();
            let doppler = (Hertz::<f64>::from(Hertz::new(f32::from_bits(m.doppler.0))) + shift).to_f32();
            let mut record = *record;
            UbxWriter::new(&mut record)
                .f64((Meters::new(pseudorange) + range).get())
                .f64(phase)
                .f32(doppler.get());
            out.extend(record);
            kept += 1;
        }
        UbxWriter::new(&mut out).skip(RAWX_BEFORE_NUM_MEAS).u8(kept);
        Ok(out)
    }

    /// NAV-SAT at the new site.  A satellite's elevation may move by
    /// [`ELEVATION_CHANGE`] at most: its recorded signal and tracking
    /// still have to fit.
    /// `p` is its payload, `sat` the same decoded.
    fn sat(&mut self, p: &[u8], sat: &NavSat<'_>) -> Result<Vec<u8>, String> {
        let received = TimeOfWeek::from_ms(sat.header.time_of_week.0).seconds();
        let mut out = p[..SAT_HEADER].to_vec();
        let mut kept = 0u8;
        for (record, info) in p[SAT_HEADER..].as_chunks::<SAT_RECORD>().0.iter().zip(sat.satellites()) {
            let sv = info.id;
            let Some(e) = ephemeris_of(&self.ephemerides, info.system, sv) else {
                self.dropped += 1;
                continue;
            };
            let recorded = Degrees::new(info.elevation.0);
            let enu = self.to.local(geometry(e, received, self.to.ecef).line_of_sight);
            let elevation = Radians::new(enu.up.asin()).degrees().round_i8();
            let azimuth =
                Radians::new(enu.east.atan2(enu.north)).degrees().rem_euclid(FULL_CIRCLE).round_i16();
            if (elevation.to_i32() - recorded.to_i32()).abs() > ELEVATION_CHANGE {
                return Err(format!(
                    "too far: PRN {sv}'s elevation {} becomes {} degrees at {} s",
                    recorded.get(),
                    elevation.get(),
                    received.get()
                ));
            }
            let mut record = *record;
            UbxWriter::new(&mut record)
                .skip(SAT_RECORD_BEFORE_ELEVATION)
                .i8(elevation.get())
                .i16(azimuth.get());
            out.extend(record);
            kept += 1;
        }
        UbxWriter::new(&mut out).skip(SAT_BEFORE_NUM_SVS).u8(kept);
        Ok(out)
    }
}

/// NAV-DOP's gDOP, pDOP, tDOP, vDOP, hDOP, nDOP and eDOP from the east,
/// north and up lines of sight of the satellites used.
fn dops(used: &[Enu<f64>]) -> [u16; DOPS] {
    let Some(q) = inverse(used.iter().copied(), SINGULAR_PIVOT) else {
        return [DOP_NONE; DOPS];
    };
    let d = Dops::from_inverse(&q);
    [d.geometric(), d.position(), d.time(), d.vertical(), d.horizontal(), d.north, d.east]
        .map(|v| (v.sqrt() * DOP_SCALE).round().min(f64::from(DOP_NONE)) as u16)
}

fn run(args: &[String]) -> Result<String, String> {
    let [input, output, latitude, longitude, height_msl] = args else {
        return Err("usage: move_trace IN OUT LATITUDE_MAS LONGITUDE_MAS HEIGHT_MSL_CM".into());
    };
    let height = |s: &String| s.parse::<i32>().map(Centimeters::new).map_err(|e| format!("{s}: {e}"));
    let angle = |s: &String| s.parse::<i32>().map(MilliArcSeconds::new).map_err(|e| format!("{s}: {e}"));
    let (latitude, longitude, height_msl) = (angle(latitude)?, angle(longitude)?, height(height_msl)?);
    let data = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    if data.is_empty() {
        return Err(format!("{input}: empty"));
    }
    let mut decoder = Decoder::new();
    let messages = decoder.messages(&data)?;
    let (from, separation) = recorded_site(&messages, &mut decoder)?;
    // The new heights, each one the module's i32 holds.
    let height_msl = height_msl.checked_millimeters().ok_or(OUT_OF_RANGE)?;
    let height = height_msl.checked_add(separation).ok_or(OUT_OF_RANGE)?;
    let to = Site::geodetic(latitude.degrees(), longitude.degrees(), height.meters());
    let at = Placement {
        ecef: to.ecef.get().map(|m| Meters::new(m).centimeters().round().truncate()),
        longitude_latitude: [longitude, latitude].map(MilliArcSeconds::degrees_1e7),
        height,
        height_msl,
    };

    let clocks = messages
        .iter()
        .filter(|m| m.id == NAV_CLOCK)
        .filter_map(|m| match decoder.decode(m) {
            Some(Message::NavClock(c)) => Some(Clock::from_message(&c)),
            _ => None,
        })
        .collect();
    let ephemerides = ephemerides(&messages, &mut decoder);
    let mut moving = Move { from, to, at, ephemerides, clocks, dropped: 0 };
    let mut out = Vec::with_capacity(data.len());
    for m in &messages {
        out.extend(frame(m.id, &moving.payload(m, &mut decoder)?)?);
    }
    // NAV-DOP from the same epoch's NAV-SAT, wherever it falls in the
    // epoch: a second pass over the moved messages.
    let moved = with_dops(&out, &mut decoder)?;
    std::fs::write(output, &moved).map_err(|e| format!("{output}: {e}"))?;
    Ok(format!(
        "{} messages, {} bytes; {} RXM-RAWX and NAV-SAT records dropped",
        messages.len(),
        moved.len(),
        moving.dropped
    ))
}

/// The recording's site, from its one NAV-POSECEF, and the geoid
/// separation there, from NAV-PVT.
fn recorded_site(messages: &[Recorded], decoder: &mut Decoder) -> Result<(Site, Millimeters), String> {
    let posecef: Vec<[Centimeters; AXES]> = messages
        .iter()
        .filter(|m| m.id == NAV_POSECEF)
        .map(|m| match decoder.decode(m) {
            Some(Message::NavPosEcef(p)) => Ok([p.x, p.y, p.z].map(|c| Centimeters::new(c.0))),
            _ => Err("a NAV-POSECEF that does not decode"),
        })
        .collect::<Result<_, _>>()?;
    let first = *posecef.first().ok_or("no NAV-POSECEF")?;
    if posecef.iter().any(|p| *p != first) {
        return Err("NAV-POSECEF moves: not a fixed-position recording".into());
    }
    let pvt = messages.iter().find(|m| m.id == NAV_PVT).ok_or("no NAV-PVT")?;
    let Some(Message::NavPvt(pvt)) = decoder.decode(pvt) else {
        return Err("a NAV-PVT that does not decode".into());
    };
    let pvt = Pvt::from_message(&pvt);
    let (latitude, longitude) = (pvt.latitude.degrees(), pvt.longitude.degrees());
    let site = Site::at(Ecef::from_centimeters(first), latitude.radians(), longitude.radians());
    Ok((site, pvt.height - pvt.height_msl))
}

/// The moved recording with each NAV-DOP computed again from the
/// satellites its epoch's NAV-SAT (by iTOW) has in the solution.
fn with_dops(data: &[u8], decoder: &mut Decoder) -> Result<Vec<u8>, String> {
    let messages = decoder.messages(data)?;
    let mut used: HashMap<TimeOfWeek, Vec<Enu<f64>>> = HashMap::new();
    for m in messages.iter().filter(|m| m.id == NAV_SAT) {
        let Some(Message::NavSat(sat)) = decoder.decode(m) else {
            return Err("a NAV-SAT that does not decode".into());
        };
        let lines = sat
            .satellites()
            .filter(|s| s.system == GPS && Prn::new(s.id).is_some() && s.used)
            .map(|s| {
                let (elevation, azimuth) = (Degrees::new(s.elevation.0), Degrees::new(s.azimuth.0));
                Enu {
                    east: elevation.cos() * azimuth.sin(),
                    north: elevation.cos() * azimuth.cos(),
                    up: elevation.sin(),
                }
            })
            .collect();
        used.insert(TimeOfWeek::from_ms(sat.header.time_of_week.0), lines);
    }
    let mut out = Vec::with_capacity(data.len());
    for m in &messages {
        let mut p = m.payload.clone();
        if m.id == NAV_DOP
            && let Some(lines) = used.get(&itow(&p))
        {
            let mut w = UbxWriter::new(&mut p);
            w.skip(ITOW_BYTES);
            for v in dops(lines) {
                w.u16(v);
            }
        }
        out.extend(frame(m.id, &p)?);
    }
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(summary) => {
            eprintln!("{summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("move_trace: {e}");
            ExitCode::FAILURE
        }
    }
}
