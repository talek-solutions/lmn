use std::collections::HashMap;

// ── CategoricalHistogramParams ────────────────────────────────────────────────

/// Parameters for constructing a `CategoricalHistogram`.
pub struct CategoricalHistogramParams {
    /// Maximum number of distinct buckets to track.
    /// Values observed after the cap is reached are counted in `overflow`.
    pub max_buckets: usize,
}

// ── CategoricalHistogram ──────────────────────────────────────────────────────

/// Tracks frequency counts for a bounded set of distinct string values.
///
/// Once `max_buckets` distinct values have been seen, new distinct values are
/// counted in an `overflow` bucket rather than being added to the map. Repeated
/// values that are already in the map continue to be counted correctly.
pub struct CategoricalHistogram {
    entries: HashMap<String, u64>,
    overflow: u64,
    max_buckets: usize,
    total: u64,
}

impl CategoricalHistogram {
    /// Creates a new histogram with the given parameters.
    pub fn new(params: CategoricalHistogramParams) -> Self {
        Self {
            entries: HashMap::new(),
            overflow: 0,
            max_buckets: params.max_buckets,
            total: 0,
        }
    }

    /// Records a single string value.
    pub fn record(&mut self, value: &str) {
        self.total += 1;
        if self.entries.contains_key(value) {
            *self.entries.get_mut(value).unwrap() += 1;
        } else if self.entries.len() < self.max_buckets {
            self.entries.insert(value.to_string(), 1);
        } else {
            self.overflow += 1;
        }
    }

    /// Returns the map of value to count for all tracked distinct values.
    pub fn entries(&self) -> &HashMap<String, u64> {
        &self.entries
    }

    /// Returns the count of values that were not tracked due to bucket cap.
    pub fn overflow(&self) -> u64 {
        self.overflow
    }

    /// Returns the total number of recorded values (tracked + overflow).
    pub fn total(&self) -> u64 {
        self.total
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn hist_with_cap(cap: usize) -> CategoricalHistogram {
        CategoricalHistogram::new(CategoricalHistogramParams { max_buckets: cap })
    }

    #[test]
    fn record_increments_entry_count() {
        let mut h = hist_with_cap(10);
        h.record("ok");
        assert_eq!(h.entries()["ok"], 1);
    }

    #[test]
    fn record_same_value_increments_existing_entry() {
        let mut h = hist_with_cap(10);
        h.record("ok");
        h.record("ok");
        h.record("ok");
        assert_eq!(h.entries()["ok"], 3);
    }

    #[test]
    fn overflow_when_max_buckets_exceeded() {
        let mut h = hist_with_cap(2);
        h.record("a");
        h.record("b");
        h.record("c"); // overflow: cap=2 already has "a" and "b"
        h.record("d"); // overflow
        assert_eq!(h.overflow(), 2);
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn total_always_increments() {
        let mut h = hist_with_cap(1);
        h.record("a");
        h.record("b"); // overflow
        h.record("c"); // overflow
        assert_eq!(h.total(), 3);
    }

    #[test]
    fn no_overflow_within_limit() {
        let mut h = hist_with_cap(5);
        for v in ["a", "b", "c", "d", "e"] {
            h.record(v);
        }
        assert_eq!(h.overflow(), 0);
        assert_eq!(h.entries().len(), 5);
    }
}
