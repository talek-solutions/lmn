use rand::Rng;
use rand::SeedableRng;

use crate::stats::Distribution;

// ── NumericHistogramParams ────────────────────────────────────────────────────

/// Parameters for constructing a `NumericHistogram`.
pub struct NumericHistogramParams {
    /// Maximum number of samples to retain.
    /// Once this cap is reached, reservoir sampling is used.
    pub max_samples: usize,
}

// ── NumericHistogram ──────────────────────────────────────────────────────────

/// Bounded reservoir of float64 samples for response field statistics.
///
/// Fills up to `max_samples`, then applies Vitter's Algorithm R reservoir
/// sampling so the retained set remains a uniform random sample of all
/// observed values.
///
/// Uses `SmallRng` (a `Send`-safe PRNG) seeded from entropy at construction
/// time, so instances can be sent across threads (e.g. moved into drain tasks).
pub struct NumericHistogram {
    samples: Vec<f64>,
    max_samples: usize,
    total_seen: usize,
    rng: rand::rngs::SmallRng,
}

impl NumericHistogram {
    /// Creates a new histogram with the given parameters.
    pub fn new(params: NumericHistogramParams) -> Self {
        Self {
            samples: Vec::new(),
            max_samples: params.max_samples,
            total_seen: 0,
            rng: rand::rngs::SmallRng::from_os_rng(),
        }
    }

    /// Records a single float value.
    pub fn record(&mut self, value: f64) {
        self.total_seen += 1;
        if self.samples.len() < self.max_samples {
            self.samples.push(value);
        } else {
            let j = self.rng.random_range(0..self.total_seen);
            if j < self.max_samples {
                self.samples[j] = value;
            }
        }
    }

    /// Returns a sorted `Distribution` over the retained samples.
    pub fn distribution(&self) -> Distribution {
        Distribution::from_unsorted(self.samples.clone())
    }

    /// Returns `true` if no values have been recorded.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Returns the total number of values observed (including those not retained).
    pub fn total_seen(&self) -> usize {
        self.total_seen
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn hist_with_cap(cap: usize) -> NumericHistogram {
        NumericHistogram::new(NumericHistogramParams { max_samples: cap })
    }

    #[test]
    fn record_fills_up_to_cap() {
        let mut h = hist_with_cap(5);
        for i in 0..5 {
            h.record(i as f64);
        }
        assert_eq!(h.samples.len(), 5);
    }

    #[test]
    fn record_beyond_cap_does_not_grow_vec() {
        let mut h = hist_with_cap(3);
        for i in 0..100 {
            h.record(i as f64);
        }
        assert_eq!(h.samples.len(), 3);
    }

    #[test]
    fn total_seen_always_increments() {
        let mut h = hist_with_cap(2);
        for i in 0..10 {
            h.record(i as f64);
        }
        assert_eq!(h.total_seen(), 10);
    }

    #[test]
    fn distribution_returns_correct_quantiles() {
        let mut h = hist_with_cap(100);
        // Record values 1.0 through 100.0
        for i in 1..=100 {
            h.record(i as f64);
        }
        let dist = h.distribution();
        // Min should be 1.0, max should be 100.0
        assert_eq!(dist.min(), 1.0);
        assert_eq!(dist.max(), 100.0);
        // With 100 values: p50 idx = floor(100 * 0.5) = 50 → 51st value in sorted order
        let p50 = dist.quantile(0.5);
        assert!((1.0..=100.0).contains(&p50), "p50={p50} out of range");
    }

    #[test]
    fn is_empty_before_records() {
        let h = hist_with_cap(10);
        assert!(h.is_empty());
    }
}
