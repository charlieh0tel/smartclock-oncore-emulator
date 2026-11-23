//! What the unit tests share: the instruments' default position, which
//! the trace was moved to (`testdata/README.md`), in the Oncore's units
//! and the module's.

use core::ops::Range;

use crate::sky::Prn;
use crate::units::Centimeters;
use crate::units::Degrees1e7;
use crate::units::MilliArcSeconds;
use crate::units::Millimeters;
use crate::units::Seconds;
use crate::units::Tenths;

/// 37.32558333 N, 121.99783333 W, and in the module's 1e-7 degree.
pub(crate) const LATITUDE: MilliArcSeconds = MilliArcSeconds::new(134_372_100);
pub(crate) const LONGITUDE: MilliArcSeconds = MilliArcSeconds::new(-439_192_200);
pub(crate) const LATITUDE_1E7: Degrees1e7 = Degrees1e7::new(373_255_833);
pub(crate) const LONGITUDE_1E7: Degrees1e7 = Degrees1e7::new(-1_219_978_333);
/// 55 m above mean sea level, the geoid 28.41 m below the ellipsoid
/// there, and the height above the ellipsoid; in the module's mm.
pub(crate) const HEIGHT_MSL: Centimeters = Centimeters::new(5500);
pub(crate) const SEPARATION: Centimeters = Centimeters::new(-2841);
pub(crate) const HEIGHT_GPS: Centimeters = HEIGHT_MSL.plus(SEPARATION);
pub(crate) const HEIGHT_MSL_MM: Millimeters = HEIGHT_MSL.checked_millimeters().unwrap();
pub(crate) const HEIGHT_GPS_MM: Millimeters = HEIGHT_GPS.checked_millimeters().unwrap();
/// GPS PRN `number`, which must be one.
pub(crate) const fn prn(number: u8) -> Prn {
    Prn::new(number).unwrap()
}

/// GPS less UTC since 2017.
pub(crate) const LEAP_SECONDS: Seconds<i8> = Seconds::new(18);
/// NAV-PVT's numSV, the satellites a fix used.
pub(crate) const PVT_SATELLITES: u8 = 9;
/// The DOPs that a sky of satellites spread like the tests' gives.
pub(crate) const SPREAD_DOPS: Range<Tenths> = Tenths::new(10)..Tenths::new(100);
