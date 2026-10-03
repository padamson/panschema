// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use super::DrawingValue;
use super::Scheduler;

/// A learning rate scheduler with exponential decay.
///
/// This scheduler decreases the learning rate exponentially over time,
/// following the formula: η(t) = a * exp(-b * t), where:
/// a = eta_max, b = log(eta_max / eta_min) / (t_max - 1).
///
/// Exponential decay creates a learning rate that decreases quickly at first
/// and then more gradually, which can help achieve faster convergence in many cases.
pub struct SchedulerExponential<S> {
    /// Current iteration counter
    t: usize,
    /// Maximum number of iterations
    t_max: usize,
    /// Initial learning rate (a parameter)
    a: S,
    /// Decay factor (b parameter)
    b: S,
}

/// Implementation of the Scheduler trait for SchedulerExponential
impl<S> Scheduler<S> for SchedulerExponential<S>
where
    S: DrawingValue,
{
    /// Initializes a new exponential scheduler.
    ///
    /// This method calculates the parameters for the exponential decay formula
    /// based on the desired minimum and maximum learning rates and the number of iterations.
    ///
    /// # Parameters
    /// * `t_max` - The maximum number of iterations
    /// * `eta_min` - The minimum learning rate (reached at the end)
    /// * `eta_max` - The maximum learning rate (used at the beginning)
    ///
    /// # Returns
    /// A new SchedulerExponential instance
    fn init(t_max: usize, eta_min: S, eta_max: S) -> Self {
        let a = eta_max;
        let b = if t_max == 1 {
            S::zero()
        } else {
            (eta_max / eta_min).ln() / S::from_usize(t_max - 1).unwrap()
        };
        Self { t: 0, t_max, a, b }
    }

    /// Performs a single step of the scheduling process.
    ///
    /// This method calculates the learning rate using the exponential decay formula,
    /// provides it to the callback function, and increments the iteration counter.
    ///
    /// # Parameters
    /// * `callback` - A function that will be called with the calculated learning rate
    fn step<F: FnMut(S)>(&mut self, callback: &mut F) {
        let eta = self.a * (-self.b * S::from_usize(self.t).unwrap()).exp();
        callback(eta);
        self.t += 1;
    }

    /// Checks if the scheduling process is complete.
    ///
    /// # Returns
    /// `true` if the current iteration count has reached the maximum, `false` otherwise
    fn is_finished(&self) -> bool {
        self.t >= self.t_max
    }
}

// panschema's tests, not upstream's.
#[cfg(test)]
mod tests {
    use super::*;

    fn etas(t_max: usize, eta_min: f32, eta_max: f32) -> Vec<f32> {
        let mut scheduler = SchedulerExponential::init(t_max, eta_min, eta_max);
        let mut etas = vec![];
        scheduler.run(&mut |eta| etas.push(eta));
        etas
    }

    #[test]
    fn decays_geometrically_from_eta_max_to_eta_min_in_t_max_steps() {
        // (0.01 / 100)^(1/4) = 0.1, so each step is a tenth of the last.
        let etas = etas(5, 0.01, 100.0);
        assert_eq!(etas.len(), 5);
        let expected = [100.0, 10.0, 1.0, 0.1, 0.01];
        for (eta, want) in etas.iter().zip(expected) {
            assert!((eta / want - 1.0).abs() < 1e-5, "etas {etas:?}");
        }
    }

    #[test]
    fn a_single_step_uses_eta_max() {
        assert_eq!(etas(1, 0.01, 100.0), [100.0]);
    }

    #[test]
    fn finishes_after_exactly_t_max_steps() {
        let mut scheduler = SchedulerExponential::init(3, 0.1_f32, 1.0);
        for step in 0..3 {
            assert!(!scheduler.is_finished(), "finished before step {step}");
            scheduler.step(&mut |_| {});
        }
        assert!(scheduler.is_finished());
    }
}
