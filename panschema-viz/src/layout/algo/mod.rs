//! Graph-layout numerics, vendored from egraph-rs.
//!
//! egraph-rs publishes to PyPI, not crates.io, so its Rust crates were only
//! ever reachable as a git dependency — and the one panschema needed a patch
//! for meant carrying a fork and rebasing it against upstream churn. What the
//! graph actually uses is a small, finished slice of that workspace: three
//! layout algorithms over a 2D Euclidean drawing, sharing one all-pairs
//! Dijkstra. Owning that slice costs less than owning the fork.
//!
//! Only the Euclidean-2D path is kept. The spherical, hyperbolic and torus
//! spaces, the sparse SGD variants, and the BFS and Warshall-Floyd
//! shortest-path implementations are not vendored. Attribution and the
//! upstream revision are in THIRD-PARTY.md.

// Upstream's trait surface is kept intact rather than trimmed to exactly what
// the layouts call. An unmodified copy stays diffable against the revision it
// came from, which is what keeps a later re-sync or an upstream bug-fix cheap
// to apply; trimming would buy a little less code and cost that.
#![allow(dead_code)]

mod dijkstra;
mod distance_matrix;
mod drawing;
mod drawing_euclidean_2d;
mod kamada_kawai;
mod metric;
mod metric_euclidean_2d;
mod stress_majorization;

pub use dijkstra::all_sources_dijkstra;
pub use distance_matrix::{DistanceMatrix, FullDistanceMatrix};
pub use drawing::Drawing;
pub use drawing_euclidean_2d::DrawingEuclidean2d;
pub use kamada_kawai::KamadaKawai;
pub use metric::{Delta, Metric, MetricCartesian};
pub use metric_euclidean_2d::{DeltaEuclidean2d, MetricEuclidean2d};
pub use stress_majorization::StressMajorization;

use ndarray::prelude::*;
use num_traits::{FloatConst, FromPrimitive, Signed};
use std::hash::Hash;

/// A type usable as a node identifier in a drawing.
pub trait DrawingIndex: Eq + Hash {}
impl<T> DrawingIndex for T where T: Eq + Hash {}

/// A type usable as a coordinate value in a drawing.
pub trait DrawingValue:
    NdFloat + FromPrimitive + FloatConst + Signed + Into<f64> + From<f32>
{
}
impl<T> DrawingValue for T where
    T: NdFloat + FromPrimitive + FloatConst + Signed + Into<f64> + From<f32>
{
}
