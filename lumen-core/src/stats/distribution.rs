use std::time::Duration;

use crate::http::RequestResult;

// ── Distribution ──────────────────────────────────────────────────────────────

/// Owns a sorted snapshot of observed values and answers arbitrary quantile queries.
///
/// Construction sorts the input once; queries are O(1) index lookups.
/// Input is `f64` throughout — Duration callers convert to milliseconds at the call site.
pub struct Distribution {
    sorted: Vec<f64>,
}

impl Distribution {
    /// Sorts `values` and takes ownership. O(n log n).
    ///
    /// Use `from_sorted` when values are already sorted to skip the sort step.
    pub fn from_unsorted(mut values: Vec<f64>) -> Self {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Self { sorted: values }
    }

    /// Constructs a `Distribution` from an already-sorted `Vec<f64>`.
    ///
    /// Caller is responsible for ensuring the input is sorted in ascending order.
    /// No sort is performed; construction is O(1).
    pub fn from_sorted(values: Vec<f64>) -> Self {
        Self { sorted: values }
    }

    /// Returns the value at quantile `p` in `[0.0, 1.0]` using the floor-index formula.
    ///
    /// `idx = (n as f64 * p).floor() as usize`, clamped to `[0, n-1]`.
    /// Returns `0.0` for an empty distribution.
    ///
    /// This formula is equivalent to the integer formula `n * p / 100` (for integer
    /// percentile values 0–100) used in the ASCII table renderer, ensuring both
    /// output paths report identical percentile values.
    pub fn quantile(&self, p: f64) -> f64 {
        if self.sorted.is_empty() {
            return 0.0;
        }
        let n = self.sorted.len();
        let idx = ((n as f64 * p).floor() as usize).min(n - 1);
        self.sorted[idx]
    }

    /// Returns the minimum value, or `0.0` for an empty distribution.
    pub fn min(&self) -> f64 {
        self.sorted.first().copied().unwrap_or(0.0)
    }

    /// Returns the maximum value, or `0.0` for an empty distribution.
    pub fn max(&self) -> f64 {
        self.sorted.last().copied().unwrap_or(0.0)
    }

    /// Returns the arithmetic mean, or `0.0` for an empty distribution.
    pub fn mean(&self) -> f64 {
        if self.sorted.is_empty() {
            return 0.0;
        }
        self.sorted.iter().sum::<f64>() / self.sorted.len() as f64
    }

    /// Returns `true` if the distribution contains no values.
    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }

    /// Returns the number of values in the distribution.
    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    /// Returns the value at a pre-computed index into the sorted backing array.
    ///
    /// Use this when the caller has already computed the index via the integer formula
    /// `(n * p / 100).min(n - 1)` and wants to avoid the redundant float round-trip
    /// through `quantile(idx as f64 / n as f64)`.
    ///
    /// # Panics
    /// Panics if `idx >= self.len()`. The caller is responsible for clamping the index
    /// to `[0, n-1]` before calling this method.
    pub fn value_at(&self, idx: usize) -> f64 {
        self.sorted[idx]
    }
}

// ── LatencyDistribution ───────────────────────────────────────────────────────

/// Wraps a `RequestResult` slice and presents latency queries in milliseconds.
///
/// Converts each `Duration` to `f64` milliseconds at construction time using
/// `d.as_secs_f64() * 1000.0`. This avoids making `Distribution` generic while
/// keeping Duration-specific construction ergonomic for latency use cases.
pub struct LatencyDistribution(Distribution);

impl LatencyDistribution {
    /// Converts each `RequestResult.duration` to milliseconds, then constructs a
    /// sorted `Distribution`.
    pub fn from_results(results: &[RequestResult]) -> Self {
        let ms_values: Vec<f64> = results
            .iter()
            .map(|r| r.duration.as_secs_f64() * 1000.0)
            .collect();
        Self(Distribution::from_unsorted(ms_values))
    }

    /// Converts each `Duration` to milliseconds, then constructs a sorted `Distribution`.
    pub fn from_durations(durations: &[Duration]) -> Self {
        let ms_values: Vec<f64> = durations
            .iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .collect();
        Self(Distribution::from_unsorted(ms_values))
    }

    /// Returns the value at quantile `p` in `[0.0, 1.0]` in milliseconds.
    pub fn quantile_ms(&self, p: f64) -> f64 {
        self.0.quantile(p)
    }

    /// Returns the minimum latency in milliseconds, or `0.0` for an empty distribution.
    pub fn min_ms(&self) -> f64 {
        self.0.min()
    }

    /// Returns the maximum latency in milliseconds, or `0.0` for an empty distribution.
    pub fn max_ms(&self) -> f64 {
        self.0.max()
    }

    /// Returns the arithmetic mean latency in milliseconds, or `0.0` for an empty distribution.
    pub fn mean_ms(&self) -> f64 {
        self.0.mean()
    }

    /// Returns `true` if the distribution contains no values.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Distribution ──────────────────────────────────────────────────────────

    #[test]
    fn distribution_quantile_known_values() {
        // 100-element uniform distribution: 1.0, 2.0, ..., 100.0
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let dist = Distribution::from_sorted(values);

        // quantile(0.0): idx = floor(100 * 0.0) = 0 → value 1.0
        assert_eq!(dist.quantile(0.0), 1.0);
        // quantile(0.5): idx = floor(100 * 0.5) = 50 → value 51.0
        assert_eq!(dist.quantile(0.5), 51.0);
        // quantile(0.99): idx = floor(100 * 0.99) = 99 → value 100.0
        assert_eq!(dist.quantile(0.99), 100.0);
        // quantile(1.0): idx = floor(100 * 1.0) = 100, clamped to 99 → value 100.0
        assert_eq!(dist.quantile(1.0), 100.0);
    }

    #[test]
    fn distribution_empty_returns_zero() {
        let dist = Distribution::from_sorted(vec![]);
        assert_eq!(dist.quantile(0.5), 0.0);
        assert_eq!(dist.min(), 0.0);
        assert_eq!(dist.max(), 0.0);
        assert_eq!(dist.mean(), 0.0);
    }

    #[test]
    fn distribution_single_element() {
        let dist = Distribution::from_sorted(vec![42.0]);
        assert_eq!(dist.quantile(0.0), 42.0);
        assert_eq!(dist.quantile(0.5), 42.0);
        assert_eq!(dist.quantile(1.0), 42.0);
        assert_eq!(dist.min(), 42.0);
        assert_eq!(dist.max(), 42.0);
        assert_eq!(dist.mean(), 42.0);
    }

    #[test]
    fn distribution_quantile_boundary_p100() {
        let values: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        let dist = Distribution::from_sorted(values);
        // Must not panic — must return last element
        assert_eq!(dist.quantile(1.0), 10.0);
    }

    #[test]
    fn distribution_avg_is_arithmetic_mean() {
        let dist = Distribution::from_sorted(vec![1.0, 2.0, 3.0]);
        assert_eq!(dist.mean(), 2.0);
    }

    #[test]
    fn distribution_is_sorted_on_construction() {
        // Unsorted input: [5.0, 1.0, 3.0, 2.0, 4.0]
        let dist = Distribution::from_unsorted(vec![5.0, 1.0, 3.0, 2.0, 4.0]);
        assert_eq!(dist.min(), 1.0);
        assert_eq!(dist.max(), 5.0);
        // quantile(0.5): idx = floor(5 * 0.5) = 2 → value 3.0
        assert_eq!(dist.quantile(0.5), 3.0);
    }

    #[test]
    fn distribution_len_and_is_empty() {
        let empty = Distribution::from_sorted(vec![]);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let dist = Distribution::from_sorted(vec![1.0, 2.0, 3.0]);
        assert!(!dist.is_empty());
        assert_eq!(dist.len(), 3);
    }

    #[test]
    fn distribution_value_at_returns_correct_element() {
        // sorted: [10.0, 20.0, 30.0, 40.0, 50.0]
        let dist = Distribution::from_sorted(vec![10.0, 20.0, 30.0, 40.0, 50.0]);
        assert_eq!(dist.value_at(0), 10.0);
        assert_eq!(dist.value_at(2), 30.0);
        assert_eq!(dist.value_at(4), 50.0);
    }

    // ── LatencyDistribution ───────────────────────────────────────────────────

    #[test]
    fn latency_distribution_converts_duration_to_ms() {
        let durations = vec![Duration::from_millis(42)];
        let dist = LatencyDistribution::from_durations(&durations);
        assert_eq!(dist.quantile_ms(0.5), 42.0);
        assert_eq!(dist.min_ms(), 42.0);
        assert_eq!(dist.max_ms(), 42.0);
        assert_eq!(dist.mean_ms(), 42.0);
    }

    #[test]
    fn latency_distribution_empty() {
        let dist = LatencyDistribution::from_durations(&[]);
        assert_eq!(dist.quantile_ms(0.5), 0.0);
        assert_eq!(dist.min_ms(), 0.0);
        assert_eq!(dist.max_ms(), 0.0);
        assert_eq!(dist.mean_ms(), 0.0);
        assert!(dist.is_empty());
    }

    #[test]
    fn latency_distribution_from_results() {
        let results: Vec<RequestResult> = vec![
            RequestResult::new(Duration::from_millis(10), true, Some(200), None),
            RequestResult::new(Duration::from_millis(20), true, Some(200), None),
            RequestResult::new(Duration::from_millis(30), true, Some(200), None),
        ];
        let dist = LatencyDistribution::from_results(&results);
        assert_eq!(dist.min_ms(), 10.0);
        assert_eq!(dist.max_ms(), 30.0);
        // mean = (10 + 20 + 30) / 3 = 20.0
        assert!((dist.mean_ms() - 20.0).abs() < f64::EPSILON);
    }
}
