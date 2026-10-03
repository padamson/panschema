// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

//! Stress Majorization implementation for graph layout.
//!
//! This crate provides an implementation of the Stress Majorization algorithm,
//! a force-directed graph layout technique that minimizes a stress function.
//! The stress function measures the difference between the Euclidean distances
//! in the layout and the desired or theoretical distances (typically shortest path
//! distances in the graph).
//!
//! # Algorithm
//!
//! Stress Majorization works by iteratively solving a sequence of quadratic problems
//! that approximate the stress function. Each iteration improves the layout by
//! moving nodes to positions that reduce the overall stress.
//!
//! The algorithm implemented here is based on:
//!
//! Gansner, E. R., Koren, Y., & North, S. (2004). Graph drawing by stress
//! majorization. In International Symposium on Graph Drawing (pp. 239-250).
//!
//! # Usage
//!
//! Not run as a doctest: this module is private, so rustdoc would compile
//! the example as an external crate that cannot reach it.
//!
//! ```ignore
//! use petgraph::prelude::*;
//! use crate::layout::algo::{DrawingEuclidean2d, StressMajorization};
//!
//! // Create a graph
//! let mut graph = Graph::new_undirected();
//! let n1 = graph.add_node(());
//! let n2 = graph.add_node(());
//! let n3 = graph.add_node(());
//! graph.add_edge(n1, n2, ());
//! graph.add_edge(n2, n3, ());
//!
//! // Create initial placement
//! let mut drawing = DrawingEuclidean2d::initial_placement(&graph);
//!
//! // Run stress majorization
//! let mut sm = StressMajorization::new(&graph, &drawing, |_| 1.0);
//! sm.run(&mut drawing);
//! ```

use super::{DistanceMatrix, FullDistanceMatrix, all_sources_dijkstra};
use super::{Drawing, DrawingEuclidean2d, DrawingIndex, DrawingValue, MetricEuclidean2d};
use ndarray::prelude::*;
use petgraph::visit::{IntoEdges, IntoNodeIdentifiers, NodeCount};

/// Computes the optimal step length (alpha) in the conjugate gradient method.
///
/// The line search finds the value of alpha that minimizes the function value
/// when moving in the direction d.
///
/// # Arguments
///
/// * `a` - The coefficient matrix
/// * `dx` - The gradient vector
/// * `d` - The search direction
///
/// # Returns
///
/// The optimal step length alpha
fn line_search<S: DrawingValue>(a: &Array2<S>, dx: &Array1<S>, d: &Array1<S>) -> S {
    let n = dx.len();
    let mut alpha = -d.dot(dx);
    let mut s = S::zero();
    for i in 0..n {
        for j in 0..n {
            s += d[i] * d[j] * a[[i, j]];
        }
    }
    // No direction to search along, as when the system is already solved:
    // the step is zero, not 0/0.
    if s == S::zero() {
        return S::zero();
    }
    alpha /= s;
    alpha
}

/// Computes the gradient (delta_f) of the quadratic function f(x) = (1/2)x^T A x - b^T x.
///
/// # Arguments
///
/// * `a` - The coefficient matrix A
/// * `b` - The vector b
/// * `x` - The current position x
/// * `dx` - Output parameter where the gradient will be stored
fn delta_f<S: DrawingValue>(a: &Array2<S>, b: &Array1<S>, x: &Array1<S>, dx: &mut Array1<S>) {
    let n = b.len();
    for i in 0..n {
        dx[i] = S::zero();
        for j in 0..n {
            dx[i] += a[[i, j]] * x[j];
        }
        dx[i] -= b[i];
    }
}

/// Solves a system of linear equations Ax = b using the conjugate gradient method.
///
/// The conjugate gradient method is an iterative algorithm for solving systems
/// of the form Ax = b where A is a symmetric positive-definite matrix.
///
/// # Arguments
///
/// * `a` - The coefficient matrix A
/// * `b` - The right-hand side vector b
/// * `x` - The initial guess for x, which will be updated with the solution
/// * `epsilon` - The convergence threshold (algorithm stops when the squared residual norm is less than this value)
pub fn conjugate_gradient<S: DrawingValue>(
    a: &Array2<S>,
    b: &Array1<S>,
    x: &mut Array1<S>,
    epsilon: S,
) {
    let n = b.len();
    let mut dx = Array1::zeros(n);
    let mut d = Array1::zeros(n);
    delta_f(a, b, x, &mut dx);
    for i in 0..n {
        d[i] = -dx[i];
    }
    let mut dx_norm0 = dx.dot(&dx);
    for _ in 0..n {
        let alpha = line_search(a, &dx, &d);
        for i in 0..n {
            x[i] += alpha * d[i];
        }
        delta_f(a, b, x, &mut dx);
        let dx_norm = dx.dot(&dx);
        if dx_norm < epsilon {
            break;
        }
        let beta = dx_norm / dx_norm0;
        dx_norm0 = dx_norm;
        for i in 0..n {
            d[i] = beta * d[i] - dx[i];
        }
    }
}

/// Computes the stress value for the current layout.
///
/// The stress is defined as the weighted sum of squared differences between
/// the Euclidean distances in the layout and the desired distances.
///
/// # Arguments
///
/// * `x` - The x-coordinates of nodes
/// * `y` - The y-coordinates of nodes
/// * `w` - The weight matrix
/// * `d` - The desired distance matrix
///
/// # Returns
///
/// The stress value
fn stress<S: DrawingValue>(x: &Array1<S>, y: &Array1<S>, w: &Array2<S>, d: &Array2<S>) -> S {
    let n = x.len() + 1;
    let mut s = S::zero();
    for j in 1..n - 1 {
        for i in 0..j {
            let dx = x[i] - x[j];
            let dy = y[i] - y[j];
            let norm = (dx * dx + dy * dy).sqrt();
            let dij = d[[i, j]];
            let wij = w[[i, j]];
            let e = norm - dij;
            s += wij * e * e;
        }
    }
    for i in 0..n - 1 {
        let j = n - 1;
        let dx = x[i];
        let dy = y[i];
        let norm = (dx * dx + dy * dy).sqrt();
        let dij = d[[i, j]];
        let wij = w[[i, j]];
        let e = norm - dij;
        s += wij * e * e;
    }
    s
}

/// An implementation of the Stress Majorization algorithm for graph layout.
///
/// Stress Majorization is a force-directed technique that iteratively minimizes
/// a stress function by solving a series of simpler quadratic problems.
/// This implementation supports 2D layouts with weighted edges.
pub struct StressMajorization<S> {
    d: Array2<S>,
    w: Array2<S>,
    l_w: Array2<S>,
    l_z: Array2<S>,
    b: Array1<S>,
    stress: S,
    x_x: Array1<S>,
    x_y: Array1<S>,
    pub epsilon: S,
    pub max_iterations: usize,
}

impl<S> StressMajorization<S>
where
    S: DrawingValue,
{
    /// Creates a new `StressMajorization` instance from a graph and an initial drawing.
    ///
    /// # Arguments
    ///
    /// * `graph` - The input graph
    /// * `drawing` - The initial node positions
    /// * `length` - A function that returns the desired length for each edge
    ///
    /// # Returns
    ///
    /// A new `StressMajorization` instance
    pub fn new<G, F>(graph: G, drawing: &DrawingEuclidean2d<G::NodeId, S>, length: F) -> Self
    where
        G: IntoEdges + IntoNodeIdentifiers + NodeCount,
        G::NodeId: DrawingIndex + Ord,
        F: FnMut(G::EdgeRef) -> S,
    {
        let d = all_sources_dijkstra(graph, length);
        StressMajorization::new_with_distance_matrix(drawing, &d)
    }

    /// Creates a new `StressMajorization` instance using a pre-computed distance matrix.
    ///
    /// This is useful when you already have shortest path distances available,
    /// or want to use custom distances instead of computing them from the graph.
    ///
    /// # Arguments
    ///
    /// * `drawing` - The initial node positions
    /// * `distance_matrix` - The pre-computed distance matrix
    ///
    /// # Returns
    ///
    /// A new `StressMajorization` instance
    pub fn new_with_distance_matrix<N>(
        drawing: &DrawingEuclidean2d<N, S>,
        distance_matrix: &FullDistanceMatrix<N, S>,
    ) -> Self
    where
        N: DrawingIndex,
    {
        let n = drawing.len();
        let mut d = Array2::zeros((n, n));
        let w = Array2::zeros((n, n));
        let l_w = Array2::zeros((n - 1, n - 1));
        let mut x_x = Array1::zeros(n - 1);
        let mut x_y = Array1::zeros(n - 1);
        for i in 0..n - 1 {
            x_x[i] = drawing.raw_entry(i).0 - drawing.raw_entry(n - 1).0;
            x_y[i] = drawing.raw_entry(i).1 - drawing.raw_entry(n - 1).1;
        }
        for i in 0..n {
            for j in 0..n {
                d[[i, j]] = distance_matrix.get_by_index(i, j);
            }
        }

        let epsilon = (1e-4).into();
        let max_iterations = 100; // Default value
        let l_z = Array2::zeros((n - 1, n - 1));
        let b = Array1::zeros(n - 1);
        let mut sm = StressMajorization {
            b,
            d,
            l_w,
            l_z,
            w,
            x_x,
            x_y,
            stress: S::infinity(),
            epsilon,
            max_iterations,
        };
        sm.update_weight(|_, _, dij, _| S::one() / (dij * dij));
        sm
    }

    /// Performs a single iteration of the stress majorization algorithm.
    ///
    /// This function updates the node positions in the drawing to reduce the stress.
    ///
    /// # Arguments
    ///
    /// * `drawing` - The current node positions, which will be updated
    ///
    /// # Returns
    ///
    /// The relative change in stress (as a fraction of the previous stress value)
    pub fn apply<N>(&mut self, drawing: &mut DrawingEuclidean2d<N, S>) -> S
    where
        N: DrawingIndex,
    {
        let n = drawing.len();
        let StressMajorization {
            b, d, l_w, l_z, w, ..
        } = self;
        for i in 0..n {
            let MetricEuclidean2d(x, y) = *drawing.raw_entry(n - 1);
            drawing.raw_entry_mut(i).0 -= x;
            drawing.raw_entry_mut(i).1 -= y;
        }
        for i in 1..n - 1 {
            for j in 0..i {
                let dx = drawing.raw_entry(i).0 - drawing.raw_entry(j).0;
                let dy = drawing.raw_entry(i).1 - drawing.raw_entry(j).1;
                let norm = (dx * dx + dy * dy).sqrt();
                let lij = if norm < (1e-4).into() {
                    S::zero()
                } else {
                    -w[[i, j]] * d[[i, j]] / norm
                };
                l_z[[i, j]] = lij;
                l_z[[j, i]] = lij;
            }
        }
        for i in 0..n - 1 {
            let mut s = S::zero();
            for j in 0..n - 1 {
                if i != j {
                    s -= l_z[[i, j]];
                }
            }
            let j = n - 1;
            let dx = drawing.raw_entry(i).0;
            let dy = drawing.raw_entry(i).1;
            let norm = (dx * dx + dy * dy).sqrt();
            s -= if norm < (1e-4).into() {
                S::zero()
            } else {
                -w[[i, j]] * d[[i, j]] / norm
            };
            l_z[[i, i]] = s;
        }

        for i in 0..n - 1 {
            self.x_x[i] = drawing.raw_entry(i).0;
            let mut s = S::zero();
            for j in 0..n - 1 {
                s += l_z[[i, j]] * drawing.raw_entry(j).0;
            }
            b[i] = s;
        }
        conjugate_gradient(l_w, b, &mut self.x_x, self.epsilon);

        for i in 0..n - 1 {
            self.x_y[i] = drawing.raw_entry(i).1;
            let mut s = S::zero();
            for j in 0..n - 1 {
                s += l_z[[i, j]] * drawing.raw_entry(j).1;
            }
            b[i] = s;
        }
        conjugate_gradient(l_w, b, &mut self.x_y, self.epsilon);

        let stress = stress(&self.x_x, &self.x_y, w, d);
        // At zero stress there is nothing left to gain, and the ratio below
        // would be 0/0.
        let diff = if self.stress == S::zero() {
            S::zero()
        } else {
            (self.stress - stress) / self.stress
        };
        self.stress = stress;
        for i in 0..n - 1 {
            drawing.raw_entry_mut(i).0 = self.x_x[i];
            drawing.raw_entry_mut(i).1 = self.x_y[i];
        }
        diff
    }

    /// Runs the stress majorization algorithm until convergence.
    ///
    /// This function repeatedly applies the algorithm until the relative
    /// change in stress falls below the threshold specified by `epsilon`
    /// or the maximum number of iterations is reached.
    ///
    /// # Arguments
    ///
    /// * `coordinates` - The current node positions, which will be updated
    pub fn run<N>(&mut self, coordinates: &mut DrawingEuclidean2d<N, S>)
    where
        N: DrawingIndex,
    {
        for _ in 0..self.max_iterations {
            if self.apply(coordinates) < self.epsilon {
                break;
            }
        }
    }

    /// Updates the weights used in the stress calculation.
    ///
    /// This allows customizing how different pairs of nodes contribute to the overall
    /// stress. By default, weights are set to 1/(d_ij^2) where d_ij is the desired distance.
    ///
    /// # Arguments
    ///
    /// * `weight` - A function that takes (i, j, d_ij, current_w_ij) and returns the new weight
    pub fn update_weight<F>(&mut self, mut weight: F)
    where
        F: FnMut(usize, usize, S, S) -> S,
    {
        let n = self.x_x.len() + 1;

        for j in 1..n {
            for i in 0..j {
                let wij = weight(i, j, self.d[[i, j]], self.w[[i, j]]);
                self.w[[i, j]] = wij;
                self.w[[j, i]] = wij;
            }
        }

        for i in 0..n - 1 {
            self.l_w[[i, i]] = S::zero();
        }
        for j in 1..n - 1 {
            for i in 0..j {
                let wij = self.w[[i, j]];
                self.l_w[[i, j]] = -wij;
                self.l_w[[j, i]] = -wij;
                self.l_w[[i, i]] += wij;
                self.l_w[[j, j]] += wij;
            }
        }
        for i in 0..n - 1 {
            let j = n - 1;
            self.l_w[[i, i]] += self.w[[i, j]];
        }
        self.stress = stress(&self.x_x, &self.x_y, &self.w, &self.d);
    }
}

#[test]
fn test_conjugate_gradient() {
    let a = arr2(&[[3., 1.], [1., 2.]]);
    let b = arr1(&[6., 7.]);
    let mut x = arr1(&[2., 1.]);
    let epsilon = 1e-4;
    conjugate_gradient(&a, &b, &mut x, epsilon);
    let x_exact = [1., 3.];
    let mut d = 0.;
    for i in 0..x.len() {
        let dx = x[i] - x_exact[i];
        d += dx * dx;
    }
    assert!(d < epsilon);
}

#[test]
fn test_stress_majorization() {
    use petgraph::Graph;

    let n = 10;
    let mut graph = Graph::new_undirected();
    let nodes = (0..n).map(|_| graph.add_node(())).collect::<Vec<_>>();
    for j in 1..n {
        for i in 0..j {
            graph.add_edge(nodes[i], nodes[j], ());
        }
    }
    let mut coordinates = DrawingEuclidean2d::initial_placement(&graph);

    for &u in &nodes {
        println!("{:?}", coordinates.position(u));
    }

    let mut stress_majorization = StressMajorization::new(&graph, &coordinates, &mut |_| 1.);
    stress_majorization.run(&mut coordinates);

    for &u in &nodes {
        println!("{:?}", coordinates.position(u));
    }
}

#[test]
fn test_stress_majorization_parameters() {
    use petgraph::Graph;

    // Create a simple graph
    let mut graph = Graph::new_undirected();
    let n1 = graph.add_node(());
    let n2 = graph.add_node(());
    let n3 = graph.add_node(());
    graph.add_edge(n1, n2, ());
    graph.add_edge(n2, n3, ());

    let coordinates = DrawingEuclidean2d::initial_placement(&graph);

    // Default parameters
    let mut stress_majorization = StressMajorization::new(&graph, &coordinates, &mut |_| 1.0);

    // Check default values. The constructor builds epsilon as `(1e-4).into()`,
    // and `DrawingValue` only guarantees `From<f32>`, so the literal is an f32
    // widened to S — upstream's `== 1e-4` compared it to an f64 and never held.
    assert_eq!(stress_majorization.epsilon, f64::from(1e-4_f32));
    assert_eq!(stress_majorization.max_iterations, 100);

    // Update parameters
    stress_majorization.epsilon = 1e-6;
    stress_majorization.max_iterations = 200;

    // Check updated values
    assert_eq!(stress_majorization.epsilon, 1e-6);
    assert_eq!(stress_majorization.max_iterations, 200);
}

// panschema's tests. A path's stress optimum is exact under any weights and
// stress majorization converges to it however it stops, so most of the
// algorithm is invisible to layout-level tests; these pin the pieces directly.

#[cfg(test)]
use super::test_support::{graph_from, path, placed, positions};

/// Stress straight from its definition: the sum over pairs of w·(|pᵢ − pⱼ| − dᵢⱼ)².
#[cfg(test)]
fn reference_stress(
    points: &[(f64, f64)],
    d: impl Fn(usize, usize) -> f64,
    w: impl Fn(usize, usize) -> f64,
) -> f64 {
    let mut s = 0.0;
    for j in 1..points.len() {
        for i in 0..j {
            let e = (points[i].0 - points[j].0).hypot(points[i].1 - points[j].1) - d(i, j);
            s += w(i, j) * e * e;
        }
    }
    s
}

// `stress` stores every node relative to the last one, which sits at the
// origin; distinct weights and distances keep each term's arithmetic visible.
#[test]
fn stress_is_the_weighted_sum_of_squared_distance_errors() {
    let points = [(1.0, 1.0), (4.0, 5.0), (0.0, 0.0)];
    let d = arr2(&[[0.0, 2.0, 1.0], [2.0, 0.0, 3.0], [1.0, 3.0, 0.0]]);
    let w = arr2(&[[0.0, 2.0, 3.0], [2.0, 0.0, 0.5], [3.0, 0.5, 0.0]]);
    let x = arr1(&[points[0].0, points[1].0]);
    let y = arr1(&[points[0].1, points[1].1]);
    let expected = reference_stress(&points, |i, j| d[[i, j]], |i, j| w[[i, j]]);
    assert!((stress(&x, &y, &w, &d) - expected).abs() < 1e-12);
}

#[test]
fn pairs_are_weighted_by_their_inverse_square_distance() {
    let graph = path(3);
    let drawing = placed(&graph, &[(0.0_f64, 0.0); 3]);
    let sm = StressMajorization::new(&graph, &drawing, |_| 1.0);
    assert_eq!((sm.w[[0, 1]], sm.w[[0, 2]], sm.w[[1, 2]]), (1.0, 0.25, 1.0));
}

// `apply` returns how much one step lowered the stress, as a fraction of what
// it was, which is what `run` compares against `epsilon`.
#[test]
fn apply_reports_the_relative_drop_in_stress() {
    let graph = path(3);
    let start = [(0.0, 0.0), (3.0, 0.5), (1.0, 2.0)];
    let mut drawing = placed(&graph, &start);
    let mut sm = StressMajorization::new(&graph, &drawing, |_| 1.0);
    let path_distance = |i: usize, j: usize| (j - i) as f64;
    let weight = |i: usize, j: usize| 1.0 / ((j - i) as f64).powi(2);
    let before = reference_stress(&start, path_distance, weight);
    let reported = sm.apply(&mut drawing);
    let after = reference_stress(&positions(&drawing), path_distance, weight);
    assert!(
        after < before,
        "the step raised stress from {before} to {after}"
    );
    assert!(
        (reported - (before - after) / before).abs() < 1e-9,
        "reported {reported}, stress went from {before} to {after}"
    );
}

// Each step solves for new positions in the frame where the last node is the
// origin. Near the optimum the solution is near where the nodes already are;
// a sign error in the right-hand side mirrors them through the last node,
// which keeps every distance and so every distance-based check.
#[test]
fn a_step_from_near_the_optimum_stays_near_it() {
    // On a diagonal, so a mirror image shows on both axes.
    let graph = path(3);
    let start = [(-1.25_f64, -1.55), (-0.58, -0.83), (0.0, 0.0)];
    let mut drawing = placed(&graph, &start);
    let mut sm = StressMajorization::new(&graph, &drawing, |_| 1.0);
    sm.apply(&mut drawing);
    for (i, &(x, y)) in start.iter().enumerate() {
        let (nx, ny) = (drawing.raw_entry(i).0, drawing.raw_entry(i).1);
        assert!(
            (nx - x).hypot(ny - y) < 0.1,
            "node {i} went from ({x}, {y}) to ({nx}, {ny})"
        );
    }
}

// `run` stops when a step lowers stress by less than `epsilon` of what it
// was. A graph that can be drawn with zero stress, like a path, keeps losing
// about the same fraction each step and runs to the bound, so this uses a
// star whose three leaves cannot all be 2 apart. `epsilon` also stops each
// step's conjugate-gradient solve, so the default is kept and `run` is
// compared with stepping by hand.
#[test]
fn run_stops_at_the_first_step_that_gains_less_than_epsilon() {
    let star = graph_from(4, &[(0, 1), (0, 2), (0, 3)]);
    let start = [(0.0_f64, 0.0), (3.0, 0.5), (1.0, 2.0), (-1.0, 0.5)];
    let mut drawing = placed(&star, &start);
    StressMajorization::new(&star, &drawing, |_| 1.0).run(&mut drawing);
    let mut by_hand = placed(&star, &start);
    let mut sm = StressMajorization::new(&star, &by_hand, |_| 1.0);
    let mut steps = 0;
    while steps < sm.max_iterations {
        steps += 1;
        if sm.apply(&mut by_hand) < sm.epsilon {
            break;
        }
    }
    assert!(steps > 1, "the first step already gained less than epsilon");
    assert!(steps < sm.max_iterations, "converged only at the bound");
    for i in 0..4 {
        assert_eq!(
            (drawing.raw_entry(i).0, drawing.raw_entry(i).1),
            (by_hand.raw_entry(i).0, by_hand.raw_entry(i).1),
            "node {i} after {steps} steps"
        );
    }
}

#[test]
fn coincident_nodes_do_not_make_the_layout_non_finite() {
    // Two nodes at the same point have no direction between them; the step
    // must leave that pair's term out rather than divide by zero.
    // The last node is the frame's origin, so a node on top of it is a case
    // of its own.
    let graph = path(3);
    for (case, start) in [
        ("two free nodes", [(1.0_f64, 1.0), (1.0, 1.0), (0.0, 0.0)]),
        (
            "a node on the last one",
            [(0.0, 0.0), (2.0, 1.0), (0.0, 0.0)],
        ),
    ] {
        let mut drawing = placed(&graph, &start);
        let mut sm = StressMajorization::new(&graph, &drawing, |_| 1.0);
        sm.apply(&mut drawing);
        assert!(
            (0..3)
                .all(|i| drawing.raw_entry(i).0.is_finite() && drawing.raw_entry(i).1.is_finite()),
            "{case}"
        );
    }
}

#[test]
fn conjugate_gradient_leaves_an_exact_solution_alone() {
    // The residual is zero from the start, so the first line search has no
    // direction and must take a zero step rather than divide zero by zero.
    let a = arr2(&[[3.0_f64, 1.0], [1.0, 2.0]]);
    let b = arr1(&[6.0, 7.0]);
    let mut x = arr1(&[1.0, 3.0]);
    conjugate_gradient(&a, &b, &mut x, 1e-4);
    assert_eq!(x.to_vec(), [1.0, 3.0]);
}

// A layout already at zero stress: two tests, since the step's return value
// and the coordinates are kept finite by separate fixes.
#[test]
fn a_step_from_zero_stress_reports_no_gain() {
    let edge = path(2);
    let mut drawing = placed(&edge, &[(0.0_f64, 0.0), (1.0, 0.0)]);
    let mut sm = StressMajorization::new(&edge, &drawing, |_| 1.0);
    assert_eq!(sm.apply(&mut drawing), 0.0);
}

#[test]
fn a_step_from_zero_stress_leaves_the_layout_where_it_is() {
    let edge = path(2);
    let mut drawing = placed(&edge, &[(0.0_f64, 0.0), (1.0, 0.0)]);
    StressMajorization::new(&edge, &drawing, |_| 1.0).apply(&mut drawing);
    // Every step reports the layout in the frame with the last node at the
    // origin, so "unchanged" is the same pair shifted there.
    assert_eq!(positions(&drawing), [(-1.0, 0.0), (0.0, 0.0)]);
}

// One step solves a lone edge exactly. The run must then stop, with a further
// step gaining nothing, rather than step from a zero residual into NaN. In
// f32 from the spiral start, as the layouts run it, that is what happened.
#[test]
fn run_lays_out_a_lone_edge_at_its_length() {
    let edge = path(2);
    let mut drawing =
        DrawingEuclidean2d::<petgraph::graph::NodeIndex, f32>::initial_placement(&edge);
    let mut sm = StressMajorization::new(&edge, &drawing, |_| 1.0_f32);
    sm.run(&mut drawing);
    let [(ax, ay), (bx, by)] = positions(&drawing)[..] else {
        unreachable!()
    };
    assert!(
        ((ax - bx).hypot(ay - by) - 1.0).abs() < 1e-5,
        "nodes at ({ax}, {ay}) and ({bx}, {by})"
    );
    assert_eq!(
        sm.apply(&mut drawing),
        0.0,
        "a further step gained something"
    );
}
