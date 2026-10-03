// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use super::{Delta, Drawing, DrawingValue, Metric, Shuffle};

/// Stochastic Gradient Descent (SGD) implementation for graph layout algorithms.
///
/// This struct holds node pairs for SGD-based graph layout.
/// It replaces the previous trait-based approach with a concrete implementation that
/// all SGD algorithm variants can use.
///
/// The type parameter `S` represents the scalar type used for calculations
/// (typically `f32` or `f64`).
pub struct Sgd<S> {
    /// List of node pairs to be considered during layout optimization.
    /// Each tuple contains (i, j, distance_ij, distance_ji, weight_ij, weight_ji)
    node_pairs: Vec<(usize, usize, S, S, S, S)>,
}

impl<S> Sgd<S>
where
    S: DrawingValue,
{
    /// Creates a new SGD instance with the given node pairs.
    ///
    /// # Parameters
    /// * `node_pairs` - List of node pairs with distances and weights
    ///
    /// # Returns
    /// A new SGD instance ready for layout optimization
    pub fn new(node_pairs: Vec<(usize, usize, S, S, S, S)>) -> Self {
        Self { node_pairs }
    }

    /// The node pairs, in their current order. Only tests read them.
    #[cfg(test)]
    pub(super) fn node_pairs(&self) -> &[(usize, usize, S, S, S, S)] {
        &self.node_pairs
    }

    /// Creates a scheduler with parameters suitable for this SGD instance.
    ///
    /// This method creates a scheduler that uses eta_min and eta_max calculated
    /// from the current weight distribution in the node pairs.
    ///
    /// # Parameters
    /// * `t_max` - The maximum number of iterations for the scheduler
    /// * `epsilon` - A small value used to calculate eta_min
    ///
    /// # Returns
    /// A scheduler instance configured with appropriate learning rate bounds
    pub fn scheduler<T: super::Scheduler<S>>(&self, t_max: usize, epsilon: S) -> T {
        let (eta_min, eta_max) = self.calculate_eta_bounds(epsilon);
        T::init(t_max, eta_min, eta_max)
    }

    /// Calculates eta_min and eta_max from the current weight distribution.
    fn calculate_eta_bounds(&self, epsilon: S) -> (S, S) {
        let mut w_min = S::infinity();
        let mut w_max = S::zero();
        for &(_, _, _, _, wij, wji) in &self.node_pairs {
            for w in [wij, wji] {
                if w == S::zero() {
                    continue;
                }
                if w < w_min {
                    w_min = w;
                }
                if w > w_max {
                    w_max = w;
                }
            }
        }
        let eta_max = S::one() / w_min;
        let eta_min = epsilon / w_max;
        (eta_min, eta_max)
    }

    /// Randomly shuffles the node pairs to improve convergence.
    ///
    /// SGD algorithms typically process node pairs in a random order to avoid
    /// getting stuck in local minima. This method randomizes the order using
    /// the provided random number generator.
    pub fn shuffle<R: Shuffle>(&mut self, rng: &mut R) {
        rng.shuffle(&mut self.node_pairs);
    }

    /// Applies the SGD force calculations to the drawing, moving nodes toward their optimal positions.
    ///
    /// This is the core method that performs a single iteration of the SGD algorithm.
    /// The eta parameter is expected to be the actual learning rate (not normalized),
    /// typically coming from a scheduler that was created using this SGD instance.
    ///
    /// For each node pair (i, j):
    /// 1. Calculates the learning rate factors (mu_i, mu_j) based on weights and the learning rate
    /// 2. Computes the displacement vectors based on the difference between current and target distances
    /// 3. Moves the nodes according to the calculated forces
    ///
    /// # Parameters
    /// * `drawing` - The current node position drawing to update
    /// * `eta` - The current learning rate (from scheduler)
    pub fn apply<Diff, D, M>(&self, drawing: &mut D, eta: S)
    where
        D: Drawing<Item = M>,
        Diff: Delta<S = S>,
        M: Metric<D = Diff>,
    {
        for &(i, j, dij, dji, wij, wji) in &self.node_pairs {
            let mu_i = (eta * wij).min(S::one());
            let mu_j = (eta * wji).min(S::one());
            let delta = drawing.delta(i, j);
            let norm = delta.norm();
            if norm > S::zero() {
                let r_i = S::from_f32(0.5).unwrap() * (norm - dij) / norm;
                let r_j = S::from_f32(0.5).unwrap() * (norm - dji) / norm;
                *drawing.raw_entry_mut(i) += delta.clone() * -r_i * mu_i;
                *drawing.raw_entry_mut(j) += delta.clone() * r_j * mu_j;
            }
        }
    }
}

// panschema's tests, not upstream's.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::algo::test_support::{graph_from, placed, positions};
    use crate::layout::algo::{DrawingEuclidean2d, Scheduler};
    use petgraph::graph::NodeIndex;

    /// A scheduler that only records what `Sgd::scheduler` initialized it with.
    struct Recorded {
        t_max: usize,
        eta_min: f32,
        eta_max: f32,
    }

    impl Scheduler<f32> for Recorded {
        fn init(t_max: usize, eta_min: f32, eta_max: f32) -> Self {
            Self {
                t_max,
                eta_min,
                eta_max,
            }
        }
        fn step<F: FnMut(f32)>(&mut self, _: &mut F) {}
        fn is_finished(&self) -> bool {
            true
        }
    }

    #[test]
    fn the_scheduler_spans_the_learning_rates_the_weights_allow() {
        // The largest step any pair can take without overshooting is 1/w_min,
        // the smallest worth taking epsilon/w_max. Zero weights are skipped.
        let sgd = Sgd::new(vec![
            (0, 1, 2.0, 2.0, 0.25, 0.25),
            (0, 2, 1.0, 1.0, 1.0, 0.0),
            (1, 2, 0.5, 0.5, 4.0, 4.0),
        ]);
        let scheduler: Recorded = sgd.scheduler(15, 0.1);
        assert_eq!(scheduler.t_max, 15);
        assert_eq!(scheduler.eta_max, 4.0, "1 / w_min");
        assert_eq!(scheduler.eta_min, 0.025, "epsilon / w_max");
    }

    fn two_nodes(a: (f32, f32), b: (f32, f32)) -> DrawingEuclidean2d<NodeIndex, f32> {
        placed(&graph_from(2, &[(0, 1)]), &[a, b])
    }

    #[test]
    fn a_full_step_closes_the_gap_to_the_target_distance() {
        // 4 apart, target 2: each node moves half the error toward the other.
        let mut drawing = two_nodes((0.0, 0.0), (4.0, 0.0));
        Sgd::new(vec![(0, 1, 2.0, 2.0, 1.0, 1.0)]).apply(&mut drawing, 10.0);
        assert_eq!(positions(&drawing), [(1.0, 0.0), (3.0, 0.0)]);
    }

    #[test]
    fn a_smaller_learning_rate_takes_a_proportional_step() {
        // eta * w = 0.25 * 2 = 0.5, so each node goes half as far as a full
        // step. A weight other than 1 tells eta * w apart from eta / w.
        let mut drawing = two_nodes((0.0, 0.0), (4.0, 0.0));
        Sgd::new(vec![(0, 1, 2.0, 2.0, 2.0, 2.0)]).apply(&mut drawing, 0.25);
        assert_eq!(positions(&drawing), [(0.5, 0.0), (3.5, 0.0)]);
    }

    #[test]
    fn each_end_moves_toward_its_own_target_distance() {
        // From j's side the pair is already at its target, so only i moves;
        // and with a zero weight on j's side, j cannot move at all.
        let mut drawing = two_nodes((0.0, 0.0), (0.0, 4.0));
        Sgd::new(vec![(0, 1, 2.0, 4.0, 1.0, 1.0)]).apply(&mut drawing, 10.0);
        assert_eq!(positions(&drawing), [(0.0, 1.0), (0.0, 4.0)]);
        let mut drawing = two_nodes((0.0, 0.0), (0.0, 4.0));
        Sgd::new(vec![(0, 1, 2.0, 2.0, 1.0, 0.0)]).apply(&mut drawing, 10.0);
        assert_eq!(positions(&drawing), [(0.0, 1.0), (0.0, 4.0)]);
    }

    #[test]
    fn coincident_nodes_are_left_where_they_are() {
        // With no direction between them there is no step to take, and
        // dividing by the zero distance would make both NaN.
        let mut drawing = two_nodes((1.0, 1.0), (1.0, 1.0));
        Sgd::new(vec![(0, 1, 2.0, 2.0, 1.0, 1.0)]).apply(&mut drawing, 10.0);
        assert_eq!(positions(&drawing), [(1.0, 1.0), (1.0, 1.0)]);
    }

    #[test]
    fn shuffle_reorders_the_pairs_with_the_given_generator() {
        struct Reverse;
        impl Shuffle for Reverse {
            fn shuffle<T>(&mut self, items: &mut [T]) {
                items.reverse();
            }
        }
        let mut sgd = Sgd::new(vec![(0, 1, 1.0, 1.0, 1.0, 1.0), (0, 2, 2.0, 2.0, 1.0, 1.0)]);
        sgd.shuffle(&mut Reverse);
        let order: Vec<_> = sgd.node_pairs().iter().map(|p| (p.0, p.1)).collect();
        assert_eq!(order, [(0, 2), (0, 1)]);
    }
}
