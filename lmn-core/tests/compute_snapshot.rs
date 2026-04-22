//! Behavioral snapshot over `RunReport::from_params`.
//!
//! Unlike `report_snapshot.rs` — which pins the serialized contract against a
//! hand-built fixture — this test drives the *real* aggregation path.
//! It constructs a deterministic `RunStats` from public histogram APIs, calls
//! the real `RunReport::from_params` constructor, and snapshots the resulting
//! `RunReport`.
//!
//! A failure here flags one of:
//!   1. A schema drift (same as the schema-lock test would catch).
//!   2. A logic drift — the aggregation now computes different percentiles,
//!      error rates, status-code mappings, stage breakdowns, etc.
//!
//! The diff tells you which. These tests are the primary protection against
//! silent logic regressions in the compute layer.

use std::time::Duration;

use lmn_core::execution::{CurveStats, RunMode, RunStats, StageStats};
use lmn_core::histogram::{LatencyHistogram, StatusCodeHistogram};
use lmn_core::load_curve::{RampType, Stage};
use lmn_core::output::{RunReport, RunReportParams};

// ── Shared fixture helpers ────────────────────────────────────────────────────

/// Populates a histogram with `count` samples at `each_ms` milliseconds.
fn record_n(hist: &mut LatencyHistogram, count: u64, each_ms: u64) {
    for _ in 0..count {
        hist.record(Duration::from_millis(each_ms));
    }
}

/// Populates a status code histogram with a deterministic mix.
fn fill_status_codes(hist: &mut StatusCodeHistogram, ok: u64, server_err: u64, conn_err: u64) {
    for _ in 0..ok {
        hist.record(Some(200));
    }
    for _ in 0..server_err {
        hist.record(Some(500));
    }
    for _ in 0..conn_err {
        hist.record(None);
    }
}

// ── Fixed-mode scenario ───────────────────────────────────────────────────────

/// Deterministic RunStats for a 10-second fixed-mode run with a well-defined
/// latency distribution and a small number of failures + connection errors.
fn deterministic_fixed_run_stats() -> RunStats {
    let mut latency = LatencyHistogram::new();
    // 100 samples: 50 @ 5ms, 30 @ 20ms, 15 @ 50ms, 5 @ 200ms.
    // Yields stable, reproducible percentiles.
    record_n(&mut latency, 50, 5);
    record_n(&mut latency, 30, 20);
    record_n(&mut latency, 15, 50);
    record_n(&mut latency, 5, 200);

    let mut status_codes = StatusCodeHistogram::new();
    fill_status_codes(&mut status_codes, 95, 3, 2);

    RunStats {
        elapsed: Duration::from_secs(10),
        mode: RunMode::Fixed,
        latency,
        status_codes,
        total_requests: 100,
        total_failures: 5,
        total_skipped: 0,
        template_stats: None,
        response_stats: None,
        curve_stats: None,
        scenario_stats: None,
    }
}

/// Exercises the full `RunReport::from_params` aggregation for fixed mode.
/// Locks: latency percentiles, status code BTreeMap ordering (including the
/// `"error"` key for connection failures), `error_rate` math, `throughput_rps`
/// math, and the shape of optional sections when absent.
#[test]
fn run_report_from_fixed_mode_deterministic_samples() {
    let stats = deterministic_fixed_run_stats();
    let report = RunReport::from_params(RunReportParams { stats: &stats });
    insta::assert_json_snapshot!(report);
}

// ── Curve-mode scenario ───────────────────────────────────────────────────────

/// Deterministic RunStats for a 2-stage curve-mode run.
fn deterministic_curve_run_stats() -> RunStats {
    // Stage 0: 4 seconds, linear ramp to 10 VUs, 40 requests all OK at 10ms.
    let mut s0_latency = LatencyHistogram::new();
    record_n(&mut s0_latency, 40, 10);
    let mut s0_status = StatusCodeHistogram::new();
    fill_status_codes(&mut s0_status, 40, 0, 0);
    let stage0 = Stage {
        duration: Duration::from_secs(4),
        target_vus: 10,
        ramp: RampType::Linear,
    };
    let stage0_stats = StageStats {
        latency: s0_latency,
        status_codes: s0_status,
        total_requests: 40,
        total_failures: 0,
    };

    // Stage 1: 6 seconds, step to 50 VUs, 60 requests — 55 OK, 3 server errors, 2 connection errors.
    let mut s1_latency = LatencyHistogram::new();
    record_n(&mut s1_latency, 50, 20);
    record_n(&mut s1_latency, 10, 100);
    let mut s1_status = StatusCodeHistogram::new();
    fill_status_codes(&mut s1_status, 55, 3, 2);
    let stage1 = Stage {
        duration: Duration::from_secs(6),
        target_vus: 50,
        ramp: RampType::Step,
    };
    let stage1_stats = StageStats {
        latency: s1_latency,
        status_codes: s1_status,
        total_requests: 60,
        total_failures: 5,
    };

    // Aggregate run-level histograms = union of stage histograms.
    let mut total_latency = LatencyHistogram::new();
    record_n(&mut total_latency, 40, 10);
    record_n(&mut total_latency, 50, 20);
    record_n(&mut total_latency, 10, 100);
    let mut total_status = StatusCodeHistogram::new();
    fill_status_codes(&mut total_status, 95, 3, 2);

    RunStats {
        elapsed: Duration::from_secs(10),
        mode: RunMode::Curve,
        latency: total_latency,
        status_codes: total_status,
        total_requests: 100,
        total_failures: 5,
        total_skipped: 0,
        template_stats: None,
        response_stats: None,
        curve_stats: Some(CurveStats {
            duration: Duration::from_secs(10),
            stages: vec![stage0, stage1],
            stage_stats: vec![stage0_stats, stage1_stats],
        }),
        scenario_stats: None,
    }
}

/// Exercises the curve-mode aggregation path and the `curve_stages` output
/// section. Locks per-stage latency, throughput, and error rate math on top
/// of the run-level surface already covered by the fixed-mode test.
#[test]
fn run_report_from_curve_mode_two_stages() {
    let stats = deterministic_curve_run_stats();
    let report = RunReport::from_params(RunReportParams { stats: &stats });
    insta::assert_json_snapshot!(report);
}
