//! The least-squares geometry of a position and time solution: the
//! inverse of the normal matrix of the satellites' lines of sight, and
//! the dilutions of precision on its diagonal.  Generic over the
//! floating point type: `f32` on the firmware, `f64` in `move_trace`.

use core::cmp::Ordering;
use core::ops::Add;
use core::ops::AddAssign;
use core::ops::DivAssign;
use core::ops::Mul;
use core::ops::Neg;
use core::ops::SubAssign;

use crate::vector::Enu;

/// The solution's unknowns: east, north, up and the receiver's clock.
pub(crate) const UNKNOWNS: usize = 4;
/// Gauss-Jordan's augmented matrix's columns: the normal matrix's and an
/// identity's.
const IDENTITY: usize = UNKNOWNS;
const AUGMENTED: usize = UNKNOWNS + IDENTITY;
/// Pairs counted twice in `n` times `n + 1`.
const PAIRS: usize = 2;

/// The `n`th triangular number, `n(n + 1) / 2`: the entries of an `n` by
/// `n` matrix's upper triangle, its diagonal included.
pub(crate) const fn triangular(n: usize) -> usize {
    n * (n + 1) / PAIRS
}

/// The floating point types the geometry is computed in.
pub trait Real:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Mul<Output = Self>
    + Neg<Output = Self>
    + AddAssign
    + SubAssign
    + DivAssign
{
    const ZERO: Self;
    const ONE: Self;
    fn abs(self) -> Self;
    fn total_cmp(&self, other: &Self) -> Ordering;
}

impl Real for f32 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;

    fn abs(self) -> Self {
        f32::abs(self)
    }

    fn total_cmp(&self, other: &Self) -> Ordering {
        f32::total_cmp(self, other)
    }
}

impl Real for f64 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;

    fn abs(self) -> Self {
        f64::abs(self)
    }

    fn total_cmp(&self, other: &Self) -> Ordering {
        f64::total_cmp(self, other)
    }
}

/// The inverse of the least-squares normal matrix G'G, rows of G being
/// (-east, -north, -up, 1) to each satellite, from the satellites' unit
/// lines of sight, east, north and up; `None` for fewer than four, or a
/// pivot less than `singular`, a singular geometry.  Out of line, so its
/// matrix is off the stack before the caller goes on: inlined, it joined
/// `oncore_app::run`'s and `send_extension`'s frames (`make stack`).
#[inline(never)]
pub fn inverse<T: Real>(
    sats: impl Iterator<Item = Enu<T>>,
    singular: T,
) -> Option<[[T; UNKNOWNS]; UNKNOWNS]> {
    // The normal matrix G'G of rows (-e, -n, -u, 1), and an identity to
    // turn into its inverse.
    let mut m = [[T::ZERO; AUGMENTED]; UNKNOWNS];
    let mut n = 0;
    for s in sats {
        let row = [-s.east, -s.north, -s.up, T::ONE];
        for i in 0..UNKNOWNS {
            for j in 0..UNKNOWNS {
                m[i][j] += row[i] * row[j];
            }
        }
        n += 1;
    }
    if n < UNKNOWNS {
        return None;
    }
    for (i, r) in m.iter_mut().enumerate() {
        r[UNKNOWNS + i] = T::ONE;
    }
    // Gauss-Jordan with partial pivoting.
    for c in 0..UNKNOWNS {
        let p = (c..UNKNOWNS).max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs()))?;
        if m[p][c].abs() < singular {
            return None;
        }
        m.swap(c, p);
        let d = m[c][c];
        for v in m[c].iter_mut() {
            *v /= d;
        }
        for r in 0..UNKNOWNS {
            if r != c {
                let f = m[r][c];
                let pivot = m[c];
                for (v, p) in m[r].iter_mut().zip(pivot) {
                    *v -= f * p;
                }
            }
        }
    }
    let mut q = [[T::ZERO; UNKNOWNS]; UNKNOWNS];
    for (qr, mr) in q.iter_mut().zip(m) {
        qr.copy_from_slice(&mr[UNKNOWNS..]);
    }
    Some(q)
}

/// The squares of the dilutions of precision of each unknown: the
/// inverse normal matrix's diagonal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dops<T> {
    pub east: T,
    pub north: T,
    pub up: T,
    pub clock: T,
}

impl<T: Real> Dops<T> {
    /// The diagonal of `q`, an [`inverse`].
    pub fn from_inverse(q: &[[T; UNKNOWNS]; UNKNOWNS]) -> Self {
        Self { east: q[0][0], north: q[1][1], up: q[2][2], clock: q[3][3] }
    }

    /// HDOP squared.
    pub fn horizontal(&self) -> T {
        self.east + self.north
    }

    /// PDOP squared.
    pub fn position(&self) -> T {
        self.horizontal() + self.up
    }

    /// GDOP squared.
    pub fn geometric(&self) -> T {
        self.position() + self.clock
    }

    /// VDOP squared.
    pub fn vertical(&self) -> T {
        self.up
    }

    /// TDOP squared.
    pub fn time(&self) -> T {
        self.clock
    }
}
