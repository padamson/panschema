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
//! shortest-path implementations are not vendored. Within what is kept, only
//! code panschema uses remains. The compiler's dead-code lint holds that for
//! functions, methods and types, but it cannot see trait impls or derives:
//! those were checked by hand, and the two small value types keep their
//! ordinary derives (`Copy`, `Debug`, `Default`) whether or not anything uses
//! them. Attribution and the upstream revision are in THIRD-PARTY.md.

mod dijkstra;
mod distance_matrix;
mod drawing;
mod drawing_euclidean_2d;
mod full_sgd;
mod kamada_kawai;
mod metric;
mod metric_euclidean_2d;
mod scheduler;
mod scheduler_exponential;
mod sgd;
mod stress_majorization;

pub use dijkstra::all_sources_dijkstra;
pub use distance_matrix::{DistanceMatrix, FullDistanceMatrix};
pub use drawing::Drawing;
pub use drawing_euclidean_2d::DrawingEuclidean2d;
pub use full_sgd::FullSgd;
pub use kamada_kawai::KamadaKawai;
pub use metric::{Delta, Metric};
pub use metric_euclidean_2d::{DeltaEuclidean2d, MetricEuclidean2d};
pub use scheduler::Scheduler;
pub use scheduler_exponential::SchedulerExponential;
pub use sgd::Sgd;
pub use stress_majorization::StressMajorization;

use ndarray::prelude::*;
use num_traits::{FloatConst, FromPrimitive, Signed};
use std::hash::Hash;

/// The randomness SGD needs: an in-place shuffle of its node pairs.
///
/// This trait is panschema's, not upstream's. Upstream's `Sgd::shuffle` took a
/// `rand::Rng`; this keeps it generic without that crate, and keeps the vendored
/// code free of any path into the rest of panschema. `layout::rng::SplitMix64`
/// implements it.
pub trait Shuffle {
    fn shuffle<T>(&mut self, items: &mut [T]);
}

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

/// Fixtures the vendored files' tests share. panschema's, not upstream's.
#[cfg(test)]
mod test_support {
    use super::{Drawing, DrawingEuclidean2d, DrawingIndex, DrawingValue, MetricEuclidean2d};
    use petgraph::graph::NodeIndex;
    use petgraph::{Graph, Undirected};

    pub(super) type TestGraph = Graph<(), (), Undirected>;

    /// Nodes `0..n`, joined by `edges`.
    pub(super) fn graph_from(n: usize, edges: &[(usize, usize)]) -> TestGraph {
        let mut graph = Graph::new_undirected();
        let nodes: Vec<_> = (0..n).map(|_| graph.add_node(())).collect();
        for &(a, b) in edges {
            graph.add_edge(nodes[a], nodes[b], ());
        }
        graph
    }

    /// Nodes `0..n` joined in a line.
    pub(super) fn path(n: usize) -> TestGraph {
        graph_from(n, &(1..n).map(|i| (i - 1, i)).collect::<Vec<_>>())
    }

    /// Puts node `i` at `points[i]`.
    pub(super) fn place<N: DrawingIndex, S: DrawingValue>(
        drawing: &mut DrawingEuclidean2d<N, S>,
        points: &[(S, S)],
    ) {
        for (i, &(x, y)) in points.iter().enumerate() {
            *drawing.raw_entry_mut(i) = MetricEuclidean2d(x, y);
        }
    }

    /// A drawing of `graph` with node `i` at `points[i]`.
    pub(super) fn placed<S: DrawingValue + Default>(
        graph: &TestGraph,
        points: &[(S, S)],
    ) -> DrawingEuclidean2d<NodeIndex, S> {
        let mut drawing = DrawingEuclidean2d::new(graph);
        place(&mut drawing, points);
        drawing
    }

    /// Every node's position, in index order.
    pub(super) fn positions<N: DrawingIndex, S: DrawingValue>(
        drawing: &DrawingEuclidean2d<N, S>,
    ) -> Vec<(S, S)> {
        (0..drawing.len())
            .map(|i| (drawing.raw_entry(i).0, drawing.raw_entry(i).1))
            .collect()
    }
}
