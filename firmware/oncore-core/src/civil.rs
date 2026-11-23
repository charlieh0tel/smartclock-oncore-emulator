//! Calendar dates and times of day, as the Oncore gives them, from a
//! count of seconds.

/// Seconds from 1970-01-01 to the GPS epoch, 1980-01-06.
pub(crate) const GPS_EPOCH: i64 = 315_964_800;
/// Seconds in a minute, minutes in an hour, hours in a day, days in a
/// week; seconds in an hour and a day; months in a year.
pub const SECONDS_PER_MINUTE: i64 = 60;
pub const MINUTES_PER_HOUR: i64 = 60;
pub const HOURS_PER_DAY: i64 = 24;
pub(crate) const DAYS_PER_WEEK: i64 = 7;
pub(crate) const SECONDS_PER_HOUR: i64 = MINUTES_PER_HOUR * SECONDS_PER_MINUTE;
pub(crate) const SECONDS_PER_DAY: i64 = HOURS_PER_DAY * SECONDS_PER_HOUR;
pub const MONTHS_PER_YEAR: u8 = 12;

/// January, February and March.
pub(crate) const JANUARY: u8 = 1;
/// A month's first day.
const FIRST_DAY: u8 = 1;
const FEBRUARY: u8 = 2;
const MARCH: i64 = 3;
/// The Gregorian leap year rule: every fourth year, but not every
/// hundredth, but every four hundredth.
const LEAP_EVERY: u16 = 4;
const NO_LEAP_EVERY: u16 = 100;
const LEAP_AGAIN_EVERY: u16 = 400;

/// Howard Hinnant's `days_from_civil` and `civil_from_days` count years
/// from March, so a leap day ends a year, in eras of the calendar's
/// 400-year cycle: years and days in an era; days in a common year, in
/// four years and in a century; the days from 0000-03-01 to 1970-01-01;
/// and the months from March, 153 days in each five of them (31, 30, 31,
/// 30, 31), with the rounding that places a month's first day, and the
/// months from March to December.
const YEARS_PER_ERA: i64 = LEAP_AGAIN_EVERY as i64;
const DAYS_PER_ERA: i64 = 146_097;
const DAYS_PER_YEAR: i64 = 365;
const DAYS_PER_4_YEARS: i64 = LEAP_EVERY as i64 * DAYS_PER_YEAR;
const DAYS_PER_CENTURY: i64 =
    NO_LEAP_EVERY as i64 * DAYS_PER_YEAR + NO_LEAP_EVERY as i64 / LEAP_EVERY as i64 - 1;
const DAYS_TO_UNIX_EPOCH: i64 = 719_468;
const FIVE_MONTHS_DAYS: i64 = 153;
const FIVE_MONTHS: i64 = 5;
const MONTH_START_ROUNDING: i64 = 2;
const MARCH_TO_DECEMBER: i64 = MONTHS_PER_YEAR as i64 - MARCH + 1;

/// A Unix time: seconds from 1970-01-01 00:00:00, with no leap seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixSeconds(i64);

impl UnixSeconds {
    pub(crate) const fn new(seconds: i64) -> Self {
        Self(seconds)
    }

    pub(crate) const fn get(self) -> i64 {
        self.0
    }

    /// The start of day `days`.
    pub(crate) const fn from_days(days: Days) -> Self {
        Self(days.0 * SECONDS_PER_DAY)
    }

    /// The day it falls in.
    pub(crate) const fn day(self) -> Days {
        Days(self.0.div_euclid(SECONDS_PER_DAY))
    }

    /// Its year with the fraction of it gone, as the World Magnetic Model
    /// takes a date.
    pub(crate) fn decimal_year(self) -> DecimalYear {
        let year = i64::from(DateTime::from_unix(self).year);
        let jan1 = days_from_civil(year, JANUARY, FIRST_DAY).0;
        let next = days_from_civil(year + 1, JANUARY, FIRST_DAY).0;
        DecimalYear(
            year as f64 + (self.0 as f64 / SECONDS_PER_DAY as f64 - jan1 as f64) / (next - jan1) as f64,
        )
    }
}

/// A date in years, the fraction of its year gone: the World Magnetic
/// Model's.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) struct DecimalYear(f64);

impl DecimalYear {
    pub(crate) const fn new(year: f64) -> Self {
        Self(year)
    }

    pub(crate) const fn get(self) -> f64 {
        self.0
    }
}

/// A count of days from 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Days(i64);

impl Days {
    pub(crate) const fn new(days: i64) -> Self {
        Self(days)
    }

    pub(crate) const fn get(self) -> i64 {
        self.0
    }
}

/// A date and time of day: what `@@Aa` and `@@Ac` carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Days from 1970-01-01 to a date (Howard Hinnant's `days_from_civil`).
pub(crate) fn days_from_civil(year: i64, month: u8, day: u8) -> Days {
    let (leap_every, no_leap_every) = (i64::from(LEAP_EVERY), i64::from(NO_LEAP_EVERY));
    let y = if month <= FEBRUARY { year - 1 } else { year };
    let era = y.div_euclid(YEARS_PER_ERA);
    let yoe = y - era * YEARS_PER_ERA;
    let m = i64::from(month);
    let from_march = if m >= MARCH { m - MARCH } else { m + MARCH_TO_DECEMBER - 1 };
    let doy = (FIVE_MONTHS_DAYS * from_march + MONTH_START_ROUNDING) / FIVE_MONTHS + i64::from(day) - 1;
    let doe = yoe * DAYS_PER_YEAR + yoe / leap_every - yoe / no_leap_every + doy;
    Days::new(era * DAYS_PER_ERA + doe - DAYS_TO_UNIX_EPOCH)
}

/// The date of a count of days from 1970-01-01.
pub(crate) fn civil_from_days(days: Days) -> (i64, u8, u8) {
    let (leap_every, no_leap_every) = (i64::from(LEAP_EVERY), i64::from(NO_LEAP_EVERY));
    let z = days.get() + DAYS_TO_UNIX_EPOCH;
    let era = z.div_euclid(DAYS_PER_ERA);
    let doe = z - era * DAYS_PER_ERA;
    let yoe =
        (doe - doe / DAYS_PER_4_YEARS + doe / DAYS_PER_CENTURY - doe / (DAYS_PER_ERA - 1)) / DAYS_PER_YEAR;
    let doy = doe - (DAYS_PER_YEAR * yoe + yoe / leap_every - yoe / no_leap_every);
    let mp = (FIVE_MONTHS * doy + MONTH_START_ROUNDING) / FIVE_MONTHS_DAYS;
    let day = (doy - (FIVE_MONTHS_DAYS * mp + MONTH_START_ROUNDING) / FIVE_MONTHS + 1) as u8;
    let month = if mp < MARCH_TO_DECEMBER { mp + MARCH } else { mp - (MARCH_TO_DECEMBER - 1) } as u8;
    let year = yoe + era * YEARS_PER_ERA + i64::from(month <= FEBRUARY);
    (year, month, day)
}

impl DateTime {
    /// The date and time of a Unix time.
    pub(crate) fn from_unix(t: UnixSeconds) -> Self {
        let (year, month, day) = civil_from_days(t.day());
        // Seconds into the day.
        let s = t.0.rem_euclid(SECONDS_PER_DAY);
        Self {
            year: year as u16,
            month,
            day,
            hour: (s / SECONDS_PER_HOUR) as u8,
            minute: (s % SECONDS_PER_HOUR / SECONDS_PER_MINUTE) as u8,
            second: (s % SECONDS_PER_MINUTE) as u8,
        }
    }

    /// As a Unix time.
    #[cfg(test)]
    pub(crate) fn to_unix(self) -> UnixSeconds {
        let day = UnixSeconds::from_days(days_from_civil(i64::from(self.year), self.month, self.day));
        UnixSeconds(
            day.0
                + i64::from(self.hour) * SECONDS_PER_HOUR
                + i64::from(self.minute) * SECONDS_PER_MINUTE
                + i64::from(self.second),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::DateTime;
    use super::FEBRUARY;
    use super::GPS_EPOCH;
    use super::SECONDS_PER_DAY;
    use super::SECONDS_PER_HOUR;
    use super::SECONDS_PER_MINUTE;
    use super::UnixSeconds;

    #[test]
    fn gps_epoch() {
        /// The GPS epoch, 1980-01-06 00:00 (IS-GPS-200).
        const EPOCH: (u16, u8, u8, u8) = (1980, 1, 6, 0);
        let t = DateTime::from_unix(UnixSeconds::new(GPS_EPOCH));
        assert_eq!((t.year, t.month, t.day, t.hour), EPOCH);
        assert_eq!(t.to_unix(), UnixSeconds::new(GPS_EPOCH));
    }

    /// 2000-02-29, in days from 1970-01-01.
    const LEAP_DAY: i64 = 11_016;

    #[test]
    fn round_trip_across_leap_years() {
        // 01:02:03 into each day.
        const CLOCK: (u8, u8, u8) = (1, 2, 3);
        const TIME_OF_DAY: i64 =
            CLOCK.0 as i64 * SECONDS_PER_HOUR + CLOCK.1 as i64 * SECONDS_PER_MINUTE + CLOCK.2 as i64;
        /// Days from 1970-01-01: the day before, 1970-03-01 and the day
        /// before it, 1971-01-01, 2000-01-01, the leap day, and two far on.
        const DAYS: [i64; 9] = [-1, 0, 59, 60, 365, 10_957, LEAP_DAY, 20_000, 40_000];
        for days in DAYS {
            let unix = UnixSeconds::new(days * SECONDS_PER_DAY + TIME_OF_DAY);
            let t = DateTime::from_unix(unix);
            assert_eq!(t.to_unix(), unix);
            assert_eq!((t.hour, t.minute, t.second), CLOCK);
        }
    }

    /// 2000, a leap year by the four-hundredth rule.
    #[test]
    fn a_leap_year_by_the_gregorian_rules() {
        /// A leap year's February's days.
        const LEAP_FEBRUARY_DAYS: u8 = 29;
        let leap_day = DateTime::from_unix(UnixSeconds::new(LEAP_DAY * SECONDS_PER_DAY));
        assert_eq!((leap_day.month, leap_day.day), (FEBRUARY, LEAP_FEBRUARY_DAYS));
    }
}
