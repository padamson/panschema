// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

//! # Kamada-Kawai Graph Layout Algorithm
//!
//! This crate provides an implementation of the Kamada-Kawai graph layout algorithm,
//! a force-directed layout method for drawing graphs in 2D space.
//!
//! The algorithm works by modeling the graph as a system of springs, where:
//! - Each pair of nodes is connected by a spring
//! - The ideal length of each spring is proportional to the shortest path distance between nodes
//! - The algorithm iteratively moves nodes to minimize the energy of the spring system
//!
//! ## References
//!
//! Kamada, T., & Kawai, S. (1989). An algorithm for drawing general undirected graphs.
//! Information Processing Letters, 31(1), 7-15.
//!
//! ## Example
//!
//! Not run as a doctest: this module is private, so rustdoc would compile
//! the example as an external crate that cannot reach it.
//!
//! ```ignore
//! use petgraph::prelude::*;
//! use crate::layout::algo::{Drawing, DrawingEuclidean2d, KamadaKawai};
//!
//! // Create an undirected graph
//! let mut graph = Graph::new_undirected();
//! let n1 = graph.add_node(());
//! let n2 = graph.add_node(());
//! let n3 = graph.add_node(());
//! graph.add_edge(n1, n2, ());
//! graph.add_edge(n2, n3, ());
//!
//! // Create initial placement
//! let mut drawing = DrawingEuclidean2d::<NodeIndex, f32>::initial_placement(&graph);
//!
//! // Create and run the Kamada-Kawai layout algorithm
//! let kamada_kawai = KamadaKawai::new(&graph, |_| 1.0);
//! kamada_kawai.run(&mut drawing);
//! ```
//!
use super::{DistanceMatrix, FullDistanceMatrix, all_sources_dijkstra};
use super::{Drawing, DrawingEuclidean2d, DrawingIndex, DrawingValue};
use ndarray::prelude::*;
use petgraph::visit::{IntoEdges, IntoNodeIdentifiers, NodeCount};

#[cfg(test)]
thread_local! {
    /// Spring terms `pair_gradient` has evaluated on this thread, so tests can
    /// check what `run` costs without timing it.
    static PAIR_GRADIENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn norm<S>(x: S, y: S) -> S
where
    S: DrawingValue,
{
    x.hypot(y).max(S::one())
}

/// Implementation of the Kamada-Kawai graph layout algorithm.
///
/// This struct stores the spring constants and ideal distances between nodes,
/// calculated based on the graph's topology, as well as the convergence threshold.
pub struct KamadaKawai<S> {
    /// Spring constants for each pair of nodes
    k: Array2<S>,
    /// Ideal distances between nodes
    l: Array2<S>,
    /// Convergence threshold
    pub eps: S,
    /// Most node moves `run` makes. Set by panschema, not upstream, which ran
    /// until convergence with no bound, so a layout that never converged never
    /// returned. Moves grow with the square of the node count, staying well under
    /// n² across the graph shapes measured, so the default of 10·n² binds only
    /// when convergence fails. Each move costs O(n); a full O(n²) recompute
    /// is added only when the running sums report convergence, to confirm it.
    pub max_moves: usize,
}

impl<S> KamadaKawai<S> {
    /// Creates a new Kamada-Kawai layout algorithm instance from a graph.
    ///
    /// This constructor calculates the shortest path distances between all pairs of nodes
    /// in the graph, using the provided edge length function.
    ///
    /// # Arguments
    ///
    /// * `graph` - The input graph
    /// * `length` - A function that returns the length of each edge
    ///
    /// # Returns
    ///
    /// A new `KamadaKawai` instance
    pub fn new<G, F>(graph: G, length: F) -> Self
    where
        G: IntoEdges + IntoNodeIdentifiers + NodeCount,
        G::NodeId: DrawingIndex + Ord,
        F: FnMut(G::EdgeRef) -> S,
        S: DrawingValue,
    {
        let l = all_sources_dijkstra(graph, length);
        KamadaKawai::new_with_distance_matrix(&l)
    }

    /// Creates a new Kamada-Kawai layout algorithm instance from a pre-computed distance matrix.
    ///
    /// This constructor uses an existing distance matrix rather than calculating
    /// it from the graph. This can be useful when the distances have already been
    /// computed or when using a custom distance metric.
    ///
    /// # Arguments
    ///
    /// * `d` - A full distance matrix containing shortest path distances between all pairs of nodes
    ///
    /// # Returns
    ///
    /// A new `KamadaKawai` instance
    pub fn new_with_distance_matrix<N>(d: &FullDistanceMatrix<N, S>) -> Self
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        let eps = S::from_f32(1e-1).unwrap();
        let n = d.shape().0;

        // Both halves are read from one triangle so that k and l are symmetric
        // bit for bit, which `shares_of` relies on. A shortest-path matrix is
        // symmetric up to rounding, but summing a path's lengths in the other
        // order can differ in the last bit.
        let mut l = Array2::zeros((n, n));
        let mut k = Array2::zeros((n, n));
        for i in 0..n {
            for j in 0..n {
                l[[i, j]] = d.get_by_index(i.min(j), i.max(j));
                k[[i, j]] = S::one() / (l[[i, j]] * l[[i, j]]);
            }
        }
        let max_moves = n.saturating_mul(n).saturating_mul(10);
        KamadaKawai {
            k,
            l,
            eps,
            max_moves,
        }
    }

    /// The node with the largest gradient, computed from scratch, or None once
    /// every gradient is under `eps`. `run` keeps running sums instead; tests
    /// use this as the reference it must agree with.
    #[cfg(test)]
    fn select_node<N>(&self, drawing: &DrawingEuclidean2d<N, S>) -> Option<usize>
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        let mut gradients = vec![(S::zero(), S::zero()); drawing.len()];
        self.fill_gradients(drawing, &mut gradients);
        self.steepest(&gradients)
    }

    /// Computes every node's energy gradient from scratch into `gradients`.
    fn fill_gradients<N>(&self, drawing: &DrawingEuclidean2d<N, S>, gradients: &mut [(S, S)])
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        for (m, gradient) in gradients.iter_mut().enumerate() {
            *gradient = self.gradient(m, drawing);
        }
    }

    /// The node with the largest gradient, or None once every gradient is
    /// under `eps`. NaN never compares greater, so a NaN gradient reads as
    /// converged.
    fn steepest(&self, gradients: &[(S, S)]) -> Option<usize>
    where
        S: DrawingValue,
    {
        let mut delta2_max = S::zero();
        let mut m_target = 0;
        for (m, &(dedx, dedy)) in gradients.iter().enumerate() {
            let delta2 = dedx * dedx + dedy * dedy;
            if delta2 > delta2_max {
                delta2_max = delta2;
                m_target = m;
            }
        }
        if delta2_max < self.eps * self.eps {
            None
        } else {
            Some(m_target)
        }
    }

    /// The energy gradient at node `m`: the sum of `pair_gradient(m, i)` over
    /// every other node `i`.
    fn gradient<N>(&self, m: usize, drawing: &DrawingEuclidean2d<N, S>) -> (S, S)
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        let mut dedx = S::zero();
        let mut dedy = S::zero();
        for i in (0..drawing.len()).filter(|&i| i != m) {
            let (gx, gy) = self.pair_gradient(m, i, drawing);
            dedx += gx;
            dedy += gy;
        }
        (dedx, dedy)
    }

    /// Writes into `shares[i]` the share of node `i`'s gradient that comes from
    /// its spring to `m`, and returns `m`'s own gradient. `k` and `l` are
    /// symmetric, so each spring pulls on `m` exactly opposite to how it pulls
    /// on `i`: `m`'s gradient is the negated sum, bit for bit what `gradient`
    /// computes.
    fn shares_of<N>(
        &self,
        m: usize,
        drawing: &DrawingEuclidean2d<N, S>,
        shares: &mut [(S, S)],
    ) -> (S, S)
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        let mut dedx = S::zero();
        let mut dedy = S::zero();
        for (i, share) in shares.iter_mut().enumerate() {
            *share = if i == m {
                (S::zero(), S::zero())
            } else {
                self.pair_gradient(i, m, drawing)
            };
            dedx -= share.0;
            dedy -= share.1;
        }
        (dedx, dedy)
    }

    /// The share of the energy gradient at `m` that comes from the spring
    /// between `m` and `i`.
    fn pair_gradient<N>(&self, m: usize, i: usize, drawing: &DrawingEuclidean2d<N, S>) -> (S, S)
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        #[cfg(test)]
        PAIR_GRADIENTS.with(|count| count.set(count.get() + 1));
        let KamadaKawai { k, l, .. } = self;
        let dx = drawing.raw_entry(m).0 - drawing.raw_entry(i).0;
        let dy = drawing.raw_entry(m).1 - drawing.raw_entry(i).1;
        let d = norm(dx, dy);
        let scale = k[[m, i]] * (S::one() - l[[m, i]] / d);
        (scale * dx, scale * dy)
    }

    /// Moves a single node to reduce its energy.
    ///
    /// This method calculates the optimal position for the specified node
    /// using a second-order approximation of the energy function, and updates
    /// its position in the drawing.
    ///
    /// # Arguments
    ///
    /// * `m` - The index of the node to move
    /// * `drawing` - The current node positions, which will be updated
    pub fn apply_to_node<N>(&self, m: usize, drawing: &mut DrawingEuclidean2d<N, S>)
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        let n = drawing.len();
        let KamadaKawai { k, l, .. } = self;
        let xm = drawing.raw_entry(m).0;
        let ym = drawing.raw_entry(m).1;
        let mut hxx = S::zero();
        let mut hyy = S::zero();
        let mut hxy = S::zero();
        let mut dedx = S::zero();
        let mut dedy = S::zero();
        for i in 0..n {
            if i != m {
                let xi = drawing.raw_entry(i).0;
                let yi = drawing.raw_entry(i).1;
                let dx = xm - xi;
                let dy = ym - yi;
                let d = norm(dx, dy);
                let d3 = d * d * d;
                hxx += k[[m, i]] * (S::one() - l[[m, i]] * dy * dy / d3);
                hyy += k[[m, i]] * (S::one() - l[[m, i]] * dx * dx / d3);
                hxy += k[[m, i]] * l[[m, i]] * dx * dy / d3;
                dedx += k[[m, i]] * (S::one() - l[[m, i]] / d) * dx;
                dedy += k[[m, i]] * (S::one() - l[[m, i]] / d) * dy;
            }
        }
        let det = hxx * hyy - hxy * hxy;
        let delta_x = (hyy * dedx - hxy * dedy) / det;
        let delta_y = (hxx * dedy - hxy * dedx) / det;
        drawing.raw_entry_mut(m).0 -= delta_x;
        drawing.raw_entry_mut(m).1 -= delta_y;
    }

    /// Runs the Kamada-Kawai algorithm until convergence, or until it has made
    /// `max_moves` node moves.
    ///
    /// This method repeatedly selects the node with the maximum energy gradient
    /// and moves it to reduce the energy.
    ///
    /// Moving one node changes only the springs attached to it, so the
    /// gradients are kept as running sums: each move subtracts the moved node's
    /// springs' old shares from every other node's gradient and adds their new
    /// ones, which costs O(n) rather than the O(n²) of recomputing them all.
    ///
    /// Running sums drift, so neither of their verdicts is taken on trust. The
    /// node they pick is checked from scratch before it moves, at no extra cost
    /// since its gradient falls out of the shares the move needs anyway, and is
    /// skipped if it has in fact settled. When they report convergence, every
    /// gradient is recomputed, O(n²), and the run stops only if that agrees;
    /// otherwise the node that recompute picks moves without a further check.
    /// Skips cannot stall the run: each leaves one more sum exact and under
    /// `eps` and touches no other, so they run out and a recompute follows.
    ///
    /// # Arguments
    ///
    /// * `drawing` - The initial node positions, which will be updated to the final layout
    pub fn run<N>(&self, drawing: &mut DrawingEuclidean2d<N, S>)
    where
        N: DrawingIndex,
        S: DrawingValue,
    {
        let mut gradients = vec![(S::zero(), S::zero()); drawing.len()];
        let mut shares = gradients.clone();
        self.fill_gradients(drawing, &mut gradients);
        let mut moves = 0;
        while moves < self.max_moves {
            let m = match self.steepest(&gradients) {
                Some(m) => {
                    let (dedx, dedy) = self.shares_of(m, drawing, &mut shares);
                    gradients[m] = (dedx, dedy);
                    // As in `steepest`, a NaN gradient reads as settled.
                    let delta2 = dedx * dedx + dedy * dedy;
                    if delta2.is_nan() || delta2 < self.eps * self.eps {
                        continue;
                    }
                    m
                }
                None => {
                    self.fill_gradients(drawing, &mut gradients);
                    match self.steepest(&gradients) {
                        Some(m) => {
                            self.shares_of(m, drawing, &mut shares);
                            m
                        }
                        None => return,
                    }
                }
            };
            for (gradient, share) in gradients.iter_mut().zip(&shares) {
                gradient.0 -= share.0;
                gradient.1 -= share.1;
            }
            self.apply_to_node(m, drawing);
            gradients[m] = self.shares_of(m, drawing, &mut shares);
            for (gradient, share) in gradients.iter_mut().zip(&shares) {
                gradient.0 += share.0;
                gradient.1 += share.1;
            }
            moves += 1;
        }
    }
}

#[test]
fn test_kamada_kawai() {
    use petgraph::Graph;

    let n = 10;
    let mut graph = Graph::new_undirected();
    let nodes = (0..n).map(|_| graph.add_node(())).collect::<Vec<_>>();
    for i in 0..n {
        for j in 0..i {
            graph.add_edge(nodes[j], nodes[i], ());
        }
    }

    let mut coordinates = DrawingEuclidean2d::initial_placement(&graph);

    for &u in &nodes {
        println!("{:?}", coordinates.position(u));
    }

    let kamada_kawai = KamadaKawai::new(&graph, &mut |_| 1.);
    kamada_kawai.run(&mut coordinates);

    for &u in &nodes {
        println!("{:?}", coordinates.position(u));
    }
}

// panschema's, not upstream's. A hang is not a failure, so `run` goes on its own
// thread: with `eps` at zero the stop test can never pass, and a regression fails
// here instead of never finishing.
#[test]
fn run_returns_when_the_layout_cannot_converge() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let graph = graph_from(5, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
        let mut drawing =
            DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
        let mut kamada_kawai = KamadaKawai::new(&graph, |_| 1.0_f32);
        kamada_kawai.eps = 0.0;
        kamada_kawai.run(&mut drawing);
        done.send(()).unwrap();
    });
    match finished.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("run never returned with eps = 0"),
        // The worker died before reporting back: surface its panic, not a hang.
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().unwrap_err())
        }
    }
}

#[cfg(test)]
fn graph_from(n: usize, edges: &[(usize, usize)]) -> petgraph::Graph<(), (), petgraph::Undirected> {
    let mut graph = petgraph::Graph::new_undirected();
    let nodes: Vec<_> = (0..n).map(|_| graph.add_node(())).collect();
    for &(a, b) in edges {
        graph.add_edge(nodes[a], nodes[b], ());
    }
    graph
}

#[test]
fn run_with_no_moves_leaves_the_drawing_unchanged() {
    let graph = graph_from(5, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
    let initial = DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
    let mut drawing =
        DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
    let mut kamada_kawai = KamadaKawai::new(&graph, |_| 1.0_f32);
    kamada_kawai.max_moves = 0;
    kamada_kawai.run(&mut drawing);
    for i in 0..5 {
        assert_eq!(
            (drawing.raw_entry(i).0, drawing.raw_entry(i).1),
            (initial.raw_entry(i).0, initial.raw_entry(i).1),
            "node {i} moved with max_moves = 0"
        );
    }
}

// The default bound is meant to bind only when convergence fails. These shapes,
// at 25 to 30 nodes, need several hundred moves; each must reach convergence
// rather than be cut off.
#[test]
fn the_default_bound_lets_representative_layouts_converge() {
    let n = 30;
    let tree: Vec<_> = (1..n).map(|i| ((i - 1) / 2, i)).collect();
    let mut tree_with_chords = tree.clone();
    for i in (3..n).step_by(7) {
        tree_with_chords.push((i, (i * 5) % n));
    }
    let ring: Vec<_> = (0..n).map(|i| (i, (i + 1) % n)).collect();
    let mut grid = vec![];
    for r in 0..5 {
        for c in 0..5 {
            let v = r * 5 + c;
            if c + 1 < 5 {
                grid.push((v, v + 1));
            }
            if r + 1 < 5 {
                grid.push((v, v + 5));
            }
        }
    }
    for (shape, graph) in [
        ("tree", graph_from(n, &tree)),
        ("tree with chords", graph_from(n, &tree_with_chords)),
        ("ring", graph_from(n, &ring)),
        ("grid", graph_from(25, &grid)),
    ] {
        let mut drawing =
            DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
        let kamada_kawai = KamadaKawai::new(&graph, |_| 1.0_f32);
        kamada_kawai.run(&mut drawing);
        // select_node also reports None when a coordinate is NaN, so convergence
        // means that and finite coordinates.
        assert!(
            kamada_kawai.select_node(&drawing).is_none(),
            "the {shape} was cut off by the default bound before converging"
        );
        assert!(
            (0..drawing.len())
                .all(|i| drawing.raw_entry(i).0.is_finite() && drawing.raw_entry(i).1.is_finite()),
            "the {shape} ended with a non-finite coordinate"
        );
    }
}

// `run` keeps running sums of the gradients; selecting each move from scratch is
// the reference. The sums only steer which node moves next, and each move is
// computed from the drawing itself, so the same picks give the same layout to
// the bit. A different pick would still converge to a nearby layout, so this is
// exact on purpose: the pick sequence is the contract.
// Selecting from scratch evaluates every spring for every move, `run` only the
// moved node's, so it must evaluate a small fraction of the reference's terms.
#[test]
fn run_matches_selecting_from_scratch_at_a_fraction_of_the_cost() {
    let evaluated = |work: &mut dyn FnMut()| {
        PAIR_GRADIENTS.with(|count| count.set(0));
        work();
        PAIR_GRADIENTS.with(|count| count.get())
    };
    let n = 60;
    let tree: Vec<_> = (1..n).map(|i| ((i - 1) / 2, i)).collect();
    let ring: Vec<_> = (0..n).map(|i| (i, (i + 1) % n)).collect();
    for (shape, graph) in [
        ("tree", graph_from(n, &tree)),
        ("ring", graph_from(n, &ring)),
    ] {
        let kamada_kawai = KamadaKawai::new(&graph, |_| 1.0_f32);
        let mut reference =
            DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
        let reference_cost = evaluated(&mut || {
            for _ in 0..kamada_kawai.max_moves {
                match kamada_kawai.select_node(&reference) {
                    Some(m) => kamada_kawai.apply_to_node(m, &mut reference),
                    None => break,
                }
            }
        });
        assert!(
            kamada_kawai.select_node(&reference).is_none(),
            "the {shape} reference did not converge"
        );
        let mut drawing =
            DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&graph);
        let run_cost = evaluated(&mut || kamada_kawai.run(&mut drawing));
        for i in 0..n {
            let (x, y) = (drawing.raw_entry(i).0, drawing.raw_entry(i).1);
            let (rx, ry) = (reference.raw_entry(i).0, reference.raw_entry(i).1);
            assert!(
                x == rx && y == ry,
                "{shape} node {i}: run put it at ({x}, {y}), the reference at ({rx}, {ry})"
            );
        }
        assert!(
            run_cost * 10 < reference_cost,
            "the {shape} cost {run_cost} spring terms against the reference's {reference_cost}"
        );
    }
}
