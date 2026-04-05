use std::collections::HashMap;

// ── StatusCodeHistogram ───────────────────────────────────────────────────────

/// Tracks the frequency of HTTP status codes and connection errors across requests.
pub struct StatusCodeHistogram {
    counts: HashMap<u16, u64>,
    error_count: u64,
}

impl StatusCodeHistogram {
    /// Creates a new empty histogram.
    pub fn new() -> Self {
        Self {
            counts: HashMap::new(),
            error_count: 0,
        }
    }

    /// Records a single request outcome.
    ///
    /// `None` status code represents a connection error (no HTTP response received).
    pub fn record(&mut self, status_code: Option<u16>) {
        match status_code {
            Some(code) => *self.counts.entry(code).or_insert(0) += 1,
            None => self.error_count += 1,
        }
    }

    /// Returns the map of HTTP status code to count.
    pub fn counts(&self) -> &HashMap<u16, u64> {
        &self.counts
    }

    /// Returns the number of connection errors (requests with no HTTP response).
    pub fn error_count(&self) -> u64 {
        self.error_count
    }

    /// Returns the total number of recorded requests (status codes + errors).
    pub fn total(&self) -> u64 {
        self.counts.values().sum::<u64>() + self.error_count
    }
}

impl Default for StatusCodeHistogram {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_200_increments_count() {
        let mut h = StatusCodeHistogram::new();
        h.record(Some(200));
        assert_eq!(h.counts()[&200], 1);
    }

    #[test]
    fn record_none_increments_error_count() {
        let mut h = StatusCodeHistogram::new();
        h.record(None);
        h.record(None);
        assert_eq!(h.error_count(), 2);
    }

    #[test]
    fn total_sums_all_codes_and_errors() {
        let mut h = StatusCodeHistogram::new();
        h.record(Some(200));
        h.record(Some(200));
        h.record(Some(404));
        h.record(None);
        assert_eq!(h.total(), 4);
    }

    #[test]
    fn multiple_codes_tracked_independently() {
        let mut h = StatusCodeHistogram::new();
        h.record(Some(200));
        h.record(Some(201));
        h.record(Some(404));
        h.record(Some(503));
        assert_eq!(h.counts()[&200], 1);
        assert_eq!(h.counts()[&201], 1);
        assert_eq!(h.counts()[&404], 1);
        assert_eq!(h.counts()[&503], 1);
    }
}
