//! Schema-lock snapshot for the `PublishEnvelope` v1 contract.
//!
//! This test pins the full serialized shape of the envelope sent to the Lumen
//! platform via `POST /v1/runs`. The envelope wraps a `RunReport` with
//! transport metadata (envelope_version, run_id, cli_version, published_at,
//! source). Non-deterministic fields (run_id, published_at) are redacted so
//! the snapshot is stable across runs.
//!
//! When this test fails:
//! 1. If the envelope schema change is intentional, bump `ENVELOPE_VERSION`
//!    and coordinate with the platform team before accepting the new snapshot
//!    via `cargo insta review`.
//! 2. If the change is *not* intentional, treat the diff as a bug.

use std::collections::BTreeMap;

use lmn_core::output::{
    FloatFieldSummary, LatencyStats, RequestSummary, ResponseStatsReport, RunMeta, RunReport,
    ScenarioReport, ScenarioStepReport, StageReport,
};
use lmn_core::publish::PublishEnvelope;
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
        response_stats: Some(ResponseStatsReport {
            responses_parsed: 1000,
            string_fields: {
                let mut sf = BTreeMap::new();
                let mut dist = BTreeMap::new();
                dist.insert("ok".into(), 985);
                dist.insert("error".into(), 15);
                sf.insert("status".into(), dist);
                sf
            },
            float_fields: {
                let mut ff = BTreeMap::new();
                ff.insert(
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
                ff
            },
            mismatch_counts: {
                let mut mc = BTreeMap::new();
                mc.insert("missing_field".into(), 3);
                mc
            },
        }),
        curve_stages: Some(vec![
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
        ]),
        scenarios: Some(vec![ScenarioReport {
            name: "checkout".into(),
            requests: sample_requests(),
            latency: sample_latency(),
            status_codes: sample_status_codes(),
            steps: vec![ScenarioStepReport {
                name: "login".into(),
                requests: sample_requests(),
                latency: sample_latency(),
                status_codes: sample_status_codes(),
            }],
        }]),
        thresholds: Some(ThresholdReport {
            total: 1,
            passed: 1,
            failed: 0,
            results: vec![ThresholdResult {
                threshold: Threshold {
                    metric: Metric::LatencyP99,
                    operator: Operator::Lt,
                    value: 200.0,
                },
                actual: 40.0,
                passed: true,
            }],
        }),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Pins the full `PublishEnvelope` v1 shape with a populated `RunReport`.
/// `run_id` and `published_at` are redacted (non-deterministic).
#[test]
fn publish_envelope_v1_schema_locked() {
    let report = canonical_run_report();
    let envelope = PublishEnvelope::new("0.3.0", &report);

    insta::assert_json_snapshot!(envelope, {
        ".run_id" => "[uuid]",
        ".published_at" => "[timestamp]",
    });
}

/// Pins the minimal envelope: fixed-mode report with all optionals absent.
#[test]
fn publish_envelope_v1_minimal_report() {
    let report = RunReport {
        version: 2,
        run: RunMeta {
            mode: "fixed".into(),
            elapsed_ms: 100.0,
            curve_duration_ms: None,
            template_generation_ms: None,
        },
        requests: RequestSummary {
            total: 10,
            ok: 10,
            failed: 0,
            skipped: 0,
            error_rate: 0.0,
            throughput_rps: 100.0,
        },
        latency: LatencyStats {
            min_ms: 1.0,
            p10_ms: 1.0,
            p25_ms: 1.0,
            p50_ms: 2.0,
            p75_ms: 3.0,
            p90_ms: 4.0,
            p95_ms: 5.0,
            p99_ms: 8.0,
            max_ms: 10.0,
            avg_ms: 2.5,
        },
        status_codes: BTreeMap::new(),
        response_stats: None,
        curve_stages: None,
        scenarios: None,
        thresholds: None,
    };

    let envelope = PublishEnvelope::new("0.3.0", &report);

    insta::assert_json_snapshot!(envelope, {
        ".run_id" => "[uuid]",
        ".published_at" => "[timestamp]",
    });
}
