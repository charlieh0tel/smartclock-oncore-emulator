//! The GPS L1 C/A navigation message, from the module's RXM-SFRBX
//! subframes (IS-GPS-200N, `third_party/gps.gov/`), decoded by
//! smartclock-gps-ublox's `subframe`: each satellite's alert and
//! anti-spoof flags (HOW, 20.3.3.2), URA index and health (subframe 1,
//! 20.3.3.3.1.3-4), the ephemeris and clock (subframes 1 to 3,
//! 20.3.3.3-4) for pseudorange corrections, and the almanac (subframes 4
//! and 5, 20.3.3.5, Table 20-VI) for the Doppler of satellites not
//! tracked, with satellite position and velocity from either (Table
//! 20-IV), and the UTC parameters (subframe 4 page 18, 20.3.3.5.1.6) for
//! the offset from GPS time to UTC and a coming leap second.

use libm::atan2;

use libm::sqrt;

use smartclock_ubx::subframe;
use smartclock_ubx::subframe::Body;
use smartclock_ubx::subframe::IonosphereUtc;
use smartclock_ubx::subframe::Scaled;
use smartclock_ubx::subframe::Subframe;
use smartclock_ubx::subframe::Subframe1;
use smartclock_ubx::subframe::Subframe2;
use smartclock_ubx::subframe::Subframe3;
use smartclock_ubx::subframe::Words;
use smartclock_ubx::subframe::unit;

use crate::civil::SECONDS_PER_HOUR;
use crate::sky::PRNS;
use crate::sky::Prn;
use crate::units::CubicMetersPerSecondSquared;
use crate::units::Ecef;
use crate::units::EcefVelocity;
use crate::units::GpsSeconds;
use crate::units::GpsWeek;
use crate::units::HALVES;
use crate::units::Hertz;
use crate::units::Meters;
use crate::units::NS_PER_SECOND;
use crate::units::Nanoseconds;
use crate::units::Radians;
use crate::units::RadiansPerSecond;
use crate::units::RootMeters;
use crate::units::SECONDS_PER_WEEK;
use crate::units::Seconds;
use crate::units::SecondsPerRootMeter;
use crate::units::SecondsPerSecond;
use crate::units::SecondsPerSecondSquared;
use crate::units::nearest_weeks;

/// WGS 84 values (Table 20-IV): the earth's gravitational constant, m^3/s^2,
/// and rotation rate, rad/s.
const MU: CubicMetersPerSecondSquared = CubicMetersPerSecondSquared::new(3.986005e14);
pub(crate) const OMEGA_E: RadiansPerSecond = RadiansPerSecond::new(7.292_115_146_7e-5);
/// The pi IS-GPS-200 defines for these equations (20.3.3.4.3), not the
/// closest f64.
#[expect(clippy::approx_constant, reason = "IS-GPS-200's pi for these equations, not the closest f64")]
const PI: f64 = 3.141_592_653_589_8;
/// The almanac's inclination less this, semicircles, is broadcast
/// (20.3.3.5.2.1).
const ALMANAC_I_REFERENCE: f64 = 0.30;

/// 2^`n`, exactly.
const fn pow2(n: i32) -> f64 {
    if n >= 0 { (1u64 << n) as f64 } else { 1.0 / (1u64 << -n) as f64 }
}

/// A broadcast value's number: its integer times two to its scale.
fn number<T: Copy + Into<f64>, U, const SCALE: i8>(value: Scaled<T, U, SCALE>) -> f64 {
    value.raw.into() * pow2(i32::from(SCALE))
}

fn seconds<T: Copy + Into<f64>, const SCALE: i8>(value: Scaled<T, unit::Second, SCALE>) -> Seconds {
    Seconds::new(number(value))
}

fn meters<T: Copy + Into<f64>, const SCALE: i8>(value: Scaled<T, unit::Meter, SCALE>) -> Meters {
    Meters::new(number(value))
}

fn root_meters<T: Copy + Into<f64>, const SCALE: i8>(value: Scaled<T, unit::RootMeter, SCALE>) -> RootMeters {
    RootMeters::new(number(value))
}

fn radians<T: Copy + Into<f64>, const SCALE: i8>(value: Scaled<T, unit::Radian, SCALE>) -> Radians {
    Radians::new(number(value))
}

fn semicircles<T: Copy + Into<f64>, const SCALE: i8>(value: Scaled<T, unit::SemiCircle, SCALE>) -> Radians {
    Radians::new(number(value) * PI)
}

fn semicircles_per_second<T: Copy + Into<f64>, const SCALE: i8>(
    value: Scaled<T, unit::SemiCirclePerSecond, SCALE>,
) -> RadiansPerSecond {
    RadiansPerSecond::new(number(value) * PI)
}

/// Kepler's equation's iterations, more than the minimum three.
const KEPLER_ITERATIONS: usize = 8;
/// The harmonic the orbit's corrections are in: twice the argument of
/// latitude (Table 20-IV).
const SECOND_HARMONIC: f64 = 2.0;

/// The UTC parameters, the last subframe 4 page 18 (Table 20-IX).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc(IonosphereUtc);

impl Utc {
    /// As broadcast.
    pub const fn broadcast(&self) -> &IonosphereUtc {
        &self.0
    }

    /// The fractional part of GPS time less UTC at `t`, to the nearest ns:
    /// A0 + A1 (tE - tot + 604800 (WN - `WNt`)) (20.3.3.5.2.4), `WNt` taken as
    /// the week modulo 256 nearest `t`'s.
    pub fn fraction(&self, t: GpsSeconds) -> Nanoseconds {
        let u = &self.0;
        let week = GpsWeek::containing(t).map_or(0, GpsWeek::get);
        let weeks = nearest_weeks(i64::from(u.reference_week) - i64::from(week));
        let since = (t.seconds_of_week().get() - weeks * SECONDS_PER_WEEK) as f64 - number(u.reference_time);
        let seconds = number(u.a0) + number(u.a1) * since;
        Nanoseconds::new(libm::round(seconds * NS_PER_SECOND as f64) as i32)
    }

    /// Whether a leap second is scheduled: the leap seconds after it
    /// differ from those now.
    pub fn change_scheduled(&self) -> bool {
        self.0.gps_less_utc_after != self.0.gps_less_utc
    }
}

/// One satellite's almanac, a page of subframe 4 or 5 (Table 20-VI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Almanac(subframe::Almanac);

/// A position and velocity, ECEF, m and m/s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct State {
    pub(crate) position: Ecef,
    pub(crate) velocity: EcefVelocity,
}

impl Almanac {
    /// An almanac of zeros but its reference time, toa in 2^12 s.
    #[cfg(test)]
    pub(crate) const fn zero_at(toa: u8) -> Self {
        Self(subframe::Almanac {
            satellite: 0,
            eccentricity: Scaled::new(0),
            time_of_almanac: Scaled::new(toa),
            inclination_offset: Scaled::new(0),
            right_ascension_rate: Scaled::new(0),
            health: 0,
            root_semi_major_axis: Scaled::new(0),
            right_ascension: Scaled::new(0),
            perigee: Scaled::new(0),
            mean_anomaly: Scaled::new(0),
            af0: Scaled::new(0),
            af1: Scaled::new(0),
        })
    }

    /// The orbit, in Table 20-IV's units.  The almanac has no harmonic
    /// corrections, mean motion correction or inclination rate, and its
    /// inclination is 0.30 semicircles less (20.3.3.5.2.1).
    fn orbit(&self) -> Orbit {
        let a = &self.0;
        Orbit {
            sqrt_a: root_meters(a.root_semi_major_axis),
            e: number(a.eccentricity),
            m0: semicircles(a.mean_anomaly),
            omega0: semicircles(a.right_ascension),
            omega_dot: semicircles_per_second(a.right_ascension_rate),
            i0: Radians::new((ALMANAC_I_REFERENCE + number(a.inclination_offset)) * PI),
            omega: semicircles(a.perigee),
            reference: seconds(a.time_of_almanac),
            ..Orbit::default()
        }
    }

    /// The satellite's position and velocity at GPS time of week `t`.
    pub(crate) fn state(&self, t: Seconds) -> State {
        self.orbit().state(t).0
    }

    /// The Doppler of the satellite's L1, positive approaching, seen from
    /// a stationary receiver at `receiver` (ECEF, m) at GPS time of week
    /// `t`: the satellite's motion alone, as the Oncore's.
    pub(crate) fn doppler(&self, t: Seconds, receiver: Ecef) -> Hertz<f64> {
        let s = self.state(t);
        let rate = s.velocity.along(s.position - receiver);
        rate.doppler()
    }
}

/// Orbital elements (Table 20-IV).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Orbit {
    /// The square root of the semi-major axis, and the eccentricity.
    sqrt_a: RootMeters,
    e: f64,
    /// The mean anomaly at the reference time, and the mean motion's
    /// correction.
    m0: Radians,
    delta_n: RadiansPerSecond,
    /// The ascending node's longitude at the week's start, and its rate.
    omega0: Radians,
    omega_dot: RadiansPerSecond,
    /// The inclination at the reference time, and its rate.
    i0: Radians,
    i_dot: RadiansPerSecond,
    /// The argument of perigee.
    omega: Radians,
    /// Second harmonic corrections to the argument of latitude, radius
    /// and inclination.
    cuc: Radians,
    cus: Radians,
    crc: Meters,
    crs: Meters,
    cic: Radians,
    cis: Radians,
    /// toe or toa, time of week.
    reference: Seconds,
}

/// The eccentric anomaly of mean anomaly `m` and eccentricity `e`, by
/// [`KEPLER_ITERATIONS`] of Newton's method on Kepler's equation.
fn eccentric_anomaly(m: Radians, e: f64) -> Radians {
    let mut ek = m;
    for _ in 0..KEPLER_ITERATIONS {
        ek += (m - ek + Radians::new(e * ek.sin())) / (1.0 - e * ek.cos());
    }
    ek
}

impl Orbit {
    /// The position and velocity at GPS time of week `t`, and the
    /// eccentric anomaly (Table 20-IV).
    fn state(&self, t: Seconds) -> (State, Radians) {
        let semi_major = self.sqrt_a * self.sqrt_a;
        let e = self.e;
        let n = MU.mean_motion(semi_major) + self.delta_n;
        let tk = t.since(self.reference);
        let m = self.m0 + n.angle(tk);
        let ek = eccentric_anomaly(m, e);
        let (sin_e, cos_e) = (ek.sin(), ek.cos());
        let root = sqrt(1.0 - e * e);
        let phi = Radians::new(atan2(root * sin_e, cos_e - e)) + self.omega;
        let (sin_2phi, cos_2phi) = ((SECOND_HARMONIC * phi).sin(), (SECOND_HARMONIC * phi).cos());
        let u = phi + self.cus * sin_2phi + self.cuc * cos_2phi;
        let r = semi_major * (1.0 - e * cos_e) + self.crs * sin_2phi + self.crc * cos_2phi;
        let i = self.i0 + self.cis * sin_2phi + self.cic * cos_2phi + self.i_dot.angle(tk);
        let omega_dot = self.omega_dot - OMEGA_E;
        let big_omega = self.omega0 + omega_dot.angle(tk) - OMEGA_E.angle(self.reference);
        let (x, y) = (r * u.cos(), r * u.sin());
        let (so, co, si, ci) = (big_omega.sin(), big_omega.cos(), i.sin(), i.cos());
        // Rates (sheet 3).
        let e_dot = n / (1.0 - e * cos_e);
        let nu_dot = e_dot * root / (1.0 - e * cos_e);
        let i_dot = self.i_dot + SECOND_HARMONIC * nu_dot * (self.cis * cos_2phi - self.cic * sin_2phi);
        let u_dot = nu_dot + SECOND_HARMONIC * nu_dot * (self.cus * cos_2phi - self.cuc * sin_2phi);
        let r_dot = e * semi_major * e_dot * sin_e
            + SECOND_HARMONIC * nu_dot * (self.crs * cos_2phi - self.crc * sin_2phi);
        let x_dot = r_dot * u.cos() - r * u_dot * u.sin();
        let y_dot = r_dot * u.sin() + r * u_dot * u.cos();
        let state = State {
            position: Ecef::from_axes([x * co - y * ci * so, x * so + y * ci * co, y * si]),
            velocity: EcefVelocity::from_axes([
                -x * omega_dot * so + x_dot * co
                    - y_dot * so * ci
                    - y * (omega_dot * co * ci - i_dot * so * si),
                x * omega_dot * co + x_dot * so + y_dot * co * ci
                    - y * (omega_dot * so * ci + i_dot * co * si),
                y_dot * si + y * i_dot * ci,
            ]),
        };
        (state, ek)
    }
}

/// The relativistic clock term's constant (20.3.3.3.3.1).
const F: SecondsPerRootMeter = SecondsPerRootMeter::new(-4.442_807_633e-10);

/// A satellite's subframes 1 to 3, the last of each received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subframes {
    first: Option<Subframe1>,
    second: Option<Subframe2>,
    third: Option<Subframe3>,
}

impl Subframes {
    /// None received.
    pub(crate) const fn new() -> Self {
        Self { first: None, second: None, third: None }
    }

    /// Some received: the satellite's ephemeris has been read from.
    pub(crate) const fn started(&self) -> bool {
        self.first.is_some() || self.second.is_some() || self.third.is_some()
    }

    /// Subframe 1 alone received, for tests.
    #[cfg(test)]
    pub(crate) const fn first() -> Self {
        Self {
            first: Some(Subframe1 {
                week: 0,
                l2_codes: 0,
                ura: 0,
                health: 0,
                iodc: 0,
                l2_p_data_off: false,
                group_delay: Scaled::new(0),
                toc: Scaled::new(0),
                af2: Scaled::new(0),
                af1: Scaled::new(0),
                af0: Scaled::new(0),
            }),
            ..Self::new()
        }
    }
}

impl Default for Subframes {
    fn default() -> Self {
        Self::new()
    }
}

/// A satellite's ephemeris and clock (subframes 1 to 3, 20.3.3.3 and
/// 20.3.3.4, Tables 20-I and 20-III), in Table 20-IV's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ephemeris {
    orbit: Orbit,
    /// Issue of data, ephemeris.
    pub(crate) iode: u8,
    /// Clock reference time, time of week, and polynomial, s, s/s, s/s^2.
    toc: Seconds,
    af0: Seconds,
    af1: SecondsPerSecond,
    af2: SecondsPerSecondSquared,
    /// L1 group delay.
    tgd: Seconds,
}

impl Subframes {
    /// The ephemeris, once subframes 1 to 3 carry one issue of data: IODC's
    /// low 8 bits and both IODEs (20.3.4.4).  Out of line, so its locals
    /// are off the stack before the caller goes on: inlined, they joined
    /// `oncore_app::run`'s frame (`make stack`).
    #[inline(never)]
    pub fn ephemeris(&self) -> Option<Ephemeris> {
        let (Some(s1), Some(s2), Some(s3)) = (self.first, self.second, self.third) else { return None };
        let iode = s2.iode;
        // IODC's low 8 bits.
        if s1.iodc as u8 != iode || s3.iode != iode {
            return None;
        }
        Some(Ephemeris {
            orbit: Orbit {
                sqrt_a: root_meters(s2.root_semi_major_axis),
                e: number(s2.eccentricity),
                m0: semicircles(s2.mean_anomaly),
                delta_n: semicircles_per_second(s2.mean_motion_difference),
                omega0: semicircles(s3.right_ascension),
                omega_dot: semicircles_per_second(s3.right_ascension_rate),
                i0: semicircles(s3.inclination),
                i_dot: semicircles_per_second(s3.inclination_rate),
                omega: semicircles(s3.perigee),
                cuc: radians(s2.cuc),
                cus: radians(s2.cus),
                crc: meters(s3.crc),
                crs: meters(s2.crs),
                cic: radians(s3.cic),
                cis: radians(s3.cis),
                reference: seconds(s2.toe),
            },
            iode,
            toc: seconds(s1.toc),
            af0: seconds(s1.af0),
            af1: SecondsPerSecond::new(number(s1.af1)),
            af2: SecondsPerSecondSquared::new(number(s1.af2)),
            tgd: seconds(s1.group_delay),
        })
    }
}

/// A satellite's position and velocity at a time of transmission, with
/// its L1 clock offset, and drift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Transmitter {
    pub(crate) state: State,
    pub(crate) clock: Seconds,
    pub(crate) drift: SecondsPerSecond,
}

/// The derivative of `dt` squared is this times `dt`: the clock
/// polynomial's af2 term's rate.
const SQUARE_DERIVATIVE: f64 = 2.0;

/// An ephemeris's curve fit interval, s, and half of it: four hours
/// with the fit interval flag clear, toe in its middle (IS-GPS-200,
/// 20.3.3.4.3.1).
const FIT_INTERVAL_HOURS: i64 = 4;
const FIT_INTERVAL_S: f64 = (FIT_INTERVAL_HOURS * SECONDS_PER_HOUR) as f64;
const FIT_HALF_S: f64 = FIT_INTERVAL_S / HALVES as f64;
const FIT_HALF: Seconds = Seconds::new(FIT_HALF_S);

impl Ephemeris {
    /// Whether GPS time of week `t` is within its fit interval.
    pub(crate) fn fits(&self, t: Seconds) -> bool {
        t.since(self.orbit.reference).abs() <= FIT_HALF
    }

    /// At GPS time of week `t`: the position and velocity (Table 20-IV)
    /// and the L1 clock offset with its relativistic term less the group
    /// delay (20.3.3.3.3.1-2).
    pub(crate) fn transmitter(&self, t: Seconds) -> Transmitter {
        let (state, ek) = self.orbit.state(t);
        let dt = t.since(self.toc);
        let relativistic = F * self.orbit.e * self.orbit.sqrt_a * ek.sin();
        Transmitter {
            state,
            clock: self.af0 + self.af1 * dt + self.af2 * dt * dt + relativistic - self.tgd,
            drift: self.af1 + SQUARE_DERIVATIVE * self.af2 * dt,
        }
    }
}

/// What a satellite's own subframes say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Broadcast {
    /// From its last HOW (bits 18 and 19).
    pub(crate) alert: bool,
    pub(crate) anti_spoof: bool,
    /// From its last subframe 1: URA index 0 to 15 and 6-bit health.
    pub(crate) ura: Option<u8>,
    pub(crate) health: u8,
}

impl Broadcast {
    /// Nothing decoded.
    pub(crate) const fn new() -> Self {
        Self { alert: false, anti_spoof: false, ura: None, health: 0 }
    }

    /// Its URA says it is inaccurate, over 16 m.
    pub(crate) fn inaccurate(&self) -> bool {
        self.ura.is_some_and(|u| u >= URA_INACCURATE)
    }
}

impl Default for Broadcast {
    fn default() -> Self {
        Self::new()
    }
}

/// URA index 7 and up: a nominal URA over 16 m (IS-GPS-200N
/// 20.3.3.3.1.3, where index 6's is 16 m).
const URA_INACCURATE: u8 = 7;

/// The navigation message gathered from the module's subframes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nav {
    pub(crate) broadcast: [Broadcast; PRNS],
    pub almanac: [Option<Almanac>; PRNS],
    pub ephemeris: [Subframes; PRNS],
    /// The almanac's reference time, toa in 2^12 s, and week, 8 bits
    /// (subframe 5 page 25).
    pub(crate) almanac_reference: Option<(u8, u8)>,
    /// The UTC parameters, the last page 18 heard.
    pub utc: Option<Utc>,
}

impl Nav {
    /// Nothing decoded.
    #[expect(
        clippy::large_stack_arrays,
        reason = "a const fn: the firmware builds it at compile time, in oncore-app's static State"
    )]
    #[expect(
        clippy::new_without_default,
        reason = "too large to build and return by value: built in place (AGENTS.md, Stack and RAM)"
    )]
    pub const fn new() -> Self {
        Self {
            broadcast: [Broadcast::new(); PRNS],
            almanac: [None; PRNS],
            ephemeris: [Subframes::new(); PRNS],
            almanac_reference: None,
            utc: None,
        }
    }
}

impl Nav {
    /// Takes a GPS L1 C/A subframe from satellite `prn`.
    pub fn subframe(&mut self, prn: Prn, words: &Words) {
        let Some(subframe) = Subframe::decode(words) else { return };
        let b = &mut self.broadcast[prn.index()];
        b.alert = subframe.handover.alert;
        b.anti_spoof = subframe.handover.anti_spoof;
        let s = &mut self.ephemeris[prn.index()];
        match subframe.body {
            Body::Subframe1(s1) => {
                (b.ura, b.health) = (Some(s1.ura), s1.health);
                s.first = Some(s1);
            }
            Body::Subframe2(s2) => s.second = Some(s2),
            Body::Subframe3(s3) => s.third = Some(s3),
            // Almanac pages: subframe 5's for 1 to 24, subframe 4's for 25
            // to 32 (Figure 20-1, sheet 4).
            Body::Almanac(a) => {
                if let Some(p) = Prn::new(a.satellite) {
                    self.almanac[p.index()] = Some(Almanac(a));
                }
            }
            Body::IonosphereUtc(u) => self.utc = Some(Utc(u)),
            Body::AlmanacHealth(h) => self.almanac_reference = Some((h.time_of_almanac.raw, h.week)),
            Body::Configurations(_) | Body::Page(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use smartclock_ubx::subframe;
    use smartclock_ubx::subframe::Scaled;
    use smartclock_ubx::subframe::WORDS;
    use smartclock_ubx::subframe::Words;

    use super::ALMANAC_I_REFERENCE;
    use super::Almanac;
    use super::Ephemeris;
    use super::FIT_HALF;
    use super::GpsSeconds;
    use super::GpsWeek;
    use super::MU;
    use super::NS_PER_SECOND;
    use super::Nav;
    use super::OMEGA_E;
    use super::Seconds;
    use super::number;
    use super::pow2;
    use crate::fixtures::prn;
    use crate::sky::Prn;
    use crate::units::DEGREES_PER_HALF_CIRCLE;
    use crate::units::Meters;
    use crate::units::MetersPerSecond;
    use crate::units::RootMeters;
    use crate::units::SECONDS_PER_WEEK;
    use crate::units::SecondsPerSecond;
    use crate::units::SecondsPerSecondSquared;
    use crate::units::WEEK_MODULUS;

    /// A field's place: its word, 1 to 10, and its bits, numbered 1 to 30
    /// from the first sent, as IS-GPS-200 Figures 20-1 and 20-2 number
    /// them.
    #[derive(Debug, Clone, Copy)]
    struct At {
        word: usize,
        first: u32,
        len: u32,
    }

    /// A word's bits.
    const WORD_BITS: u32 = 30;
    /// The HOW's subframe ID (Figure 20-2); subframes 4 and 5's SV ID
    /// (Figure 20-1 sheets 4 and 5); page 18's fields (sheet 8), A0
    /// across words 7 and 8.
    const SUBFRAME_ID: At = At { word: 2, first: 20, len: 3 };
    const SV_ID: At = At { word: 3, first: 3, len: 6 };
    const UTC_A1: At = At { word: 6, first: 1, len: 24 };
    const UTC_A0_MSB: At = At { word: 7, first: 1, len: 24 };
    const UTC_A0_LSB: At = At { word: 8, first: 1, len: 8 };
    const UTC_TOT: At = At { word: 8, first: 9, len: 8 };
    const UTC_WNT: At = At { word: 8, first: 17, len: 8 };
    const UTC_DELTA_T_LS: At = At { word: 9, first: 1, len: 8 };
    const UTC_WN_LSF: At = At { word: 9, first: 9, len: 8 };
    const UTC_DN: At = At { word: 9, first: 17, len: 8 };
    const UTC_DELTA_T_LSF: At = At { word: 10, first: 1, len: 8 };
    /// Subframe 4, and page 18's SV ID (Table 20-V).
    const UTC_SUBFRAME: u32 = 4;
    const UTC_PAGE: u32 = 56;
    /// A0's scale, s, A1's, s/s, and tot's, s (Table 20-IX).
    const A0_SCALE: f64 = pow2(-30);
    const A1_SCALE: f64 = pow2(-50);
    const TOT_SCALE: f64 = pow2(12);

    /// Puts `value`'s low bits at `at` in `words`, in the module's layout,
    /// the 30 bits low.
    fn put(words: &mut [u32; WORDS], at: At, value: u32) {
        words[at.word - 1] |= (value & ((1 << at.len) - 1)) << (WORD_BITS - (at.first + at.len - 1));
    }

    /// Worked by hand from Figure 20-2, in the module's layout (the 30
    /// bits low): a HOW with alert (bit 18), anti-spoof (19) and subframe
    /// ID 1 (20-22); a subframe 1 word 3 with URA index 7 (bits 13-16)
    /// and health 0b100001 (17-22); from PRN 5.
    const HOW_ALERT_AS_SUBFRAME_1: u32 = (1 << 12) | (1 << 11) | (1 << 8);
    const WORD_3: u32 = ((7 << 8) | (0b10_0001 << 2)) << 6;
    const URA_INDEX: u8 = 7;
    const HEALTH_BITS: u8 = 0b10_0001;
    const PRN: Prn = prn(5);
    /// The HOW's and word 3's places in a subframe.
    const HOW: usize = 1;
    const THIRD: usize = 2;

    /// Page 18's fields where Figure 20-1 sheet 8 puts them, A0 across
    /// words 7 and 8, the signed ones negative.
    #[test]
    fn subframe_4_page_18() {
        const A1: i32 = -3;
        const A0: i32 = -0x1234_5678;
        const TOT: u8 = 0x90;
        const WNT: u8 = 0x81;
        const DELTA_T_LS: i8 = 18;
        const WN_LSF: u8 = 137;
        const DN: u8 = 7;
        const DELTA_T_LSF: i8 = -5;
        /// Whole cycles of 256 weeks, to a week `WNt` names.
        const WEEKS: u16 = WEEK_MODULUS as u16 * 8;
        let mut nav = Nav::new();
        let mut w = [0u32; WORDS];
        put(&mut w, SUBFRAME_ID, UTC_SUBFRAME);
        put(&mut w, SV_ID, UTC_PAGE);
        put(&mut w, UTC_A1, A1 as u32);
        put(&mut w, UTC_A0_MSB, (A0 as u32) >> UTC_A0_LSB.len);
        put(&mut w, UTC_A0_LSB, A0 as u32);
        put(&mut w, UTC_TOT, u32::from(TOT));
        put(&mut w, UTC_WNT, u32::from(WNT));
        put(&mut w, UTC_DELTA_T_LS, u32::from(DELTA_T_LS as u8));
        put(&mut w, UTC_WN_LSF, u32::from(WN_LSF));
        put(&mut w, UTC_DN, u32::from(DN));
        put(&mut w, UTC_DELTA_T_LSF, u32::from(DELTA_T_LSF as u8));
        nav.subframe(PRN, &Words::new(w));
        let utc = nav.utc.unwrap();
        let u = utc.broadcast();
        assert_eq!((u.a1.raw, u.a0.raw, u.reference_time.raw, u.reference_week), (A1, A0, TOT, WNT));
        assert_eq!(
            (u.gps_less_utc.0, u.leap_week, u.leap_day, u.gps_less_utc_after.0),
            (DELTA_T_LS, WN_LSF, DN, DELTA_T_LSF)
        );
        assert!(utc.change_scheduled());
        // At tot in week WNt, A0 alone; a week later, A1 for a week more.
        let at_tot = GpsSeconds::week_start(GpsWeek::new(u16::from(WNT) + WEEKS))
            + Seconds::new((f64::from(TOT) * TOT_SCALE) as i64);
        let ns = |s: f64| libm::round(s * NS_PER_SECOND as f64) as i32;
        assert_eq!(utc.fraction(at_tot).get(), ns(f64::from(A0) * A0_SCALE));
        let later = at_tot + Seconds::new(SECONDS_PER_WEEK);
        assert_eq!(
            utc.fraction(later).get(),
            ns(f64::from(A0) * A0_SCALE + f64::from(A1) * A1_SCALE * SECONDS_PER_WEEK as f64)
        );
    }

    #[test]
    fn how_and_subframe_1() {
        let mut nav = Nav::new();
        let mut w = [0u32; WORDS];
        w[HOW] = HOW_ALERT_AS_SUBFRAME_1;
        w[THIRD] = WORD_3;
        nav.subframe(PRN, &Words::new(w));
        let b = nav.broadcast[PRN.index()];
        assert!(b.alert && b.anti_spoof);
        assert_eq!((b.ura, b.health), (Some(URA_INDEX), HEALTH_BITS));
        assert!(nav.ephemeris[PRN.index()].started());
    }

    /// GPS's sqrt(A), and inclination, degrees and semicircles.
    const SQRT_A: RootMeters = RootMeters::new(5153.7);
    const INCLINATION_DEGREES: f64 = 55.0;
    const INCLINATION: f64 = INCLINATION_DEGREES / DEGREES_PER_HALF_CIRCLE as f64;
    /// The almanac's scales (Table 20-VI): delta i, 2^-19 semicircles;
    /// sqrt(A), 2^-11 m^1/2.
    const DELTA_I_SCALE: f64 = pow2(-19);
    const SQRT_A_SCALE: f64 = pow2(-11);

    /// A circular orbit at GPS's radius and inclination, at its node at
    /// time 0.
    fn circular() -> Almanac {
        Almanac(subframe::Almanac {
            inclination_offset: Scaled::new(((INCLINATION - ALMANAC_I_REFERENCE) / DELTA_I_SCALE) as i16),
            root_semi_major_axis: Scaled::new((SQRT_A.get() / SQRT_A_SCALE) as u32),
            ..Almanac::zero_at(0).0
        })
    }

    /// The circular orbit's radius is A, sqrt(A) as broadcast squared.
    #[test]
    fn a_circular_orbit_has_its_radius() {
        const RADIUS_TOLERANCE: Meters = Meters::new(0.01);
        let alm = circular();
        let r = alm.state(Seconds::new(0.0)).position.norm();
        let root_a = RootMeters::new(number(alm.0.root_semi_major_axis));
        let a = root_a * root_a;
        assert!((r - a).abs() < RADIUS_TOLERANCE, "radius {r:?} vs {a:?}");
    }

    /// At its node, the orbit is in the equatorial plane.
    #[test]
    fn a_circular_orbit_crosses_the_equator_at_its_node() {
        /// The node's height from the plane.
        const NODE_TOLERANCE: Meters = Meters::new(1.0);
        let [_, _, z] = circular().state(Seconds::new(0.0)).position.get();
        assert!(Meters::new(z).abs() < NODE_TOLERANCE);
    }

    /// The orbit's speed is sqrt(mu/A) in inertial space; the earth-fixed
    /// speed differs by the earth's rotation at that radius, at most
    /// `OMEGA_E` r.
    #[test]
    fn a_circular_orbit_has_its_speed() {
        /// The tolerance past the earth's rotation.
        const SPEED_TOLERANCE: MetersPerSecond = MetersPerSecond::new(1.0);
        let s = circular().state(Seconds::new(0.0));
        let (r, v) = (s.position.norm(), s.velocity.norm());
        let a = SQRT_A * SQRT_A;
        let inertial = MU.mean_motion(a) * a;
        let rotation = OMEGA_E * r;
        assert!((v - inertial).abs() < rotation + SPEED_TOLERANCE, "speed {v:?} vs {inertial:?}");
    }

    /// Velocity is the derivative of position: a second's step against it.
    #[test]
    fn a_circular_orbit_moves_at_its_velocity() {
        /// A second's step, and its position difference against the
        /// velocity's.
        const STEP: Seconds = Seconds::new(1.0);
        const STEP_TOLERANCE: Meters = Meters::new(0.5);
        let alm = circular();
        let s = alm.state(Seconds::new(0.0));
        let later = alm.state(STEP).position.get();
        for (k, ((after, before), velocity)) in
            later.into_iter().zip(s.position.get()).zip(s.velocity.get()).enumerate()
        {
            let numeric = Meters::new(after) - Meters::new(before);
            let moved = MetersPerSecond::new(velocity) * STEP;
            assert!((numeric - moved).abs() < STEP_TOLERANCE, "axis {k}: {numeric:?} vs {moved:?}");
        }
    }

    #[test]
    fn an_ephemeris_fits_two_hours_either_side_of_toe_across_the_week() {
        /// A toe near the week's end, a week, and a second.
        const TOE: Seconds = Seconds::new(600_000.0);
        const WEEK: Seconds = Seconds::new(SECONDS_PER_WEEK as f64);
        const SECOND: Seconds = Seconds::new(1.0);
        let orbit = super::Orbit { reference: TOE, ..Default::default() };
        let e = Ephemeris {
            orbit,
            iode: 0,
            toc: Seconds::default(),
            af0: Seconds::default(),
            af1: SecondsPerSecond::default(),
            af2: SecondsPerSecondSquared::default(),
            tgd: Seconds::default(),
        };
        assert!(e.fits(TOE - FIT_HALF));
        assert!(!e.fits(TOE - FIT_HALF - SECOND));
        // Past the week's end.
        assert!(e.fits(TOE + FIT_HALF - WEEK));
        assert!(!e.fits(TOE + FIT_HALF + SECOND - WEEK));
    }
}
