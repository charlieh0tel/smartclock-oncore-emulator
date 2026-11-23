//! Three-vectors: the axis count, vectors in local east, north and up
//! axes, and unit directions in ECEF axes.  ECEF positions and
//! velocities, with their units, are `units::Ecef` and
//! `units::EcefVelocity`.

/// The axes of a three-vector: x, y and z, or east, north and up.
pub const AXES: usize = 3;

/// A vector in local east, north and up axes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Enu<T> {
    pub east: T,
    pub north: T,
    pub up: T,
}

/// A unit vector in ECEF axes: a line of sight, or a local axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Direction([f64; AXES]);

impl Direction {
    /// `v`, which must be a unit vector.
    pub const fn new(v: [f64; AXES]) -> Self {
        Self(v)
    }

    /// x, y and z.
    pub(crate) const fn get(self) -> [f64; AXES] {
        self.0
    }

    /// The cosine of the angle to `other`.
    pub fn dot(self, other: Self) -> f64 {
        dot(self.0, other.0)
    }
}

/// The dot product of `a` and `b`, x, y, z in turn.
pub(crate) fn dot(a: [f64; AXES], b: [f64; AXES]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
