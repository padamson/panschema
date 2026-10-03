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

// panschema's tests, not upstream's.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::algo::Delta;
    use crate::layout::algo::test_support::graph_from;

    #[test]
    fn nodes_are_found_by_id_and_unknown_ids_are_not() {
        let mut drawing = DrawingEuclidean2d::<char, f32>::from_node_indices(&['a', 'b']);
        assert_eq!(drawing.len(), 2);
        *drawing.position_mut('b').unwrap() = MetricEuclidean2d(3.0, 4.0);
        assert_eq!((drawing.x('a'), drawing.y('a')), (Some(0.0), Some(0.0)));
        assert_eq!((drawing.x('b'), drawing.y('b')), (Some(3.0), Some(4.0)));
        assert_eq!((drawing.raw_entry(1).0, drawing.raw_entry(1).1), (3.0, 4.0));
        assert!(drawing.position('z').is_none());
        assert!(drawing.position_mut('z').is_none());
        assert_eq!(drawing.x('z'), None);
        assert_eq!(drawing.y('z'), None);
    }

    #[test]
    fn delta_points_from_the_second_node_to_the_first() {
        let mut drawing = DrawingEuclidean2d::<char, f32>::from_node_indices(&['a', 'b']);
        *drawing.raw_entry_mut(0) = MetricEuclidean2d(1.0, 1.0);
        *drawing.raw_entry_mut(1) = MetricEuclidean2d(4.0, 5.0);
        let delta = drawing.delta(0, 1);
        assert_eq!((delta.0, delta.1), (-3.0, -4.0));
        assert_eq!(delta.norm(), 5.0);
    }

    // The k-th node in the given order sits at radius 10·√k, turned k golden
    // angles (π(3 − √5)) from the x axis: a sunflower spiral, so no two nodes
    // start on top of each other and none sits far from the rest.
    #[test]
    fn initial_placement_spirals_out_in_the_given_order() {
        let graph = graph_from(4, &[]);
        let nodes: Vec<_> = graph.node_indices().collect();
        let order = [nodes[3], nodes[1], nodes[0], nodes[2]];
        let drawing = DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement_with_node_order(
            &graph, &order,
        );
        let golden = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
        for (k, &u) in order.iter().enumerate() {
            let r = 10.0 * (k as f32).sqrt();
            let theta = golden * k as f32;
            let (x, y) = (drawing.x(u).unwrap(), drawing.y(u).unwrap());
            assert!(
                (x - r * theta.cos()).abs() < 1e-4 && (y - r * theta.sin()).abs() < 1e-4,
                "node {k} in order at ({x}, {y}), expected radius {r} at angle {theta}"
            );
        }
    }

    #[test]
    fn initial_placement_follows_the_graph_order() {
        let graph = graph_from(3, &[]);
        let nodes: Vec<_> = graph.node_indices().collect();
        let by_graph =
            DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
        let by_order = DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement_with_node_order(
            &graph, &nodes,
        );
        for &u in &nodes {
            assert_eq!(by_graph.x(u), by_order.x(u));
            assert_eq!(by_graph.y(u), by_order.y(u));
        }
    }
}
