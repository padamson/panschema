// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

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
