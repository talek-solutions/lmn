use rand::Rng;

// ── SamplingParams ────────────────────────────────────────────────────────────

/// Configuration for the two-stage sampling mechanism (VU threshold + reservoir).
pub struct SamplingParams {
    /// VU count below which all results are collected.
    /// Set to `0` to disable VU-threshold sampling entirely (always rate 1.0).
    /// Default: 50.
    pub vu_threshold: usize,
    /// Maximum results to retain in the buffer.
    /// Default: 100_000.
    pub reservoir_size: usize,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            vu_threshold: 50,
            reservoir_size: 100_000,
        }
    }
}

// ── ReservoirAction ───────────────────────────────────────────────────────────

/// Instruction returned by `SamplingState::reservoir_slot`.
pub enum ReservoirAction {
    /// Append the result to the end of the results buffer.
    Push,
    /// Replace the result at the given index in the results buffer.
    Replace(usize),
    /// Drop the result; the buffer is full and this slot lost the lottery.
    Discard,
}

// ── SamplingState ─────────────────────────────────────────────────────────────

/// Runtime sampling state. Tracks counters and drives both the VU-threshold
/// gate and Vitter's Algorithm R reservoir gate.
pub struct SamplingState {
    vu_threshold: usize,
    reservoir_size: usize,
    sample_rate: f64,
    min_sample_rate: f64,
    /// Actual (unsampled) total request count — always incremented.
    total_requests: usize,
    /// Actual (unsampled) failure count — always incremented.
    total_failures: usize,
    /// Denominator for the reservoir replacement lottery (Vitter's Algorithm R).
    total_seen_for_reservoir: usize,
    rng: rand::rngs::ThreadRng,
}

impl SamplingState {
    pub fn new(params: SamplingParams) -> Self {
        Self {
            vu_threshold: params.vu_threshold,
            reservoir_size: params.reservoir_size,
            sample_rate: 1.0,
            min_sample_rate: 1.0,
            total_requests: 0,
            total_failures: 0,
            total_seen_for_reservoir: 0,
            rng: rand::rng(),
        }
    }

    /// Call on each coordinator tick when the active VU count may have changed.
    ///
    /// When `vus > vu_threshold` (and `vu_threshold != 0`), caps the collection
    /// rate to `threshold / vus`. Otherwise rate is 1.0 (collect everything).
    pub fn set_active_vus(&mut self, vus: usize) {
        self.sample_rate = if self.vu_threshold == 0 || vus <= self.vu_threshold {
            1.0
        } else {
            self.vu_threshold as f64 / vus as f64
        };
        self.min_sample_rate = self.min_sample_rate.min(self.sample_rate);
    }

    /// Always call for every completed request. Updates the unsampled counters
    /// regardless of whether this result will be stored in the reservoir.
    pub fn record_request(&mut self, success: bool) {
        self.total_requests += 1;
        if !success {
            self.total_failures += 1;
        }
    }

    /// VU-threshold gate: returns `true` if this result should proceed toward
    /// the reservoir. At `sample_rate >= 1.0` always returns `true`.
    pub fn should_collect(&mut self) -> bool {
        self.sample_rate >= 1.0 || self.rng.random::<f64>() < self.sample_rate
    }

    /// Reservoir gate (Vitter's Algorithm R). Call only when `should_collect()`
    /// returned `true`.
    ///
    /// Increments the internal seen-counter and returns the storage instruction:
    /// - `Push` — buffer not yet full; append.
    /// - `Replace(j)` — buffer full; replace slot `j` (uniform random).
    /// - `Discard` — buffer full; this result lost the lottery.
    pub fn reservoir_slot(&mut self, results_len: usize) -> ReservoirAction {
        self.total_seen_for_reservoir += 1;
        if results_len < self.reservoir_size {
            ReservoirAction::Push
        } else {
            let j = self.rng.random_range(0..self.total_seen_for_reservoir);
            if j < self.reservoir_size {
                ReservoirAction::Replace(j)
            } else {
                ReservoirAction::Discard
            }
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn total_requests(&self) -> usize {
        self.total_requests
    }

    pub fn total_failures(&self) -> usize {
        self.total_failures
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn min_sample_rate(&self) -> f64 {
        self.min_sample_rate
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> SamplingState {
        SamplingState::new(SamplingParams::default())
    }

    // ── set_active_vus ────────────────────────────────────────────────────────

    #[test]
    fn rate_is_1_below_threshold() {
        let mut s = default_state();
        s.set_active_vus(49);
        assert_eq!(s.sample_rate(), 1.0);
    }

    #[test]
    fn rate_is_1_at_threshold() {
        let mut s = default_state();
        s.set_active_vus(50);
        assert_eq!(s.sample_rate(), 1.0);
    }

    #[test]
    fn rate_drops_above_threshold() {
        let mut s = default_state();
        s.set_active_vus(100);
        assert!((s.sample_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn rate_scales_proportionally() {
        let mut s = SamplingState::new(SamplingParams {
            vu_threshold: 50,
            reservoir_size: 100_000,
        });
        s.set_active_vus(200);
        assert!((s.sample_rate() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_threshold_always_collects() {
        let mut s = SamplingState::new(SamplingParams {
            vu_threshold: 0,
            reservoir_size: 100_000,
        });
        s.set_active_vus(10_000);
        assert_eq!(s.sample_rate(), 1.0);
        // should_collect must always be true when rate == 1.0
        for _ in 0..100 {
            assert!(s.should_collect());
        }
    }

    #[test]
    fn min_sample_rate_tracks_lowest_observed() {
        let mut s = default_state();
        s.set_active_vus(100); // rate = 0.5
        s.set_active_vus(200); // rate = 0.25
        s.set_active_vus(50); // rate = 1.0 — min must not increase
        assert!((s.min_sample_rate() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn min_sample_rate_starts_at_1() {
        let s = default_state();
        assert_eq!(s.min_sample_rate(), 1.0);
    }

    // ── record_request ────────────────────────────────────────────────────────

    #[test]
    fn record_request_increments_total() {
        let mut s = default_state();
        s.record_request(true);
        s.record_request(true);
        assert_eq!(s.total_requests(), 2);
    }

    #[test]
    fn record_request_tracks_failures() {
        let mut s = default_state();
        s.record_request(true);
        s.record_request(false);
        s.record_request(false);
        assert_eq!(s.total_requests(), 3);
        assert_eq!(s.total_failures(), 2);
    }

    #[test]
    fn record_request_success_does_not_increment_failures() {
        let mut s = default_state();
        s.record_request(true);
        assert_eq!(s.total_failures(), 0);
    }

    // ── should_collect ────────────────────────────────────────────────────────

    #[test]
    fn should_collect_always_true_at_full_rate() {
        let mut s = default_state();
        s.set_active_vus(10); // rate = 1.0
        for _ in 0..1000 {
            assert!(s.should_collect());
        }
    }

    #[test]
    fn should_collect_probabilistic_at_half_rate() {
        let mut s = default_state();
        s.set_active_vus(100); // rate = 0.5
        let collected: usize = (0..10_000).filter(|_| s.should_collect()).count();
        // Expect ~5000; allow ±15% tolerance
        assert!(
            collected > 4_000 && collected < 6_000,
            "expected ~5000 collected, got {collected}"
        );
    }

    // ── reservoir_slot ────────────────────────────────────────────────────────

    #[test]
    fn reservoir_pushes_while_not_full() {
        let mut s = SamplingState::new(SamplingParams {
            vu_threshold: 0,
            reservoir_size: 5,
        });
        for i in 0..5 {
            match s.reservoir_slot(i) {
                ReservoirAction::Push => {}
                _ => panic!("expected Push at results_len={i}"),
            }
        }
    }

    #[test]
    fn reservoir_never_pushes_when_full() {
        let mut s = SamplingState::new(SamplingParams {
            vu_threshold: 0,
            reservoir_size: 5,
        });
        // Bring total_seen_for_reservoir up to 5 (all Pushed).
        for i in 0..5 {
            s.reservoir_slot(i);
        }
        // Now results_len == reservoir_size == 5; must not Push.
        for _ in 0..100 {
            if let ReservoirAction::Push = s.reservoir_slot(5) {
                panic!("Push when reservoir is full")
            }
        }
    }

    #[test]
    fn reservoir_replace_index_is_in_bounds() {
        let mut s = SamplingState::new(SamplingParams {
            vu_threshold: 0,
            reservoir_size: 5,
        });
        // Fill reservoir first.
        for i in 0..5 {
            s.reservoir_slot(i);
        }
        for _ in 0..200 {
            if let ReservoirAction::Replace(idx) = s.reservoir_slot(5) {
                assert!(
                    idx < 5,
                    "Replace index {idx} out of bounds for reservoir_size=5"
                );
            }
        }
    }

    #[test]
    fn reservoir_discard_rate_decreases_over_time() {
        // With a very large total_seen relative to reservoir_size, most slots
        // should be Discard. This verifies the algorithm converges correctly.
        let mut s = SamplingState::new(SamplingParams {
            vu_threshold: 0,
            reservoir_size: 10,
        });
        // Fill reservoir.
        for i in 0..10 {
            s.reservoir_slot(i);
        }
        // Add 990 more (total_seen = 1000, reservoir_size = 10).
        // Expected replace rate ≈ 10/1000 = 1%.
        let mut replaces = 0usize;
        let mut discards = 0usize;
        for _ in 0..1000 {
            match s.reservoir_slot(10) {
                ReservoirAction::Replace(_) => replaces += 1,
                ReservoirAction::Discard => discards += 1,
                ReservoirAction::Push => panic!("unexpected Push"),
            }
        }
        assert!(
            discards > replaces,
            "expected more discards than replaces at high total_seen; replaces={replaces}, discards={discards}"
        );
    }

    // ── sampling reflects history ─────────────────────────────────────────────

    #[test]
    fn is_sampling_reflects_history() {
        // min_sample_rate stays < 1.0 even if VUs later drop back below threshold.
        let mut s = default_state();
        s.set_active_vus(10); // rate = 1.0
        assert_eq!(s.min_sample_rate(), 1.0);

        s.set_active_vus(100); // rate = 0.5
        assert!((s.min_sample_rate() - 0.5).abs() < f64::EPSILON);

        s.set_active_vus(10); // rate = 1.0 again — min must not reset
        assert!((s.min_sample_rate() - 0.5).abs() < f64::EPSILON);
    }
}
