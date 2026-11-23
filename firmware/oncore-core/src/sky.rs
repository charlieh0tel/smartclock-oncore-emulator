//! The GPS satellites as the module sees them, by PRN, from NAV-SAT and
//! RXM-RAWX (UBX-13003221 R28, pp. 381, 409).  The Oncore is GPS only, so
//! other systems are left out here; the module still uses them.

use core::fmt;
use core::ops::RangeInclusive;

use crate::units::DEGREES_PER_QUARTER_CIRCLE;
use crate::units::DecibelHertz;
use crate::units::Decimeters;
use crate::units::Degrees;
use crate::units::Hertz;
use smartclock_gps::record::Health;
use smartclock_gps_ublox::record::satellite_health;
use smartclock_ubx::message::SYSTEM_GPS;
use smartclock_ubx::nav::OrbitSource;
use smartclock_ubx::nav::SatelliteInfo;

/// GPS PRNs 1 to 32.
pub const PRNS: usize = 32;

/// A GPS satellite's PRN, 1 to [`PRNS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prn(u8);

impl Prn {
    /// `prn`, if it is a GPS PRN.
    pub const fn new(prn: u8) -> Option<Self> {
        if prn >= 1 && prn as usize <= PRNS { Some(Self(prn)) } else { None }
    }

    /// The PRN at `index` in a table by PRN, below [`PRNS`].
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index as u8 + 1)
    }

    /// The PRN's number, as the wire carries it.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Its place in a table by PRN: PRN - 1.
    pub const fn index(self) -> usize {
        self.0 as usize - 1
    }

    /// Its bit in a [`PrnSet`]: bit PRN - 1.
    const fn bit(self) -> u32 {
        1 << self.index()
    }

    /// Every PRN, in order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..PRNS).map(Self::from_index)
    }
}

impl fmt::Display for Prn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A set of PRNs, bit PRN - 1 for each, as `@@Am` and `@@Bd` carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrnSet(u32);

impl PrnSet {
    /// No PRN.
    pub(crate) const EMPTY: Self = Self(0);

    /// The set whose bits are `bits`, as the wire carries it.
    pub(crate) const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Its bits, as the wire carries it.
    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, prn: Prn) -> bool {
        self.0 & prn.bit() != 0
    }

    pub(crate) const fn insert(&mut self, prn: Prn) {
        self.0 |= prn.bit();
    }
}

impl FromIterator<Prn> for PrnSet {
    fn from_iter<I: IntoIterator<Item = Prn>>(prns: I) -> Self {
        let mut set = Self::EMPTY;
        for prn in prns {
            set.insert(prn);
        }
        set
    }
}

/// NAV-SAT's signal qualities, qualityInd: none, searching, acquired,
/// unusable, code locked, and code and carrier locked from 5.
pub(crate) const NO_SIGNAL: u8 = 0;
pub(crate) const SEARCHING: u8 = 1;
pub(crate) const ACQUIRED: u8 = 2;
pub(crate) const UNUSABLE: u8 = 3;
#[cfg(test)]
pub(crate) const CODE_LOCKED: u8 = 4;
/// The best quality, code and carrier locked, as the tests' satellites
/// have it.
#[cfg(test)]
pub(crate) const LOCKED: u8 = 7;
/// The elevations a satellite may have, degrees.
const ELEVATIONS: RangeInclusive<Degrees<i8>> =
    Degrees::new(-(DEGREES_PER_QUARTER_CIRCLE as i8))..=Degrees::new(DEGREES_PER_QUARTER_CIRCLE as i8);

/// The Oncore's channel tracking modes (VP reference p. 72): code
/// search, code acquire, AGC set, ephemeris acquire, and available for
/// position.
pub(crate) const MODE_CODE_SEARCH: u8 = 0;
pub(crate) const MODE_CODE_ACQUIRE: u8 = 1;
pub(crate) const MODE_AGC_SET: u8 = 2;
const MODE_EPHEMERIS_ACQUIRE: u8 = 7;
pub const MODE_POSITION: u8 = 8;

/// One satellite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Satellite {
    /// In the module's last NAV-SAT.
    pub(crate) present: bool,
    pub(crate) cno: DecibelHertz,
    /// Outside -90 to 90 degrees when unknown.
    pub elevation: Degrees<i8>,
    /// 0 to 360 degrees.
    pub azimuth: Degrees<i16>,
    /// Pseudorange residual.
    pub(crate) residual: Decimeters,
    /// NAV-SAT's qualityInd, 0 to 7.
    pub(crate) quality: u8,
    /// svUsed: in the module's fix; health, as NAV-SAT gives it.
    pub(crate) used: bool,
    pub(crate) health: Health,
    /// ephAvail: the module holds its ephemeris.
    pub(crate) ephemeris: bool,
    /// The module holds an orbit of some kind for it: an orbitSource,
    /// almAvail or ephAvail.
    pub(crate) orbit: bool,
    /// Hz, positive approaching; measured, if `measured`.
    pub doppler: Hertz<i16>,
    pub measured: bool,
}

impl Satellite {
    /// Not in view.
    pub(crate) const fn new() -> Self {
        Self {
            present: false,
            cno: DecibelHertz::new(0),
            elevation: Degrees::new(0),
            azimuth: Degrees::new(0),
            residual: Decimeters::new(0),
            quality: NO_SIGNAL,
            used: false,
            health: Health::Unknown,
            ephemeris: false,
            orbit: false,
            doppler: Hertz::new(0),
            measured: false,
        }
    }
}

impl Default for Satellite {
    fn default() -> Self {
        Self::new()
    }
}

impl Satellite {
    /// A NAV-SAT block's GPS satellite: its PRN and what the module sees
    /// of it.
    pub(crate) fn from_nav_sat(info: &SatelliteInfo) -> Option<(Prn, Self)> {
        let prn = Prn::new(info.id).filter(|_| info.system == SYSTEM_GPS)?;
        let satellite = Self {
            present: true,
            cno: DecibelHertz::new(info.signal.0),
            elevation: Degrees::new(info.elevation.0),
            azimuth: Degrees::new(info.azimuth.0),
            residual: Decimeters::new(info.residual.0),
            quality: info.quality,
            used: info.used,
            health: satellite_health(info.health),
            ephemeris: info.ephemeris,
            orbit: info.orbit_source != OrbitSource::None || info.almanac || info.ephemeris,
            doppler: Hertz::new(0),
            measured: false,
        };
        Some((prn, satellite))
    }

    pub fn used(&self) -> bool {
        self.used
    }

    pub(crate) fn unhealthy(&self) -> bool {
        self.health == Health::Unhealthy
    }

    /// Available for position, mode 8.
    pub(crate) fn positioning(&self) -> bool {
        self.mode() == Some(MODE_POSITION)
    }

    /// The module knows where it is: an elevation, from some orbit.
    pub(crate) fn located(&self) -> bool {
        self.orbit && ELEVATIONS.contains(&self.elevation)
    }

    /// The Oncore's channel tracking mode (VP reference p. 72), or `None`
    /// with no signal (PLAN.md, "Satellite reports").
    pub(crate) fn mode(&self) -> Option<u8> {
        match self.quality {
            NO_SIGNAL => None,
            SEARCHING => Some(MODE_CODE_SEARCH),
            ACQUIRED => Some(MODE_CODE_ACQUIRE),
            UNUSABLE => Some(MODE_AGC_SET),
            _ if self.ephemeris => Some(MODE_POSITION),
            _ => Some(MODE_EPHEMERIS_ACQUIRE),
        }
    }
}

/// The satellites, by PRN - 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sky {
    pub(crate) satellites: [Satellite; PRNS],
}

impl Sky {
    /// Nothing in view.
    pub(crate) const fn new() -> Self {
        Self { satellites: [Satellite::new(); PRNS] }
    }

    /// Nothing in view any more, in place.
    pub(crate) fn clear(&mut self) {
        self.satellites.fill(Satellite::new());
    }
}

impl Sky {
    /// Satellite `prn`, as the Oncore numbers it (0 none), if present.
    pub(crate) fn get(&self, prn: u8) -> Option<&Satellite> {
        let s = &self.satellites[Prn::new(prn)?.index()];
        s.present.then_some(s)
    }

    /// PRNs and satellites present.
    pub fn iter(&self) -> impl Iterator<Item = (Prn, &Satellite)> {
        Prn::all().zip(&self.satellites).filter(|(_, s)| s.present)
    }
}

#[cfg(test)]
mod tests {
    use super::ACQUIRED;
    use super::CODE_LOCKED;
    use super::LOCKED;
    use super::MODE_AGC_SET;
    use super::MODE_CODE_ACQUIRE;
    use super::MODE_CODE_SEARCH;
    use super::MODE_EPHEMERIS_ACQUIRE;
    use super::MODE_POSITION;
    use super::NO_SIGNAL;
    use super::PRNS;
    use super::SEARCHING;
    use super::Satellite;
    use super::Sky;
    use super::UNUSABLE;
    use crate::fixtures::prn;
    use crate::units::Degrees;

    fn sat(quality: u8, ephemeris: bool) -> Satellite {
        /// A satellite's elevation.
        const ELEVATION: Degrees<i8> = Degrees::new(30);
        Satellite {
            present: true,
            elevation: ELEVATION,
            quality,
            ephemeris,
            orbit: ephemeris,
            ..Satellite::default()
        }
    }

    #[test]
    fn modes() {
        assert_eq!(sat(NO_SIGNAL, false).mode(), None);
        assert_eq!(sat(SEARCHING, false).mode(), Some(MODE_CODE_SEARCH));
        assert_eq!(sat(ACQUIRED, false).mode(), Some(MODE_CODE_ACQUIRE));
        assert_eq!(sat(UNUSABLE, false).mode(), Some(MODE_AGC_SET));
        assert_eq!(sat(CODE_LOCKED, false).mode(), Some(MODE_EPHEMERIS_ACQUIRE));
        assert_eq!(sat(LOCKED, true).mode(), Some(MODE_POSITION));
    }

    #[test]
    fn by_prn() {
        /// PRN 1 locked, 5 tracked low, 10 in the almanac, 12 of unknown
        /// elevation; 2 absent, and a PRN past the last.
        const LOW: Degrees<i8> = Degrees::new(5);
        const HIGH: Degrees<i8> = Degrees::new(40);
        const UNKNOWN: Degrees<i8> = Degrees::new(-91);
        const PRESENT: [u8; 4] = [1, 5, 10, 12];
        const ABSENT: u8 = 2;
        let mut sky = Sky::new();
        let slot = |number: u8| prn(number).index();
        sky.satellites[slot(PRESENT[0])] = sat(LOCKED, true);
        sky.satellites[slot(PRESENT[1])] = Satellite { elevation: LOW, ..sat(CODE_LOCKED, true) };
        sky.satellites[slot(PRESENT[2])] =
            Satellite { elevation: HIGH, orbit: true, present: true, ..Satellite::default() };
        sky.satellites[slot(PRESENT[3])] =
            Satellite { elevation: UNKNOWN, present: true, ..Satellite::default() };
        assert_eq!(sky.iter().map(|(p, _)| p.get()).collect::<Vec<_>>(), PRESENT);
        assert!(sky.get(ABSENT).is_none());
        assert!(sky.get(PRNS as u8 + 1).is_none());
    }
}
