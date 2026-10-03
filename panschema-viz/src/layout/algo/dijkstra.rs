// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use ndarray::prelude::*;
use petgraph::visit::{EdgeRef, IntoEdges, IntoNodeIdentifiers};
use std::cmp::Ordering;
use std::{cmp::Reverse, collections::BinaryHeap, hash::Hash};

use super::{DistanceMatrix, FullDistanceMatrix};

/// A heap key for a path length. Upstream used `ordered_float::OrderedFloat`;
/// this is panschema's replacement, so that crate is not a dependency.
///
/// Only the relaxation below pushes a key, and it pushes only a length that
/// compared less than the current one, which a NaN never does. So no NaN
/// reaches the heap, `partial_cmp` always answers, and the `Equal` fallback
/// is unreachable: it exists to satisfy the type checker, not to order NaN.
struct Length<S>(S);

impl<S: PartialOrd> PartialEq for Length<S> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<S: PartialOrd> Eq for Length<S> {}

impl<S: PartialOrd> PartialOrd for Length<S> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<S: PartialOrd> Ord for Length<S> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

/// Computes the shortest path distances from a single source node `s` using Dijkstra's algorithm
/// and populates the corresponding row in the provided `distance_matrix`.
///
/// This function uses a binary heap for efficiency and handles weighted edges.
/// It modifies the `distance_matrix` in place.
///
/// # Type Parameters
///
/// * `G`: The graph type, implementing `IntoEdges` and `IntoNodeIdentifiers`.
/// * `S`: The scalar type for distances, implementing `NdFloat`.
/// * `F`: The type of the function/closure used to get edge lengths.
/// * `D`: The distance matrix type, implementing `DistanceMatrix<G::NodeId, S>`.
///
/// # Arguments
///
/// * `graph`: The graph to perform Dijkstra's algorithm on.
/// * `length`: A function or closure that takes an `EdgeRef` and returns its length (`S`).
/// * `s`: The starting node ID for the algorithm.
/// * `distance_matrix`: A mutable reference to the distance matrix to be populated.
///   The distances from `s` will be written into the row corresponding to `s`.
pub fn dijkstra_with_distance_matrix<G, S, F, D>(
    graph: G,
    length: F,
    s: G::NodeId,
    distance_matrix: &mut D,
) where
    G: IntoEdges + IntoNodeIdentifiers,
    G::NodeId: Eq + Hash + Ord,
    F: FnMut(G::EdgeRef) -> S,
    S: NdFloat,
    D: DistanceMatrix<G::NodeId, S>,
{
    let mut length = length;
    let k = distance_matrix.row_index(s).unwrap();
    let j = distance_matrix.col_index(s).unwrap();
    let mut queue = BinaryHeap::new();
    queue.push((Reverse(Length(S::zero())), s));
    distance_matrix.set_by_index(k, j, S::zero());
    while let Some((Reverse(Length(d)), u)) = queue.pop() {
        for edge in graph.edges(u) {
            let v = edge.target();
            let j = distance_matrix.col_index(v).unwrap();
            let e = d + length(edge);
            if e < distance_matrix.get_by_index(k, j) {
                queue.push((Reverse(Length(e)), v));
                distance_matrix.set_by_index(k, j, e);
            }
        }
    }
}

/// Computes the shortest path distances between all pairs of nodes using Dijkstra's algorithm.
///
/// This function runs Dijkstra's algorithm starting from every node in the graph.
///
/// # Type Parameters
///
/// * `G`: The graph type, implementing `IntoEdges` and `IntoNodeIdentifiers`.
/// * `S`: The scalar type for distances, implementing `NdFloat`.
/// * `F`: The type of the function/closure used to get edge lengths.
///
/// # Arguments
///
/// * `graph`: The graph to perform Dijkstra's algorithm on.
/// * `length`: A function or closure that takes an `EdgeRef` and returns its length (`S`).
///   Note: This function will be called multiple times for each edge.
///
/// # Returns
///
/// A `FullDistanceMatrix` containing the shortest path distances between all pairs of nodes.
pub fn all_sources_dijkstra<G, S, F>(graph: G, length: F) -> FullDistanceMatrix<G::NodeId, S>
where
    G: IntoEdges + IntoNodeIdentifiers,
    G::NodeId: Eq + Hash + Ord,
    F: FnMut(G::EdgeRef) -> S,
    S: NdFloat,
{
    let mut length = length;
    let mut distance_matrix = FullDistanceMatrix::new(graph);
    for u in graph.node_identifiers() {
        dijkstra_with_distance_matrix(graph, &mut length, u, &mut distance_matrix);
    }
    distance_matrix
}

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::Graph;

    #[test]
    fn the_heap_pops_the_shortest_length_first() {
        let mut heap = BinaryHeap::new();
        for d in [3.0_f32, 1.0, 2.0, 1.5] {
            heap.push(Reverse(Length(d)));
        }
        let popped: Vec<f32> =
            std::iter::from_fn(|| heap.pop().map(|Reverse(Length(d))| d)).collect();
        assert_eq!(popped, [1.0, 1.5, 2.0, 3.0]);
    }

    // Dijkstra reaches the same distances in any pop order, so a wrong order
    // shows up only as extra relaxation. Here `b` is reachable directly at 10
    // and through `a` at 2, with a chain hanging off it. Shortest-first pops
    // s, a, b(2), the chain once, then the stale b(10): with the incident edge
    // counts 2, 2, 3, 2, 2, 2, 2, 1 and 3 that is 19 length calls. Longest-first
    // (or a constant order, which makes the heap a stack) pops b(10) first and
    // walks the chain at the wrong distances before a corrects it: 28 calls.
    #[test]
    fn shortest_first_relaxes_the_chain_once() {
        let mut graph = Graph::<(), f32, petgraph::Undirected>::new_undirected();
        let n: Vec<_> = (0..8).map(|_| graph.add_node(())).collect();
        let (s, a, b) = (n[0], n[1], n[2]);
        graph.add_edge(s, a, 1.0);
        graph.add_edge(s, b, 10.0);
        graph.add_edge(a, b, 1.0);
        graph.add_edge(b, n[3], 1.0);
        for i in 3..7 {
            graph.add_edge(n[i], n[i + 1], 1.0);
        }
        let mut calls = 0;
        let mut distances = FullDistanceMatrix::new(&graph);
        dijkstra_with_distance_matrix(
            &graph,
            |e| {
                calls += 1;
                *e.weight()
            },
            s,
            &mut distances,
        );
        assert_eq!(calls, 19);
        let expected = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        for (i, d) in expected.iter().enumerate() {
            assert_eq!(distances.get_by_index(0, i), *d, "distance to node {i}");
        }
    }
}
