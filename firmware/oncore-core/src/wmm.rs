//! Magnetic declination from the World Magnetic Model 2025 (NOAA,
//! `third_party/noaa/WMM2025COF/`), for `@@Bk`'s magnetic variation: the
//! model's spherical harmonic series to degree 12, in Schmidt
//! semi-normalized associated Legendre functions, with secular variation,
//! evaluated at a geodetic position and rotated from geocentric to
//! ellipsoidal axes, as NOAA's reference software does.  NOAA's test
//! values check it.

use libm::asin;
use libm::pow;
use libm::sqrt;

use crate::civil::DecimalYear;
use crate::dop::triangular;
use crate::module::WGS84_SEMI_MAJOR_M;
use crate::units::Degrees;
use crate::units::Kilometers;
use crate::units::M_PER_KM;
use crate::units::Nanotesla;
use crate::units::NanoteslaPerYear;
use crate::units::Radians;
use crate::units::meridian;

include!(concat!(env!("OUT_DIR"), "/wmm.rs"));

/// The model's maximum degree.
const N: usize = 12;
/// The geomagnetic reference radius.
const RADIUS: Kilometers = Kilometers::new(6371.2);
/// The field's terms go as `(RADIUS / r)` to the degree plus this: the
/// potential's `n + 1`, and one more for its gradient.
const RADIAL_POWER: usize = 2;
/// Schmidt semi-normalization's factor of 2 for orders above 0, taken in
/// at order 1 and carried up the orders with the rest.
const SCHMIDT_FIRST_ORDER: f64 = 2.0;
/// WGS 84's semi-major axis.
const WGS84_A: Kilometers = Kilometers::new(WGS84_SEMI_MAJOR_M / M_PER_KM);

/// The field's north, east and down components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Field {
    pub(crate) x: Nanotesla,
    pub(crate) y: Nanotesla,
    pub(crate) z: Nanotesla,
}

impl Field {
    /// Declination, east of true north.
    pub(crate) fn declination(&self) -> Degrees<f64> {
        self.y.direction(self.x).degrees()
    }
}

/// The `k`th odd number, `2k - 1`: the Legendre recursions' factors.
const fn odd(k: usize) -> usize {
    ODD_STRIDE * k - 1
}

/// The step from one odd number to the next.
const ODD_STRIDE: usize = 2;

/// The field at geodetic latitude and longitude, height above the
/// ellipsoid and date.
///
/// The Schmidt semi-normalized associated Legendre functions of the sine
/// of geocentric latitude, and their derivatives in latitude, come by
/// recursion, Gauss-normalized, then scaled: for each order m, up the
/// degrees n from m, each from the two below, so only the last two are
/// kept, and the next order's diagonal from this one's.
pub(crate) fn field(
    latitude: Degrees<f64>,
    longitude: Degrees<f64>,
    height: Kilometers,
    year: DecimalYear,
) -> Field {
    let (phi, lambda) = (latitude.radians(), longitude.radians());
    // Geodetic to geocentric spherical.
    let (p, z) = meridian(WGS84_A, phi, height);
    let r = p.root_sum_square(z);
    let phi_c = Radians::new(asin(z / r));
    let (x, z) = (phi_c.sin(), phi_c.cos());
    let power: [f64; N + 1] = core::array::from_fn(|n| pow(RADIUS / r, (n + RADIAL_POWER) as f64));
    let dt = year - EPOCH;

    let zero = Nanotesla::new(0.0);
    let (mut bx, mut by, mut bz) = (zero, zero, zero);
    // Gauss-normalized P(m, m) and its derivative.
    let (mut diagonal, mut diagonal_d) = (1.0, 0.0);
    // Gauss to Schmidt factors of order m, by degree.
    let mut schmidt = [1.0; N + 1];
    for m in 0..=N {
        if m == 0 {
            for n in 1..=N {
                let below = schmidt[n - 1];
                schmidt[n] = below * odd(n) as f64 / n as f64;
            }
        } else {
            (diagonal, diagonal_d) = (z * diagonal, z * diagonal_d + x * diagonal);
            let factor = if m == 1 { SCHMIDT_FIRST_ORDER } else { 1.0 };
            for (n, s) in schmidt.iter_mut().enumerate().skip(m) {
                *s *= sqrt(((n - m + 1) as f64 * factor) / (n + m) as f64);
            }
        }
        let m_lambda = lambda * m as f64;
        let (cos_m_lambda, sin_m_lambda) = (m_lambda.cos(), m_lambda.sin());
        // P(n - 1, m) and P(n - 2, m), and their derivatives.
        let (mut below, mut below_d) = (diagonal, diagonal_d);
        let (mut two_below, mut two_below_d) = (0.0, 0.0);
        for n in m..=N {
            let (pn, dpn) = if n == m {
                (diagonal, diagonal_d)
            } else {
                let c = if n == m + 1 {
                    0.0
                } else {
                    ((n - 1) * (n - 1) - m * m) as f64 / (odd(n) * odd(n - 1)) as f64
                };
                (x * below - c * two_below, x * below_d - z * below - c * two_below_d)
            };
            (two_below, two_below_d, below, below_d) = (below, below_d, pn, dpn);
            if n == 0 {
                continue;
            }
            // Schmidt semi-normalized P(n, m) and its derivative in
            // geocentric latitude, the recursion's being in colatitude
            // (NOAA's reference software's Pcup and dPcup).
            let (legendre, legendre_d) = (pn * schmidt[n], -dpn * schmidt[n]);
            let k = triangular(n) + m;
            let [g, h, gd, hd] = COEFFICIENTS[k - 1].map(f64::from);
            let g = Nanotesla::new(g) + dt * NanoteslaPerYear::new(gd);
            let h = Nanotesla::new(h) + dt * NanoteslaPerYear::new(hd);
            bz -= power[n] * (g * cos_m_lambda + h * sin_m_lambda) * (n + 1) as f64 * legendre;
            by += power[n] * (g * sin_m_lambda - h * cos_m_lambda) * m as f64 * legendre;
            bx -= power[n] * (g * cos_m_lambda + h * sin_m_lambda) * legendre_d;
        }
    }
    by = by / phi_c.cos();
    // Geocentric to ellipsoidal axes.
    let psi = phi_c - phi;
    Field { x: bx * psi.cos() - bz * psi.sin(), y: by, z: bx * psi.sin() + bz * psi.cos() }
}

#[cfg(test)]
mod tests {
    use super::field;
    use crate::civil::DecimalYear;
    use crate::units::Degrees;
    use crate::units::Kilometers;
    use crate::units::Nanotesla;

    const TEST_VALUES: &str = include_str!("../../../third_party/noaa/WMM2025COF/WMM2025_TestValues.txt");

    /// The test values' columns (`WMM2025_TestValues.txt`'s header):
    /// decimal year, height km, latitude and longitude degrees,
    /// declination degrees, and X, Y and Z nT; the file's lines of values.
    const YEAR: usize = 0;
    const HEIGHT_KM: usize = 1;
    const LATITUDE: usize = 2;
    const LONGITUDE: usize = 3;
    const DECLINATION: usize = 4;
    const X: usize = 7;
    const Y: usize = 8;
    const Z: usize = 9;
    const VALUES: usize = 12;
    /// Declination to its printed 0.01 degree, rounded, and X, Y and Z to
    /// 0.01 nT.
    const DECLINATION_TOLERANCE: Degrees<f64> = Degrees::new(0.006);
    const FIELD_TOLERANCE: Nanotesla = Nanotesla::new(0.01);

    /// Every one of NOAA's test values.
    #[test]
    fn noaa_test_values() {
        let mut checked = 0;
        for line in TEST_VALUES.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let f: Vec<f64> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let b = field(
                Degrees::new(f[LATITUDE]),
                Degrees::new(f[LONGITUDE]),
                Kilometers::new(f[HEIGHT_KM]),
                DecimalYear::new(f[YEAR]),
            );
            assert!(
                (b.declination() - Degrees::new(f[DECLINATION])).abs() <= DECLINATION_TOLERANCE,
                "{line}: D {:?}",
                b.declination()
            );
            for (got, want, name) in [(b.x, f[X], "X"), (b.y, f[Y], "Y"), (b.z, f[Z], "Z")] {
                let want = Nanotesla::new(want);
                assert!((got - want).abs() < FIELD_TOLERANCE, "{line}: {name} {got:?} vs {want:?}");
            }
            checked += 1;
        }
        assert!(checked >= VALUES, "{checked} test values");
    }
}
