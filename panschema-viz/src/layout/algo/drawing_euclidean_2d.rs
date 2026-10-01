// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use super::{DeltaEuclidean2d, Drawing, DrawingIndex, DrawingValue, MetricEuclidean2d};
use num_traits::{FloatConst, FromPrimitive};
use petgraph::visit::IntoNodeIdentifiers;
use std::collections::HashMap;

/// Represents a drawing of items (nodes) in 2-dimensional Euclidean space.
///
/// This is a specialized version of `DrawingEuclidean` for 2D.
/// It implements the `Drawing` trait.
///
/// # Type Parameters
///
/// * `N`: The type used for indexing items (must implement `DrawingIndex`).
/// * `S`: The scalar type used for coordinates (must implement `DrawingValue`).
pub struct DrawingEuclidean2d<N, S> {
    coordinates: Vec<MetricEuclidean2d<S>>,
    index_map: HashMap<N, usize>,
}

impl<N, S> DrawingEuclidean2d<N, S>
where
    N: DrawingIndex,
    S: DrawingValue,
{
    pub fn new<G>(graph: G) -> Self
    where
        G: IntoNodeIdentifiers,
        G::NodeId: DrawingIndex + Into<N>,
        N: Copy,
        S: Default,
    {
        let indices = graph
            .node_identifiers()
            .map(|u| u.into())
            .collect::<Vec<N>>();
        Self::from_node_indices(&indices)
    }

    pub fn from_node_indices(indices: &[N]) -> Self
    where
        N: Copy,
        S: Default,
    {
        let index_map = indices
            .iter()
            .enumerate()
            .map(|(i, &u)| (u, i))
            .collect::<HashMap<_, _>>();
        let coordinates = vec![MetricEuclidean2d::default(); indices.len()];
        Self {
            coordinates,
            index_map,
        }
    }

    pub fn x(&self, u: N) -> Option<S> {
        self.position(u).map(|p| p.0)
    }

    pub fn y(&self, u: N) -> Option<S> {
        self.position(u).map(|p| p.1)
    }

    pub fn initial_placement<G>(graph: G) -> Self
    where
        G: IntoNodeIdentifiers,
        G::NodeId: DrawingIndex + Into<N>,
        N: Copy,
        S: FloatConst + FromPrimitive + Default,
    {
        let nodes = graph.node_identifiers().collect::<Vec<_>>();
        Self::initial_placement_with_node_order(graph, &nodes)
    }

    pub fn initial_placement_with_node_order<G>(graph: G, nodes: &[G::NodeId]) -> Self
    where
        G: IntoNodeIdentifiers,
        G::NodeId: DrawingIndex + Into<N>,
        N: Copy,
        S: FloatConst + FromPrimitive + Default,
    {
        let mut drawing = Self::new(graph);
        for (i, &u) in nodes.iter().enumerate() {
            let r = S::from_usize(10).unwrap() * S::from_usize(i).unwrap().sqrt();
            let theta = S::PI()
                * (S::from_usize(3).unwrap() - S::from_usize(5).unwrap().sqrt())
                * (S::from_usize(i).unwrap());
            let x = r * theta.cos();
            let y = r * theta.sin();
            if let Some(p) = drawing.position_mut(u.into()) {
                *p = MetricEuclidean2d(x, y);
            }
        }
        drawing
    }
}

impl<N, S> Drawing for DrawingEuclidean2d<N, S>
where
    N: DrawingIndex,
    S: DrawingValue,
{
    type Index = N;
    type Item = MetricEuclidean2d<S>;

    fn len(&self) -> usize {
        self.coordinates.len()
    }

    fn position(&self, u: Self::Index) -> Option<&Self::Item> {
        self.index_map.get(&u).map(|&i| &self.coordinates[i])
    }

    fn position_mut(&mut self, u: Self::Index) -> Option<&mut Self::Item> {
        self.index_map.get(&u).map(|&i| &mut self.coordinates[i])
    }

    fn raw_entry(&self, i: usize) -> &Self::Item {
        &self.coordinates[i]
    }

    fn raw_entry_mut(&mut self, i: usize) -> &mut Self::Item {
        &mut self.coordinates[i]
    }

    fn delta(&self, i: usize, j: usize) -> DeltaEuclidean2d<S> {
        self.raw_entry(i) - self.raw_entry(j)
    }
}
