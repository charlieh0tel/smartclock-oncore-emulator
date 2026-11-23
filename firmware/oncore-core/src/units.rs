//! Quantities with units, each a newtype that carries its own
//! conversions, so a unit is never a bare integer and its factors are
//! written once.

use core::cmp::Ordering;
use core::iter::Sum;
use core::num::TryFromIntError;
use core::ops::Add;
use core::ops::AddAssign;
use core::ops::Div;
use core::ops::Mul;
use core::ops::Neg;
use core::ops::Sub;
use core::ops::SubAssign;

use libm::atan2;
use libm::cos;
use libm::round;
use libm::sin;
use libm::sqrt;
use libm::sqrtf;

use crate::civil::DAYS_PER_WEEK;
use crate::civil::DecimalYear;
use crate::civil::GPS_EPOCH;
use crate::civil::SECONDS_PER_DAY;
use crate::civil::SECONDS_PER_MINUTE;
use crate::civil::UnixSeconds;
use crate::module::WGS84_E2;
use crate::module::WGS84_SEMI_MAJOR_M;
use crate::vector::AXES;
use crate::vector::Direction;
use crate::vector::dot;

/// Seconds in a GPS week.
pub(crate) const SECONDS_PER_WEEK: i64 = DAYS_PER_WEEK * SECONDS_PER_DAY;
/// Milliseconds in a second.
const MS_PER_SECOND: i64 = 1000;
/// Nanoseconds in a second, and in a millisecond.
pub(crate) const NS_PER_SECOND: i64 = 1_000_000_000;
const NS_PER_MS: i64 = NS_PER_SECOND / MS_PER_SECOND;
/// Seconds in a nanosecond.
const SECONDS_PER_NS: f64 = 1e-9;
/// Picoseconds in a nanosecond.
const PS_PER_NS: i32 = 1000;
/// The speed of light, m/s (IS-GPS-200, 20.3.3.3.3.1), and the GPS L1
/// carrier's frequency, Hz.
pub(crate) const SPEED_OF_LIGHT: f64 = 299_792_458.0;
const L1_HZ: f64 = 1_575_420_000.0;
/// L1's wavelength, m.
const L1_WAVELENGTH: f64 = SPEED_OF_LIGHT / L1_HZ;
/// Range at the speed of light: ns per 0.1 m, to six places, and 0.1 m
/// per microsecond, to the nearest, as the Oncore reports a clock's bias
/// and drift in range.
const NS_PER_DECIMETER: f32 = ((NS_PER_SECOND as f64 / DECIMETERS_PER_METER / SPEED_OF_LIGHT * MILLIONTHS
    + HALF_F64) as i64) as f32
    / MILLIONTHS as f32;
const DECIMETERS_PER_US: i64 = (SPEED_OF_LIGHT * DECIMETERS_PER_METER / US_PER_SECOND + HALF_F64) as i64;
const NS_PER_US: i64 = 1000;
const DECIMETERS_PER_METER: f64 = 10.0;
const US_PER_SECOND: f64 = 1e6;
const MILLIONTHS: f64 = 1e6;
/// Hz of L1 Doppler per ns/s of clock drift.
const L1_HZ_PER_NS_PER_S: f32 = (L1_HZ * SECONDS_PER_NS) as f32;
/// Millimeters in a centimeter, a decimeter and a meter; centimeters in
/// a meter and a kilometer.
const MM_PER_CM: i32 = 10;
const MM_PER_DM: i32 = 100;
const MM_PER_M: f64 = 1000.0;
const CM_PER_M: f64 = 100.0;
const CM_PER_KM: f64 = CM_PER_M * M_PER_KM;
/// Meters in a kilometer.
pub(crate) const M_PER_KM: f64 = 1000.0;
/// Milliarcseconds in a degree; the module's 1e-7 degree units in one,
/// and its 1e-5 degree units in a tenth of one.
const MAS_PER_DEGREE: i32 = 3_600_000;
const DEGREES_1E7_PER_DEGREE: i64 = 10_000_000;
const DEGREES_1E5_PER_TENTH: i32 = 10_000;
/// Tenths in a unit; in a second and a week, as `@@Ce` gives its time.
pub(crate) const TENTHS_PER_UNIT: u8 = 10;
pub const TENTHS_PER_SECOND: u32 = TENTHS_PER_UNIT as u32;
const TENTHS_PER_WEEK: u32 = SECONDS_PER_WEEK as u32 * TENTHS_PER_SECOND;
/// Degrees in a circle, in half and in a quarter of one; tenths of a
/// degree in a circle.
pub const DEGREES_PER_CIRCLE: i16 = 360;
pub(crate) const DEGREES_PER_HALF_CIRCLE: i16 = DEGREES_PER_CIRCLE / HALVES as i16;
pub(crate) const DEGREES_PER_QUARTER_CIRCLE: i16 = DEGREES_PER_HALF_CIRCLE / HALVES as i16;
pub(crate) const TENTHS_PER_CIRCLE: i32 = DEGREES_PER_CIRCLE as i32 * TENTHS_PER_UNIT as i32;
/// A minute.
pub const MINUTE: Seconds<i64> = Seconds(SECONDS_PER_MINUTE);
/// ns in Time RAIM's alarm unit.
const NS_PER_HUNDRED_NS: i32 = 100;
/// A half, as a rounding adds it, in `f32` and `f64`; halves in a whole,
/// as a rounding adds half its divisor.
const HALF: f32 = 0.5;
const HALF_F64: f64 = 0.5;
pub(crate) const HALVES: i64 = 2;

/// `x` and a half: cast to an integer, which truncates, `x` rounded to
/// the nearest, halves up, for `x` above -0.5.  Not `roundf`, which
/// takes halves away from zero and differs just below a half.
pub(crate) fn plus_half(x: f32) -> f32 {
    x + HALF
}

/// The square root, libm's, and 0 for a square of 0 or less, which
/// rounding can leave.
pub(crate) fn square_root(x: f32) -> f32 {
    if x <= 0.0 { 0.0 } else { sqrtf(x) }
}
/// Seconds in a week, and in half of one, `f64`.
const WEEK_S: f64 = SECONDS_PER_WEEK as f64;
const HALF_WEEK_S: f64 = WEEK_S * HALF_F64;

/// `v` times `num` over `den`, to the nearest, halves away from zero.
const fn scale_rounded(v: i64, num: i64, den: i64) -> i64 {
    let x = v * num;
    (x + x.signum() * (den / HALVES)) / den
}

/// A GPS week number, counted from the GPS epoch, not modulo 1024.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GpsWeek(u16);

/// The 8-bit weeks' modulus: page 18's `WNt` and WNLSF (IS-GPS-200
/// 20.3.3.5.2.4).
pub(crate) const WEEK_MODULUS: i64 = 256;

/// A difference of weeks modulo 256 as the nearest, -128 to 127.
pub(crate) fn nearest_weeks(weeks: i64) -> i64 {
    let low = weeks.rem_euclid(WEEK_MODULUS);
    if low >= WEEK_MODULUS / 2 { low - WEEK_MODULUS } else { low }
}

impl GpsWeek {
    pub const fn new(week: u16) -> Self {
        Self(week)
    }

    pub(crate) const fn get(self) -> u16 {
        self.0
    }

    /// The week before it.
    pub const fn previous(self) -> Self {
        Self(self.0 - 1)
    }

    /// Its low 8 bits, as an almanac's week carries it.
    pub const fn almanac_week(self) -> u8 {
        self.0 as u8
    }

    /// The week whose low 8 bits are `low` nearest this one, -128 to 127
    /// weeks away: a week as page 18 or page 25 broadcasts it, in full.
    pub(crate) fn nearest(self, low: u8) -> Self {
        Self((i64::from(self.0) + nearest_weeks(i64::from(low) - i64::from(self.0))) as u16)
    }

    /// The week a time falls in, if it is one a `u16` holds.
    pub(crate) fn containing(t: GpsSeconds) -> Option<Self> {
        u16::try_from(t.0.div_euclid(SECONDS_PER_WEEK)).ok().map(Self)
    }
}

/// Time of week: milliseconds from the start of the GPS week, Sunday
/// 00:00:00 GPS time, as u-blox's iTOW.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfWeek(u32);

impl TimeOfWeek {
    pub const fn from_ms(ms: u32) -> Self {
        Self(ms)
    }

    pub(crate) const fn ms(self) -> Milliseconds<u32> {
        Milliseconds(self.0)
    }

    /// Seconds, with the milliseconds as a fraction.
    pub fn seconds(self) -> Seconds {
        Seconds(f64::from(self.0) / MS_PER_SECOND as f64)
    }
}

/// GPS time: seconds from the GPS epoch, 1980-01-06 00:00:00, with no
/// leap seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GpsSeconds(i64);

impl GpsSeconds {
    #[cfg(test)]
    pub(crate) const fn new(seconds: i64) -> Self {
        Self(seconds)
    }

    pub const fn get(self) -> i64 {
        self.0
    }

    /// From a Unix time, as GPS time, no leap seconds taken.
    pub(crate) const fn from_unix(unix: UnixSeconds) -> Self {
        Self(unix.get() - GPS_EPOCH)
    }

    /// As a Unix time, no leap seconds taken.
    pub(crate) const fn unix(self) -> UnixSeconds {
        UnixSeconds::new(self.0 + GPS_EPOCH)
    }

    /// The start of a week, Sunday 00:00:00.
    pub(crate) fn week_start(week: GpsWeek) -> Self {
        Self(i64::from(week.get()) * SECONDS_PER_WEEK)
    }

    /// From a week and time of week, `fraction` added to the time of week
    /// (-500000 to 500000 ns in NAV-TIMEGPS), to the nearest second.
    pub(crate) fn from_week(week: GpsWeek, tow: TimeOfWeek, fraction: Nanoseconds) -> Self {
        Self::week_start(week) + (tow.ms().nanoseconds() + Nanoseconds::<i64>::from(fraction)).round_seconds()
    }

    /// Seconds into its week.
    pub(crate) fn seconds_of_week(self) -> Seconds<i64> {
        Seconds(self.0.rem_euclid(SECONDS_PER_WEEK))
    }
}

impl Add<Seconds<i64>> for GpsSeconds {
    type Output = Self;

    fn add(self, seconds: Seconds<i64>) -> Self {
        Self(self.0 + seconds.0)
    }
}

impl AddAssign<Seconds<i64>> for GpsSeconds {
    fn add_assign(&mut self, seconds: Seconds<i64>) {
        self.0 += seconds.0;
    }
}

impl Sub<Seconds<i64>> for GpsSeconds {
    type Output = Self;

    fn sub(self, seconds: Seconds<i64>) -> Self {
        Self(self.0 - seconds.0)
    }
}

/// The interval between two times.
impl Sub for GpsSeconds {
    type Output = Seconds<i64>;

    fn sub(self, other: Self) -> Seconds<i64> {
        Seconds(self.0 - other.0)
    }
}

impl Add<Seconds<i64>> for UnixSeconds {
    type Output = Self;

    fn add(self, s: Seconds<i64>) -> Self {
        Self::new(self.get() + s.0)
    }
}

impl Sub<Seconds<i64>> for UnixSeconds {
    type Output = Self;

    fn sub(self, s: Seconds<i64>) -> Self {
        Self::new(self.get() - s.0)
    }
}

/// A time or an interval, ns: `i32` in the Oncore's and the module's
/// fields, `u32`, `u16` and `i8` in `@@Ba`'s and `@@Bn`'s, `f32` in Time
/// RAIM's arithmetic, `i64` in a time of week's and a pulse's sums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Nanoseconds<T = i32>(T);

impl<T: Copy> Nanoseconds<T> {
    pub(crate) const fn new(ns: T) -> Self {
        Self(ns)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Nanoseconds<f32> {
    pub(crate) fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// The order of `self` and `other`, NaN included ([`f32::total_cmp`]).
    pub(crate) fn total_cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }

    pub(crate) fn squared(self) -> SquareNanoseconds {
        SquareNanoseconds(self.0 * self.0)
    }

    /// To the nearest, halves up, for above -0.5 ([`plus_half`]).
    pub(crate) fn round(self) -> Nanoseconds<i64> {
        Nanoseconds(plus_half(self.0) as i64)
    }

    /// To the nearest, halves up, held at `u16`'s most.
    pub(crate) fn round_u16(self) -> Nanoseconds<u16> {
        Nanoseconds(plus_half(self.0).min(u16::MAX.into()) as u16)
    }
}

impl Nanoseconds<i64> {
    /// To the nearest second, halves up.
    pub(crate) const fn round_seconds(self) -> Seconds<i64> {
        Seconds((self.0 + NS_PER_SECOND / HALVES).div_euclid(NS_PER_SECOND))
    }

    /// Its time into the second.
    pub(crate) const fn within_second(self) -> Nanoseconds<u32> {
        Nanoseconds(self.0.rem_euclid(NS_PER_SECOND) as u32)
    }
}

impl Add for Nanoseconds<i64> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

/// A squared time, ns^2: Time RAIM's sum of squares and variance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct SquareNanoseconds(f32);

impl SquareNanoseconds {
    /// Its square root, 0 for 0 or less ([`square_root`]).
    pub(crate) fn sqrt(self) -> Nanoseconds<f32> {
        Nanoseconds(square_root(self.0))
    }
}

/// The mean of a sum of `n`.
impl Div<f32> for SquareNanoseconds {
    type Output = Self;

    fn div(self, n: f32) -> Self {
        Self(self.0 / n)
    }
}

/// As their values' `f32` sum.
impl Sum for SquareNanoseconds {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self(iter.map(|v| v.0).sum())
    }
}

impl From<Nanoseconds> for Nanoseconds<i64> {
    fn from(ns: Nanoseconds) -> Self {
        Self(ns.0.into())
    }
}

impl Neg for Nanoseconds {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Add for Nanoseconds<f32> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl AddAssign for Nanoseconds<f32> {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl Sub for Nanoseconds<f32> {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// The mean of a sum of `n`.
impl Div<f32> for Nanoseconds<f32> {
    type Output = Self;

    fn div(self, n: f32) -> Self {
        Self(self.0 / n)
    }
}

impl Nanoseconds {
    /// As `f32`.
    pub(crate) fn to_f32(self) -> Nanoseconds<f32> {
        Nanoseconds(self.0 as f32)
    }

    /// `self` less `other`, held at the ends of the range.
    pub(crate) const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Held within `i8`'s range.
    pub(crate) fn saturating_i8(self) -> Nanoseconds<i8> {
        Nanoseconds(self.0.clamp(i8::MIN.into(), i8::MAX.into()) as i8)
    }

    pub(crate) fn seconds(self) -> Seconds {
        Seconds(f64::from(self.0) * SECONDS_PER_NS)
    }

    /// The range light travels in it, truncated.
    pub(crate) fn decimeters(self) -> Decimeters<i64> {
        Decimeters(range_decimeters(i64::from(self.0)))
    }
}

/// ns of light travel time to 0.1 m of range, truncated.
const fn range_decimeters(ns: i64) -> i64 {
    ns * DECIMETERS_PER_US / NS_PER_US
}

/// A clock's drift, ns/s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NanosecondsPerSecond(i32);

impl NanosecondsPerSecond {
    pub(crate) const fn new(drift: i32) -> Self {
        Self(drift)
    }

    pub const fn get(self) -> i32 {
        self.0
    }

    /// The L1 Doppler it makes.
    pub fn l1_doppler(self) -> Hertz<f32> {
        Hertz(self.0 as f32 * L1_HZ_PER_NS_PER_S)
    }

    /// Its magnitude as a range rate, truncated.
    pub(crate) fn decimeters_per_second(self) -> DecimetersPerSecond<i64> {
        DecimetersPerSecond(range_decimeters(i64::from(self.0).abs()))
    }
}

/// A time interval, ps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Picoseconds(i32);

impl Picoseconds {
    pub(crate) const fn new(ps: i32) -> Self {
        Self(ps)
    }

    /// To the nearest ns, halves away from zero.
    pub(crate) const fn round_ns(self) -> Nanoseconds {
        Nanoseconds(scale_rounded(self.0 as i64, 1, PS_PER_NS as i64) as i32)
    }
}

/// A range, 0.1 m: `i16` for NAV-SAT's pseudorange residual, `i64` for a
/// clock's bias in range, as `@@Bk` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Decimeters<T = i16>(T);

impl<T: Copy> Decimeters<T> {
    pub(crate) const fn new(dm: T) -> Self {
        Self(dm)
    }

    pub(crate) const fn get(self) -> T {
        self.0
    }
}

impl<T: Neg<Output = T>> Neg for Decimeters<T> {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Decimeters {
    /// The time light takes to travel it.
    pub(crate) fn to_ns(self) -> Nanoseconds<f32> {
        Nanoseconds(f32::from(self.0) * NS_PER_DECIMETER)
    }
}

/// A range rate, 0.1 m/s: `i64` for a clock's drift in range, `i32` for
/// a velocity, as `@@Bk` reports them; `u16` and `i16` its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct DecimetersPerSecond<T>(T);

impl<T: Copy> DecimetersPerSecond<T> {
    pub(crate) const fn new(v: T) -> Self {
        Self(v)
    }

    pub(crate) const fn get(self) -> T {
        self.0
    }
}

impl<T: Neg<Output = T>> Neg for DecimetersPerSecond<T> {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

/// A height or a distance, cm: `i32` for the Oncore's and the module's
/// fields, `f64` for a pseudorange correction before it is rounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Centimeters<T = i32>(T);

impl<T: Copy> Centimeters<T> {
    pub const fn new(cm: T) -> Self {
        Self(cm)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Centimeters<f64> {
    /// To the nearest, halves away from zero.
    pub fn round(self) -> Self {
        Self(round(self.0))
    }

    /// Held to an `i32`, the fraction dropped.
    pub fn truncate(self) -> Centimeters {
        Centimeters(self.0 as i32)
    }
}

impl Centimeters {
    /// `self` and `other`, at compile time.
    pub(crate) const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// In mm, if an `i32` holds it.
    pub const fn checked_millimeters(self) -> Option<Millimeters> {
        match self.0.checked_mul(MM_PER_CM) {
            Some(mm) => Some(Millimeters(mm)),
            None => None,
        }
    }

    pub fn meters(self) -> Meters {
        Meters(f64::from(self.0) / CM_PER_M)
    }

    pub(crate) fn kilometers(self) -> Kilometers {
        Kilometers(f64::from(self.0) / CM_PER_KM)
    }
}

impl Add for Centimeters {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.plus(other)
    }
}

impl Sub for Centimeters {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// A length, km: the World Magnetic Model's.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct Kilometers(f64);

impl Kilometers {
    pub(crate) const fn new(km: f64) -> Self {
        Self(km)
    }

    /// The hypotenuse of `self` and `other`, by its square root.
    pub(crate) fn root_sum_square(self, other: Self) -> Self {
        Self(sqrt(self.0 * self.0 + other.0 * other.0))
    }
}

/// The ratio of two lengths.
impl Div for Kilometers {
    type Output = f64;

    fn div(self, other: Self) -> f64 {
        self.0 / other.0
    }
}

/// An angle, milliarcseconds: the Oncore's latitude and longitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct MilliArcSeconds(i32);

impl Neg for MilliArcSeconds {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl MilliArcSeconds {
    pub const fn new(mas: i32) -> Self {
        Self(mas)
    }

    pub const fn get(self) -> i32 {
        self.0
    }

    /// From the module's 1e-7 degree, to the nearest.
    pub(crate) const fn from_degrees_1e7(v: Degrees1e7) -> Self {
        Self(scale_rounded(v.0 as i64, MAS_PER_DEGREE as i64, DEGREES_1E7_PER_DEGREE) as i32)
    }

    /// In the module's 1e-7 degree, to the nearest.
    pub const fn degrees_1e7(self) -> Degrees1e7 {
        Degrees1e7(scale_rounded(self.0 as i64, DEGREES_1E7_PER_DEGREE, MAS_PER_DEGREE as i64) as i32)
    }

    pub fn degrees(self) -> Degrees<f64> {
        Degrees(f64::from(self.0) / f64::from(MAS_PER_DEGREE))
    }
}

/// A height or a distance, mm: `i32` for NAV-PVT's heights, `u32` for
/// CFG-TMODE2's accuracy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Millimeters<T = i32>(T);

impl<T: Copy> Millimeters<T> {
    pub const fn new(mm: T) -> Self {
        Self(mm)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Millimeters {
    /// In cm, truncated.
    pub(crate) const fn centimeters(self) -> Centimeters {
        Centimeters(self.0 / MM_PER_CM)
    }

    pub fn meters(self) -> Meters {
        Meters(f64::from(self.0) / MM_PER_M)
    }
}

impl Millimeters {
    /// `self` and `other`, if an `i32` holds it.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }
}

impl Add for Millimeters {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Millimeters {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// A speed, mm/s: `i32` for NAV-PVT's velocities and ground speed, `f64`
/// for a pseudorange correction's rate before it is rounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct MillimetersPerSecond<T = i32>(T);

impl<T: Copy> MillimetersPerSecond<T> {
    pub(crate) const fn new(v: T) -> Self {
        Self(v)
    }

    pub(crate) const fn get(self) -> T {
        self.0
    }
}

impl MillimetersPerSecond {
    /// In cm/s, truncated.
    pub(crate) const fn centimeters_per_second(self) -> CentimetersPerSecond<i32> {
        CentimetersPerSecond(self.0 / MM_PER_CM)
    }

    /// In 0.1 m/s, truncated.
    pub(crate) const fn decimeters_per_second(self) -> DecimetersPerSecond<i32> {
        DecimetersPerSecond(self.0 / MM_PER_DM)
    }
}

impl MillimetersPerSecond<f64> {
    /// To the nearest, halves away from zero.
    pub(crate) fn round(self) -> Self {
        Self(round(self.0))
    }
}

impl Neg for MillimetersPerSecond {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

/// An angle, the module's 1e-7 degree: NAV-PVT's latitude and longitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Degrees1e7(i32);

impl Degrees1e7 {
    pub(crate) const fn new(v: i32) -> Self {
        Self(v)
    }

    pub const fn get(self) -> i32 {
        self.0
    }

    pub fn degrees(self) -> Degrees<f64> {
        Degrees(f64::from(self.0) / DEGREES_1E7_PER_DEGREE as f64)
    }
}

/// An angle, 1e-5 degree: NAV-PVT's heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Degrees1e5(i32);

impl Degrees1e5 {
    pub(crate) const fn new(v: i32) -> Self {
        Self(v)
    }

    /// In tenths of a degree, truncated.
    pub(crate) const fn decidegrees(self) -> Decidegrees<i32> {
        Decidegrees(self.0 / DEGREES_1E5_PER_TENTH)
    }
}

/// An angle, 0.1 degree: `@@Ba`'s heading, `u16`, and `@@Bk`'s magnetic
/// variation, `i16`; `f64` before it is rounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Decidegrees<T = i16>(T);

impl<T: Copy> Decidegrees<T> {
    pub const fn new(v: T) -> Self {
        Self(v)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Decidegrees<f64> {
    /// To the nearest, halves away from zero, held within `limit` either
    /// side of zero.
    pub(crate) fn round_within(self, limit: Self) -> Decidegrees {
        Decidegrees(round(self.0).clamp(-limit.0, limit.0) as i16)
    }
}

/// An angle, degrees: `u8` for the mask angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Degrees<T = i16>(T);

impl<T: Copy> Degrees<T> {
    pub const fn new(degrees: T) -> Self {
        Self(degrees)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl<T: Copy + Into<i32>> Degrees<T> {
    pub fn to_i32(self) -> Degrees<i32> {
        Degrees(self.0.into())
    }
}

impl<T: Add<Output = T>> Add for Degrees<T> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

/// An angle times a plain number.
impl<T: Mul<Output = T>> Mul<T> for Degrees<T> {
    type Output = Self;

    fn mul(self, k: T) -> Self {
        Self(self.0 * k)
    }
}

/// An angle an `i16` holds.
impl TryFrom<Degrees<i32>> for Degrees<i16> {
    type Error = TryFromIntError;

    fn try_from(degrees: Degrees<i32>) -> Result<Self, TryFromIntError> {
        i16::try_from(degrees.0).map(Self)
    }
}

/// An angle's `rem_euclid`: from 0 up to `circle`.
macro_rules! within_circle {
    ($t:ty) => {
        impl $t {
            pub(crate) const fn rem_euclid(self, circle: Self) -> Self {
                Self(self.0.rem_euclid(circle.0))
            }
        }
    };
}

within_circle!(Degrees<i16>);
within_circle!(Degrees<i32>);
within_circle!(Decidegrees<i32>);

impl Degrees<i32> {
    /// `self` and `other`, at compile time.
    pub(crate) const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }
}

impl Sub for Degrees<i32> {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl<T: Copy + Into<f64>> Degrees<T> {
    pub fn radians(self) -> Radians {
        Radians(self.0.into().to_radians())
    }

    pub fn sin(self) -> f64 {
        self.radians().sin()
    }

    pub fn cos(self) -> f64 {
        self.radians().cos()
    }
}

impl Degrees<f64> {
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// From 0 up to `circle`, as `f64::rem_euclid`.
    pub fn rem_euclid(self, circle: Self) -> Self {
        let r = self.0 % circle.0;
        Self(if r < 0.0 { r + circle.0.abs() } else { r })
    }

    /// To the nearest, halves away from zero, held to an `i8`.
    pub fn round_i8(self) -> Degrees<i8> {
        Degrees(round(self.0) as i8)
    }

    /// To the nearest, halves away from zero, held to an `i16`.
    pub fn round_i16(self) -> Degrees<i16> {
        Degrees(round(self.0) as i16)
    }

    /// In tenths of a degree, not rounded.
    pub(crate) fn decidegrees(self) -> Decidegrees<f64> {
        Decidegrees(self.0 * f64::from(TENTHS_PER_UNIT))
    }
}

/// An angle, radians.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Radians(f64);

/// An angular rate, rad/s.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct RadiansPerSecond(f64);

impl RadiansPerSecond {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }

    /// The angle turned in `t`.
    pub(crate) fn angle(self, t: Seconds) -> Radians {
        Radians(self.0 * t.0)
    }
}

impl Radians {
    pub const fn new(radians: f64) -> Self {
        Self(radians)
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    pub fn sin(self) -> f64 {
        sin(self.0)
    }

    pub fn cos(self) -> f64 {
        cos(self.0)
    }

    pub fn degrees(self) -> Degrees<f64> {
        Degrees(self.0.to_degrees())
    }
}

/// A time or an interval, s: `f64` for a GPS time of week and the
/// intervals from one, `i64` for the intervals between GPS times, `i8`
/// for the leap seconds, `u8` for `@@AD`'s thresholds, the output
/// periods, CFG-NAV5's DGNSS timeout and the seconds counted without a
/// message or a correction set, `u16` for `@@Bk`'s correction age,
/// `u32` for the engine's seconds since start.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Seconds<T = f64>(T);

impl<T: Copy> Seconds<T> {
    pub const fn new(seconds: T) -> Self {
        Self(seconds)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Eq for Seconds<i64> {}
impl Eq for Seconds<i8> {}
impl Eq for Seconds<u8> {}
impl Eq for Seconds<u16> {}

impl Seconds {
    pub(crate) fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// The range light travels in it.
    pub(crate) fn range(self) -> Meters {
        Meters(SPEED_OF_LIGHT * self.0)
    }

    /// `self` less `reference`, across a week's end: the nearer of the
    /// two ways round (IS-GPS-200 Table 20-IV, tk).
    pub(crate) fn since(self, reference: Self) -> Self {
        let dt = self.0 - reference.0;
        Self(if dt > HALF_WEEK_S {
            dt - WEEK_S
        } else if dt < -HALF_WEEK_S {
            dt + WEEK_S
        } else {
            dt
        })
    }

    /// The time of week it is, 0.1 s, to the nearest.
    pub(crate) fn tenths_of_week(self) -> Deciseconds {
        Deciseconds(round(self.0 * f64::from(TENTHS_PER_SECOND)) as u32 % TENTHS_PER_WEEK)
    }

    /// A time of week up to a week either side of this one, in it.
    pub(crate) fn within_week(self) -> Self {
        Self(if self.0 < 0.0 {
            self.0 + WEEK_S
        } else if self.0 >= WEEK_S {
            self.0 - WEEK_S
        } else {
            self.0
        })
    }
}

impl Seconds<i64> {
    /// As `f64`, as a time of week is.
    pub(crate) const fn to_f64(self) -> Seconds {
        Seconds(self.0 as f64)
    }
}

impl Seconds<u8> {
    /// A second more, held at the largest count.
    pub fn saturating_increment(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl Add for Seconds<i8> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl From<Seconds<i8>> for Seconds<i64> {
    fn from(seconds: Seconds<i8>) -> Self {
        Self(seconds.0.into())
    }
}

impl Sub for Seconds {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// A time, 0.1 s: `@@Ce`'s time of week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Deciseconds(u32);

impl Deciseconds {
    pub(crate) const fn get(self) -> u32 {
        self.0
    }
}

/// A serial line's rate, baud: the module's UART and the instrument's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Baud(u32);

impl Baud {
    pub const fn new(baud: u32) -> Self {
        Self(baud)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A time, us: CFG-TP5's pulse period and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Microseconds(u32);

impl Microseconds {
    pub(crate) const fn new(us: u32) -> Self {
        Self(us)
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }
}

/// A time, ms: `u16` for `@@Ax`'s epoch offset, `u32` for a time of
/// week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Milliseconds<T = u16>(T);

impl Milliseconds<u32> {
    pub(crate) const fn nanoseconds(self) -> Nanoseconds<i64> {
        Nanoseconds(self.0 as i64 * NS_PER_MS)
    }
}

/// A setting's wire value, two bytes.
impl From<u16> for Milliseconds {
    fn from(ms: u16) -> Self {
        Self(ms)
    }
}

impl From<Milliseconds> for u16 {
    fn from(ms: Milliseconds) -> Self {
        ms.0
    }
}

/// Time RAIM's alarm limit, in its unit of 100 ns (`@@Bn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct HundredNanoseconds(u16);

impl HundredNanoseconds {
    pub const fn new(v: u16) -> Self {
        Self(v)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub(crate) const fn nanoseconds(self) -> Nanoseconds {
        Nanoseconds(self.0 as i32 * NS_PER_HUNDRED_NS)
    }
}

/// A carrier-to-noise density ratio, dB-Hz: NAV-SAT's C/N0 and CFG-NAV5's
/// threshold for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct DecibelHertz(u8);

impl DecibelHertz {
    pub(crate) const fn new(v: u8) -> Self {
        Self(v)
    }

    pub(crate) const fn get(self) -> u8 {
        self.0
    }
}

/// A quantity in tenths: a DOP, its threshold or hysteresis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Tenths(u16);

impl Tenths {
    pub(crate) const fn new(v: u16) -> Self {
        Self(v)
    }

    pub(crate) const fn get(self) -> u16 {
        self.0
    }

    /// In units.
    pub(crate) fn value(self) -> f32 {
        f32::from(self.0) / f32::from(TENTHS_PER_UNIT)
    }
}

/// A setting's wire value, two bytes.
impl From<u16> for Tenths {
    fn from(v: u16) -> Self {
        Self(v)
    }
}

impl From<Tenths> for u16 {
    fn from(v: Tenths) -> Self {
        v.0
    }
}

/// A range or a distance, m: `f64` for a pseudorange, a correction, `u16`
/// for CFG-NAV5's accuracy masks and static hold distance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Meters<T = f64>(T);

impl<T: Copy> Meters<T> {
    pub const fn new(m: T) -> Self {
        Self(m)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Eq for Meters<u16> {}

impl Meters {
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// In cycles of the L1 carrier.
    pub fn l1_cycles(self) -> f64 {
        self.0 / L1_WAVELENGTH
    }

    /// The time light takes to travel it.
    pub(crate) fn light_time(self) -> Seconds {
        Seconds(self.0 / SPEED_OF_LIGHT)
    }

    /// To the nearest whole `step`, halves away from zero.
    pub(crate) fn round_to(self, step: Self) -> Self {
        Self(round(self.0 / step.0) * step.0)
    }
    /// In cm, not rounded.
    pub fn centimeters(self) -> Centimeters<f64> {
        Centimeters(self.0 * CM_PER_M)
    }
}

/// As their values' `f64` sum.
impl Sum for Meters {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self(iter.map(|m| m.0).sum())
    }
}

/// An area, m^2: a squared residual and their sum.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct SquareMeters(f64);

impl SquareMeters {
    /// Its square root, libm's.
    pub fn sqrt(self) -> Meters {
        Meters(sqrt(self.0))
    }
}

/// As their values' `f64` sum.
impl Sum for SquareMeters {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self(iter.map(|m| m.0).sum())
    }
}

impl Add for Meters {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Meters {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// A frequency, Hz: `f32` for RXM-RAWX's Doppler and a drift's, `f64`
/// for the almanac's, `i16` for a Doppler as the Oncore reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Hertz<T = f32>(T);

impl<T: Copy> Hertz<T> {
    pub const fn new(hz: T) -> Self {
        Self(hz)
    }

    pub const fn get(self) -> T {
        self.0
    }
}

impl Neg for Hertz<i16> {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}
impl Hertz<f64> {
    /// Held to an `i16`, the fraction dropped.
    pub(crate) fn truncate(self) -> Hertz<i16> {
        Hertz(self.0.clamp(i16::MIN.into(), i16::MAX.into()) as i16)
    }

    /// To the nearest, halves away from zero, held to an `i16`.
    pub(crate) fn round(self) -> Hertz<i16> {
        Self(round(self.0)).truncate()
    }
}

impl Hertz {
    /// Held to an `i16`, the fraction dropped.
    pub(crate) fn truncate(self) -> Hertz<i16> {
        Hertz::new(f64::from(self.0)).truncate()
    }

    /// To the nearest, halves away from zero, held to an `i16`: a half
    /// added in `f32`, which is not quite `roundf` just below a half.
    pub(crate) fn round(self) -> Hertz<i16> {
        Hertz::new(f64::from(if self.0 < 0.0 { self.0 - HALF } else { plus_half(self.0) })).truncate()
    }
}

impl Hertz {
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// The range rate whose L1 Doppler it is: positive receding.
    pub(crate) fn range_rate(self) -> MetersPerSecond {
        MetersPerSecond(-L1_WAVELENGTH * f64::from(self.0))
    }
}

impl Hertz<f64> {
    /// As `f32`, as RXM-RAWX's Doppler is.
    pub fn to_f32(self) -> Hertz {
        Hertz(self.0 as f32)
    }
}

impl From<Hertz> for Hertz<f64> {
    fn from(hz: Hertz) -> Self {
        Self(hz.0.into())
    }
}

impl Add for Hertz<f64> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl From<Hertz<i16>> for Hertz {
    fn from(hz: Hertz<i16>) -> Self {
        Self(f32::from(hz.0))
    }
}

impl Add for Hertz {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Hertz {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// An ECEF position, or the offset between two, m: x, y and z.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ecef([f64; AXES]);

impl Ecef {
    pub const fn new(m: [f64; AXES]) -> Self {
        Self(m)
    }

    /// x, y and z, m.
    pub const fn get(self) -> [f64; AXES] {
        self.0
    }

    /// From x, y and z.
    pub(crate) fn from_axes(axes: [Meters; AXES]) -> Self {
        Self(axes.map(|m| m.0))
    }

    /// From x, y and z, cm, as NAV-POSECEF gives them.
    pub fn from_centimeters(cm: [Centimeters; AXES]) -> Self {
        Self(cm.map(|c| c.meters().get()))
    }

    /// The length.
    pub(crate) fn norm(self) -> Meters {
        Meters(sqrt(dot(self.0, self.0)))
    }

    /// The unit vector along it.
    pub(crate) fn direction(self) -> Direction {
        let length = self.norm().get();
        Direction::new(self.0.map(|v| v / length))
    }

    /// Turned about the z axis by `angle`, as the Earth turns
    /// during a signal's transit: from the earth-fixed frame at
    /// transmission into the one at reception (IS-GPS-200,
    /// 20.3.3.4.3.3.2).
    pub(crate) fn earth_rotated(self, angle: Radians) -> Self {
        Self(earth_rotation(self.0, angle))
    }

    /// At geodetic latitude `phi` and longitude `lambda`, and `height`
    /// above the WGS-84 ellipsoid.
    pub fn from_geodetic(phi: Radians, lambda: Radians, height: Meters) -> Self {
        let (p, z) = meridian(Meters::new(WGS84_SEMI_MAJOR_M), phi, height);
        Self::from_axes([p * lambda.cos(), p * lambda.sin(), z])
    }
}

impl Sub for Ecef {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self([self.0[0] - other.0[0], self.0[1] - other.0[1], self.0[2] - other.0[2]])
    }
}

/// A velocity in ECEF axes, m/s: x, y and z.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EcefVelocity([f64; AXES]);

impl EcefVelocity {
    /// From x, y and z.
    pub(crate) fn from_axes(axes: [MetersPerSecond; AXES]) -> Self {
        Self(axes.map(|v| v.0))
    }

    /// x, y and z, m/s.
    #[cfg(test)]
    pub(crate) const fn get(self) -> [f64; AXES] {
        self.0
    }

    /// The speed.
    #[cfg(test)]
    pub(crate) fn norm(self) -> MetersPerSecond {
        MetersPerSecond(sqrt(dot(self.0, self.0)))
    }

    /// The component along unit vector `d`.
    pub(crate) fn toward(self, d: Direction) -> MetersPerSecond {
        MetersPerSecond(dot(self.0, d.get()))
    }

    /// The component along `offset`.
    pub(crate) fn along(self, offset: Ecef) -> MetersPerSecond {
        MetersPerSecond(dot(self.0, offset.0) / offset.norm().get())
    }

    /// Turned about the z axis as [`Ecef::earth_rotated`].
    pub(crate) fn earth_rotated(self, angle: Radians) -> Self {
        Self(earth_rotation(self.0, angle))
    }
}

/// `v` turned about the z axis by `angle`.
fn earth_rotation(v: [f64; AXES], angle: Radians) -> [f64; AXES] {
    let (s, c) = (angle.sin(), angle.cos());
    [c * v[0] + s * v[1], -s * v[0] + c * v[1], v[2]]
}

/// A length in `f64`, in its unit: m for ECEF, km for the World Magnetic
/// Model.
pub(crate) trait Length:
    Copy + Add<Output = Self> + Mul<f64, Output = Self> + Div<f64, Output = Self>
{
}

impl Length for Meters {}

impl Length for Kilometers {}

/// The distance from the axis and from the equatorial plane of geodetic
/// latitude `phi`, `height` above the WGS-84 ellipsoid, whose semi-major
/// axis is `semi_major`, in their unit.
pub(crate) fn meridian<L: Length>(semi_major: L, phi: Radians, height: L) -> (L, L) {
    let (sin_phi, cos_phi) = (phi.sin(), phi.cos());
    let n = semi_major / sqrt(1.0 - WGS84_E2 * sin_phi * sin_phi);
    ((n + height) * cos_phi, (n * (1.0 - WGS84_E2) + height) * sin_phi)
}

/// A clock's drift, s/s.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct SecondsPerSecond(f64);

impl SecondsPerSecond {
    pub(crate) const fn new(drift: f64) -> Self {
        Self(drift)
    }

    /// The range rate it adds, at the speed of light.
    pub(crate) fn range_rate(self) -> MetersPerSecond {
        MetersPerSecond(SPEED_OF_LIGHT * self.0)
    }
}

/// A length of time, years: from the World Magnetic Model's epoch.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct Years(f64);

/// The interval between two dates.
impl Sub for DecimalYear {
    type Output = Years;

    fn sub(self, other: Self) -> Years {
        Years(self.get() - other.get())
    }
}

/// A magnetic field's component's secular variation, nT/year.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct NanoteslaPerYear(f64);

impl NanoteslaPerYear {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }
}

/// A length's square root, m^1/2: an orbit's sqrt(A).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct RootMeters(f64);

impl RootMeters {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }

    #[cfg(test)]
    pub(crate) const fn get(self) -> f64 {
        self.0
    }
}

/// s/m^1/2: the relativistic clock term's constant.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct SecondsPerRootMeter(f64);

impl SecondsPerRootMeter {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }
}

/// A gravitational parameter, m^3/s^2: the earth's.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct CubicMetersPerSecondSquared(f64);

impl CubicMetersPerSecondSquared {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }

    /// The mean motion of an orbit with semi-major axis `a`:
    /// sqrt(mu / a^3).
    pub(crate) fn mean_motion(self, a: Meters) -> RadiansPerSecond {
        RadiansPerSecond(sqrt(self.0 / (a.0 * a.0 * a.0)))
    }
}

/// A magnetic field's component, nT.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct Nanotesla(f64);

impl Nanotesla {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }

    #[cfg(test)]
    pub(crate) fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// The direction of the field whose components across and along a
    /// reference direction are `self` and `along`: their `atan2`.
    pub(crate) fn direction(self, along: Self) -> Radians {
        Radians(atan2(self.0, along.0))
    }
}

/// A clock's drift rate, s/s^2.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) struct SecondsPerSecondSquared(f64);

impl SecondsPerSecondSquared {
    pub(crate) const fn new(v: f64) -> Self {
        Self(v)
    }
}

/// A speed, cm/s: `u8` for CFG-NAV5's static hold threshold, `i32` for
/// a ground speed, as `@@Ba` reports it, and `u16` its field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct CentimetersPerSecond<T = u8>(T);

impl<T: Copy> CentimetersPerSecond<T> {
    pub(crate) const fn new(v: T) -> Self {
        Self(v)
    }

    pub(crate) const fn get(self) -> T {
        self.0
    }
}

/// A range rate, m/s, in `f64`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct MetersPerSecond(f64);

impl MetersPerSecond {
    pub const fn new(v: f64) -> Self {
        Self(v)
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// The order of `self` and `other`, NaN included ([`f64::total_cmp`]).
    pub fn total_cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }

    /// The L1 Doppler of this range rate, positive approaching: the
    /// inverse of [`Hertz::range_rate`].
    pub fn doppler(self) -> Hertz<f64> {
        Hertz(-self.0 / L1_WAVELENGTH)
    }

    /// In mm/s, not rounded.
    pub(crate) fn millimeters_per_second(self) -> MillimetersPerSecond<f64> {
        MillimetersPerSecond(self.0 * MM_PER_M)
    }
}

impl Add for MetersPerSecond {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for MetersPerSecond {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

/// `Add` and `Sub` of two of a unit, as of their values.
macro_rules! sum {
    ($t:ty) => {
        impl Add for $t {
            type Output = Self;

            fn add(self, other: Self) -> Self {
                Self(self.0 + other.0)
            }
        }

        impl Sub for $t {
            type Output = Self;

            fn sub(self, other: Self) -> Self {
                Self(self.0 - other.0)
            }
        }
    };
}

/// A unit times and over a plain number, either side of it.
macro_rules! scaled {
    ($t:ident) => {
        impl Mul<f64> for $t {
            type Output = Self;

            fn mul(self, k: f64) -> Self {
                Self(self.0 * k)
            }
        }

        impl Mul<$t> for f64 {
            type Output = $t;

            fn mul(self, v: $t) -> $t {
                $t(self * v.0)
            }
        }

        impl Div<f64> for $t {
            type Output = Self;

            fn div(self, k: f64) -> Self {
                Self(self.0 / k)
            }
        }
    };
}

/// `$a` times `$b` is `$out`, as their values' product, in that order.
macro_rules! product {
    ($a:ty, $b:ty, $out:ident) => {
        impl Mul<$b> for $a {
            type Output = $out;

            fn mul(self, b: $b) -> $out {
                $out(self.0 * b.0)
            }
        }
    };
}

sum!(Kilometers);
sum!(Radians);
sum!(RadiansPerSecond);
sum!(SecondsPerSecond);
sum!(Nanotesla);
scaled!(Kilometers);
scaled!(Radians);
scaled!(RadiansPerSecond);
scaled!(Meters);
scaled!(SquareMeters);
scaled!(MetersPerSecond);
scaled!(Seconds);
scaled!(SecondsPerSecondSquared);
scaled!(SecondsPerRootMeter);
scaled!(Nanotesla);
product!(Meters, Meters, SquareMeters);
product!(MetersPerSecond, Seconds, Meters);
product!(RootMeters, RootMeters, Meters);
product!(SecondsPerRootMeter, RootMeters, Seconds);
product!(Years, NanoteslaPerYear, Nanotesla);
product!(RadiansPerSecond, Radians, RadiansPerSecond);
product!(RadiansPerSecond, Meters, MetersPerSecond);
product!(Meters, RadiansPerSecond, MetersPerSecond);
product!(SecondsPerSecond, Seconds, Seconds);
product!(SecondsPerSecondSquared, Seconds, SecondsPerSecond);

impl AddAssign for Radians {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl AddAssign for Nanotesla {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl SubAssign for Nanotesla {
    fn sub_assign(&mut self, other: Self) {
        self.0 -= other.0;
    }
}

impl Add for Seconds {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Degrees<f64> {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl Neg for MetersPerSecond {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Neg for Meters {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Centimeters;
    use super::DECIMETERS_PER_US;
    use super::Decimeters;
    use super::GpsSeconds;
    use super::GpsWeek;
    use super::HALVES;
    use super::L1_HZ_PER_NS_PER_S;
    use super::MM_PER_CM;
    use super::MS_PER_SECOND;
    use super::MilliArcSeconds;
    use super::Millimeters;
    use super::NS_PER_DECIMETER;
    use super::NS_PER_US;
    use super::Nanoseconds;
    use super::PS_PER_NS;
    use super::Picoseconds;
    use super::SECONDS_PER_WEEK;
    use super::Seconds;
    use super::TimeOfWeek;
    use crate::fixtures::LATITUDE;
    use crate::fixtures::LATITUDE_1E7;
    use crate::fixtures::LONGITUDE;
    use crate::fixtures::LONGITUDE_1E7;

    /// 2026-10-27 00:00:30 GPS time: week 2442, 172830 s into it.
    const WEEK: u16 = 2442;
    const TOW_MS: u32 = 172_830_000;
    const SECONDS_OF_WEEK: Seconds<i64> = Seconds::new(172_830);

    #[test]
    fn week_and_time_of_week_make_gps_time_and_back() {
        let t =
            GpsSeconds::from_week(GpsWeek::new(WEEK), TimeOfWeek::from_ms(TOW_MS), Nanoseconds::default());
        assert_eq!(t.get(), i64::from(WEEK) * SECONDS_PER_WEEK + SECONDS_OF_WEEK.get());
        assert_eq!(
            (GpsWeek::containing(t), t.seconds_of_week()),
            (Some(GpsWeek::new(WEEK)), SECONDS_OF_WEEK)
        );
    }

    #[test]
    fn the_fraction_rounds_to_the_nearest_second() {
        const LATE_MS: u32 = 999;
        const HALF_A_MS: Nanoseconds = Nanoseconds::new(500_000);
        let week = GpsWeek::new(WEEK);
        let late = GpsSeconds::from_week(
            week,
            TimeOfWeek::from_ms(TOW_MS - MS_PER_SECOND as u32 + LATE_MS),
            HALF_A_MS,
        );
        assert_eq!(late.seconds_of_week(), SECONDS_OF_WEEK);
        let early =
            GpsSeconds::from_week(week, TimeOfWeek::from_ms(TOW_MS), Nanoseconds::new(-HALF_A_MS.get()));
        assert_eq!(early.seconds_of_week(), SECONDS_OF_WEEK);
    }

    #[test]
    fn arithmetic_in_seconds() {
        const SECOND: Seconds<i64> = Seconds::new(1);
        let t = GpsSeconds::new(SECONDS_PER_WEEK);
        assert_eq!((t + SECOND) - t, SECOND);
        assert_eq!((t - SECOND).seconds_of_week(), Seconds::new(SECONDS_PER_WEEK - 1));
        assert_eq!(TimeOfWeek::from_ms(TOW_MS).seconds(), SECONDS_OF_WEEK.to_f64());
    }

    #[test]
    fn angles_to_the_nearest_either_way() {
        assert_eq!(MilliArcSeconds::from_degrees_1e7(LATITUDE_1E7), LATITUDE);
        assert_eq!(MilliArcSeconds::from_degrees_1e7(LONGITUDE_1E7), LONGITUDE);
        assert_eq!(LATITUDE.degrees_1e7(), LATITUDE_1E7);
        assert_eq!(LONGITUDE.degrees_1e7(), LONGITUDE_1E7);
    }

    /// The range factors are the figures written before they were
    /// derived from the speed of light.
    #[test]
    fn range_factors_from_the_speed_of_light() {
        const WRITTEN_NS_PER_DECIMETER: f32 = 0.333_564;
        const WRITTEN_DECIMETERS_PER_US: i64 = 2998;
        const WRITTEN_L1_HZ_PER_NS_PER_S: f32 = 1.575_42;
        assert_eq!(NS_PER_DECIMETER.to_bits(), WRITTEN_NS_PER_DECIMETER.to_bits());
        assert_eq!(DECIMETERS_PER_US, WRITTEN_DECIMETERS_PER_US);
        assert_eq!(L1_HZ_PER_NS_PER_S.to_bits(), WRITTEN_L1_HZ_PER_NS_PER_S.to_bits());
    }

    /// A ns.
    const ONE_NS: Nanoseconds = Nanoseconds::new(1);

    /// ps to ns rounds halves away from zero.
    #[test]
    fn picoseconds_round_to_the_nearest_ns() {
        const HALF_NS_PS: i32 = PS_PER_NS / HALVES as i32;
        assert_eq!(Picoseconds::new(-HALF_NS_PS).round_ns(), Nanoseconds::new(-ONE_NS.get()));
        assert_eq!(Picoseconds::new(HALF_NS_PS - 1).round_ns(), Nanoseconds::default());
    }

    /// Range and light travel time, either way: 3 m of range is 10 ns, and
    /// 1 us is 299.8 m.
    #[test]
    fn range_and_light_travel_time() {
        const THREE_M: Decimeters = Decimeters::new(30);
        const THREE_M_NS: Nanoseconds<f32> = Nanoseconds::new(10.007);
        const TOLERANCE: Nanoseconds<f32> = Nanoseconds::new(0.01);
        assert!((THREE_M.to_ns() - THREE_M_NS).abs() < TOLERANCE);
        assert_eq!(Nanoseconds::new(NS_PER_US as i32).decimeters(), Decimeters::new(DECIMETERS_PER_US));
    }

    #[test]
    fn a_difference_of_ns_saturates() {
        let least = Nanoseconds::new(i32::MIN);
        assert_eq!(least.saturating_sub(ONE_NS), least);
    }

    /// A height in mm to cm truncates, toward zero.
    #[test]
    fn a_height_in_mm_truncates_to_cm() {
        const HEIGHT_MM: i32 = -26_599;
        assert_eq!(Millimeters::new(HEIGHT_MM).centimeters(), Centimeters::new(HEIGHT_MM / MM_PER_CM));
    }
}
