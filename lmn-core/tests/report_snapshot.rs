//! Schema-lock snapshot for the `RunReport` v2 contract.
//!
//! This test pins the *shape* of the serialized `RunReport` — field names,
//! ordering, how `Option::None` vs `Some` renders, and which fields are
//! skipped when zero. It is intentionally constructed from hand-built struct
//! values, not from the real aggregation pipeline: the goal is to fail
//! loudly on any change to the downstream contract consumed by the CLI JSON
//! output, the NestJS SaaS platform, CI pipelines, and cloud mode.
//!
//! When this test fails:
//! 1. If the schema change is intentional, bump `RunReport.version` and
//!    coordinate with downstream consumers before accepting the new snapshot
//!    via `cargo insta review`.
//! 2. If the schema change is *not* intentional, treat the diff as a bug.

use std::collections::BTreeMap;

use lmn_core::output::{
    FloatFieldSummary, LatencyStats, RequestSummary, ResponseStatsReport, RunMeta, RunReport,
    ScenarioReport, ScenarioStepReport, StageReport,
};
use lmn_core::threshold::{Metric, Operator, Threshold, ThresholdReport, ThresholdResult};

// ── Builders ──────────────────────────────────────────────────────────────────

fn sample_latency() -> LatencyStats {
    LatencyStats {
        min_ms: 1.0,
        p10_ms: 2.0,
        p25_ms: 3.0,
        p50_ms: 5.0,
        p75_ms: 8.0,
        p90_ms: 12.0,
        p95_ms: 18.0,
        p99_ms: 40.0,
        max_ms: 100.0,
        avg_ms: 7.5,
    }
}

fn sample_requests() -> RequestSummary {
    RequestSummary {
        total: 1000,
        ok: 985,
        failed: 10,
        skipped: 5,
        error_rate: 0.01,
        throughput_rps: 100.0,
    }
}

fn sample_status_codes() -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    m.insert("200".into(), 985);
    m.insert("500".into(), 10);
    m.insert("error".into(), 5);
    m
}

fn sample_response_stats() -> ResponseStatsReport {
    let mut string_fields: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut status_dist = BTreeMap::new();
    status_dist.insert("error".into(), 15);
    status_dist.insert("ok".into(), 985);
    string_fields.insert("status".into(), status_dist);

    let mut float_fields = BTreeMap::new();
    float_fields.insert(
        "score".into(),
        FloatFieldSummary {
            min: 0.1,
            avg: 0.5,
            p50: 0.5,
            p95: 0.9,
            p99: 0.99,
            max: 1.0,
        },
    );

    let mut mismatch_counts = BTreeMap::new();
    mismatch_counts.insert("missing_field".into(), 3);

    ResponseStatsReport {
        responses_parsed: 1000,
        string_fields,
        float_fields,
        mismatch_counts,
    }
}

fn sample_curve_stages() -> Vec<StageReport> {
    vec![
        StageReport {
            index: 0,
            duration_ms: 2000.0,
            target_vus: 10,
            ramp: "linear".into(),
            requests: 500,
            ok: 495,
            failed: 5,
            error_rate: 0.01,
            throughput_rps: 250.0,
            latency: sample_latency(),
        },
        StageReport {
            index: 1,
            duration_ms: 3000.0,
            target_vus: 50,
            ramp: "step".into(),
            requests: 500,
            ok: 490,
            failed: 5,
            error_rate: 0.01,
            throughput_rps: 166.666_666_666_666_66,
            latency: sample_latency(),
        },
    ]
}

fn sample_scenarios() -> Vec<ScenarioReport> {
    vec![ScenarioReport {
        name: "checkout".into(),
        requests: sample_requests(),
        latency: sample_latency(),
        status_codes: sample_status_codes(),
        steps: vec![
            ScenarioStepReport {
                name: "login".into(),
                requests: sample_requests(),
                latency: sample_latency(),
                status_codes: sample_status_codes(),
            },
            ScenarioStepReport {
                name: "purchase".into(),
                requests: sample_requests(),
                latency: sample_latency(),
                status_codes: sample_status_codes(),
            },
        ],
    }]
}

fn sample_thresholds() -> ThresholdReport {
    ThresholdReport {
        total: 2,
        passed: 1,
        failed: 1,
        results: vec![
            ThresholdResult {
                threshold: Threshold {
                    metric: Metric::LatencyP99,
                    operator: Operator::Lt,
                    value: 200.0,
                },
                actual: 40.0,
                passed: true,
            },
            ThresholdResult {
                threshold: Threshold {
                    metric: Metric::ErrorRate,
                    operator: Operator::Lte,
                    value: 0.005,
                },
                actual: 0.01,
                passed: false,
            },
        ],
    }
}

/// A canonical `RunReport` with every optional branch populated.
fn canonical_run_report() -> RunReport {
    RunReport {
        version: 2,
        run: RunMeta {
            mode: "curve".into(),
            elapsed_ms: 5000.0,
            curve_duration_ms: Some(5000.0),
            template_generation_ms: Some(12.5),
        },
        requests: sample_requests(),
        latency: sample_latency(),
        status_codes: sample_status_codes(),
        response_stats: Some(sample_response_stats()),
        curve_stages: Some(sample_curve_stages()),
        scenarios: Some(sample_scenarios()),
        thresholds: Some(sample_thresholds()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Pins the full v2 serialization surface. Every `Option` is `Some(..)` and
/// every enum-shaped field emits a representative value.
#[test]
fn run_report_v2_schema_locked_all_fields_present() {
    insta::assert_json_snapshot!(canonical_run_report());
}

/// Pins the minimal v2 surface: every optional section absent. Ensures
/// `None` fields render as `null` (not omitted) and that `skipped: 0` is
/// elided from the `requests` object per the current `skip_serializing_if`
/// rule in `RequestSummary`.
#[test]
fn run_report_v2_schema_locked_all_optionals_absent() {
    let mut report = canonical_run_report();
    report.run.mode = "fixed".into();
    report.run.curve_duration_ms = None;
    report.run.template_generation_ms = None;
    report.response_stats = None;
    report.curve_stages = None;
    report.scenarios = None;
    report.thresholds = None;
    report.requests.skipped = 0;
    insta::assert_json_snapshot!(report);
}
