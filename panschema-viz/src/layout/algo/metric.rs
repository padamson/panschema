// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed,
// at rev 9b38f5d. Copyright (c) the egraph-rs authors. See THIRD-PARTY.md.
//
// Only the Euclidean-2D path panschema's graph uses is kept; the spherical,
// hyperbolic and torus spaces, the sparse SGD variants, and the BFS and
// Warshall-Floyd shortest-path implementations are not vendored.

use super::DrawingValue;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

/// Represents the difference (vector) between two points in a metric space.
///
/// This trait defines the operations that must be supported by a type representing
/// the difference between two points in a metric space, such as vector addition,
/// subtraction, scalar multiplication, and computing the norm (distance).
///
/// # Type Parameters
///
/// * `S`: The scalar type used for coordinate values and distance calculations.
pub trait Delta:
    Sized + Add<Self> + Sub<Self> + Mul<Self::S, Output = Self> + Div<Self::S> + Clone
{
    /// The scalar type used for coordinate values and distance calculations.
    type S: DrawingValue;

    /// Computes the norm (distance) of this difference vector.
    ///
    /// In a Euclidean space, this would be the length of the vector.
    /// In other spaces, it represents the distance measure appropriate to that space.
    fn norm(&self) -> Self::S;
}

/// Defines a metric space where distances between points can be measured.
///
/// A metric space is a set where a notion of distance between elements is defined.
/// This trait defines the basic operations needed for types representing points in such spaces.
pub trait Metric: Sized + AddAssign<Self::D> + SubAssign<Self::D> {
    /// The type representing the difference (vector) between two points in this metric space.
    type D: Delta;
}

/// A specialized metric for Cartesian coordinate systems.
///
/// This trait extends the basic `Metric` trait with functionality specific to
/// Cartesian coordinate systems, such as accessing individual dimensions.
pub trait MetricCartesian: Metric {
    /// Returns the value of the `n`-th dimension of this point.
    ///
    /// # Parameters
    ///
    /// * `n`: The index of the dimension to access.
    ///
    /// # Returns
    ///
    /// The scalar value of the `n`-th dimension.
    fn nth(&self, n: usize) -> &<<Self as Metric>::D as Delta>::S;

    fn nth_mut(&mut self, n: usize) -> &mut <<Self as Metric>::D as Delta>::S;
}
