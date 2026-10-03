// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use super::{Delta, DrawingValue, Metric};
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

/// Represents the difference vector between two points in 2D Euclidean space.
///
/// This struct implements the `Delta` trait for 2D Euclidean space.
/// It stores the x and y components of the vector.
///
/// # Type Parameters
///
/// * `S`: The scalar type used for coordinate values (must implement `DrawingValue`).
#[derive(Copy, Clone, Debug, Default)]
pub struct DeltaEuclidean2d<S>(pub S, pub S);

impl<S> Add for DeltaEuclidean2d<S>
where
    S: DrawingValue,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        DeltaEuclidean2d(self.0 + other.0, self.1 + other.1)
    }
}

impl<S> Sub for DeltaEuclidean2d<S>
where
    S: DrawingValue,
{
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        DeltaEuclidean2d(self.0 - other.0, self.1 - other.1)
    }
}

impl<S> Mul<S> for DeltaEuclidean2d<S>
where
    S: DrawingValue,
{
    type Output = Self;

    fn mul(self, other: S) -> Self {
        DeltaEuclidean2d(self.0 * other, self.1 * other)
    }
}

impl<S> Div<S> for DeltaEuclidean2d<S>
where
    S: DrawingValue,
{
    type Output = Self;

    fn div(self, other: S) -> Self {
        DeltaEuclidean2d(self.0 / other, self.1 / other)
    }
}

impl<S> Delta for DeltaEuclidean2d<S>
where
    S: DrawingValue,
{
    type S = S;
    fn norm(&self) -> Self::S {
        self.0.hypot(self.1)
    }
}

/// Represents a point in 2D Euclidean space.
///
/// This struct implements the `Metric` trait for 2D Euclidean space.
/// It stores the x and y coordinates of the point.
///
/// # Type Parameters
///
/// * `S`: The scalar type used for coordinate values (must implement `DrawingValue`).
#[derive(Copy, Clone, Debug, Default)]
pub struct MetricEuclidean2d<S>(pub S, pub S);

impl<S> AddAssign<DeltaEuclidean2d<S>> for MetricEuclidean2d<S>
where
    S: DrawingValue,
{
    fn add_assign(&mut self, other: DeltaEuclidean2d<S>) {
        self.0 += other.0;
        self.1 += other.1;
    }
}

impl<S> SubAssign<DeltaEuclidean2d<S>> for MetricEuclidean2d<S>
where
    S: DrawingValue,
{
    fn sub_assign(&mut self, other: DeltaEuclidean2d<S>) {
        self.0 -= other.0;
        self.1 -= other.1;
    }
}

impl<S> Metric for MetricEuclidean2d<S>
where
    S: DrawingValue,
{
    type D = DeltaEuclidean2d<S>;
}

impl<'b, S> Sub<&'b MetricEuclidean2d<S>> for &MetricEuclidean2d<S>
where
    S: DrawingValue,
{
    type Output = DeltaEuclidean2d<S>;

    fn sub(self, other: &'b MetricEuclidean2d<S>) -> DeltaEuclidean2d<S> {
        DeltaEuclidean2d(self.0 - other.0, self.1 - other.1)
    }
}

// panschema's tests, not upstream's.
#[cfg(test)]
mod tests {
    use super::*;

    fn pair(d: DeltaEuclidean2d<f32>) -> (f32, f32) {
        (d.0, d.1)
    }

    #[test]
    fn deltas_combine_componentwise() {
        let (a, b) = (DeltaEuclidean2d(3.0_f32, 4.0), DeltaEuclidean2d(1.0, -2.0));
        assert_eq!(pair(a + b), (4.0, 2.0));
        assert_eq!(pair(a - b), (2.0, 6.0));
        assert_eq!(pair(a * 2.0), (6.0, 8.0));
        assert_eq!(pair(a / 2.0), (1.5, 2.0));
        assert_eq!(a.norm(), 5.0);
    }

    #[test]
    fn points_move_by_deltas_and_differ_by_them() {
        let mut p = MetricEuclidean2d(1.0_f32, 1.0);
        p += DeltaEuclidean2d(2.0, 3.0);
        assert_eq!((p.0, p.1), (3.0, 4.0));
        p -= DeltaEuclidean2d(1.0, 5.0);
        assert_eq!((p.0, p.1), (2.0, -1.0));
        let q = MetricEuclidean2d(5.0_f32, 3.0);
        assert_eq!(pair(&q - &p), (3.0, 4.0));
    }
}
