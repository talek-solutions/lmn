use std::time::Duration;

use hdrhistogram::Histogram;

// ── LatencyHistogram ──────────────────────────────────────────────────────────

/// HDR histogram for recording request latencies.
///
/// Records durations in microseconds. Covers 1µs to 1 hour at 3 significant
/// digits of precision. Provides exact quantile queries with bounded error.
pub struct LatencyHistogram {
    inner: Histogram<u64>,
}

impl LatencyHistogram {
    /// Creates a new histogram covering 1µs to 1 hour at 3 significant digits.
    pub fn new() -> Self {
        let inner = Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)
            .expect("valid HDR histogram bounds");
        Self { inner }
    }

    /// Records a duration. Values are clamped to [1µs, 1 hour].
    pub fn record(&mut self, d: Duration) {
        let us = (d.as_micros() as u64).max(1).min(self.inner.high());
        let ok = self.inner.record(us).is_ok();
        debug_assert!(
            ok,
            "HDR histogram record failed for value {us}µs — this should never happen after clamping"
        );
    }

    /// Returns the value at quantile `q` (0.0–1.0) in milliseconds.
    pub fn quantile_ms(&self, q: f64) -> f64 {
        self.inner.value_at_quantile(q) as f64 / 1000.0
    }

    /// Returns the minimum recorded value in milliseconds.
    pub fn min_ms(&self) -> f64 {
        self.inner.min() as f64 / 1000.0
    }

    /// Returns the maximum recorded value in milliseconds.
    pub fn max_ms(&self) -> f64 {
        self.inner.max() as f64 / 1000.0
    }

    /// Returns the arithmetic mean of recorded values in milliseconds.
    pub fn mean_ms(&self) -> f64 {
        self.inner.mean() / 1000.0
    }

    /// Returns the total number of recorded values.
    pub fn total_count(&self) -> u64 {
        self.inner.len()
    }

    /// Returns `true` if no values have been recorded.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns `(value_us, count)` pairs for all recorded distinct values.
    /// Used by the CLI to render the latency bar chart.
    pub fn iter_recorded_us(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.inner
            .iter_recorded()
            .map(|v| (v.value_iterated_to(), v.count_at_value()))
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_quantile_basic() {
        let mut h = LatencyHistogram::new();
        h.record(Duration::from_millis(10));
        h.record(Duration::from_millis(20));
        h.record(Duration::from_millis(30));
        // p50 of [10, 20, 30] should be approximately 20ms
        let p50 = h.quantile_ms(0.50);
        assert!(p50 >= 10.0 && p50 <= 30.0, "p50={p50} not in [10, 30]");
    }

    #[test]
    fn record_zero_duration_does_not_panic() {
        let mut h = LatencyHistogram::new();
        // Duration::ZERO is 0µs — clamped to 1µs internally
        h.record(Duration::ZERO);
        assert!(!h.is_empty());
    }

    #[test]
    fn is_empty_before_any_record() {
        let h = LatencyHistogram::new();
        assert!(h.is_empty());
    }

    #[test]
    fn is_not_empty_after_record() {
        let mut h = LatencyHistogram::new();
        h.record(Duration::from_millis(1));
        assert!(!h.is_empty());
    }

    #[test]
    fn min_max_ms_correct() {
        let mut h = LatencyHistogram::new();
        h.record(Duration::from_millis(10));
        h.record(Duration::from_millis(100));
        assert!((h.min_ms() - 10.0).abs() < 1.0, "min_ms={}", h.min_ms());
        assert!((h.max_ms() - 100.0).abs() < 1.0, "max_ms={}", h.max_ms());
    }

    #[test]
    fn iter_recorded_us_non_empty() {
        let mut h = LatencyHistogram::new();
        h.record(Duration::from_millis(5));
        h.record(Duration::from_millis(50));

        let pairs: Vec<_> = h.iter_recorded_us().collect();
        assert!(!pairs.is_empty(), "expected at least one recorded bucket");
        // All counts should be > 0
        for (_, count) in &pairs {
            assert!(*count > 0);
        }
    }
}
