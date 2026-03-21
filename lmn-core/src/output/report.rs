use std::collections::BTreeMap;

use serde::Serialize;

use crate::threshold::ThresholdReport;

// ── RunReport ─────────────────────────────────────────────────────────────────

/// Versioned, serialization-ready report of a completed load test run.
///
/// This is the canonical contract between the Rust engine and all downstream
/// consumers (CLI JSON output, NestJS SaaS platform, CI pipelines, cloud mode).
/// The top-level `version` field allows consumers to gate on the schema version.
#[derive(Serialize, Debug)]
pub struct RunReport {
    /// Schema version. Currently `1`. Increment on any breaking schema change.
    pub version: u32,
    pub run: RunMeta,
    pub requests: RequestSummary,
    pub latency: LatencyStats,
    /// HTTP status code counts keyed by string code (e.g. `"200"`, `"404"`).
    /// The special key `"error"` covers connection errors with no HTTP response.
    pub status_codes: BTreeMap<String, usize>,
    pub sampling: SamplingInfo,
    /// Present only when `--response-template` / `--response-alias` was used.
    /// `null` when no response template was configured.
    pub response_stats: Option<ResponseStatsReport>,
    /// Present only when `mode == "curve"`. `null` in fixed mode.
    pub curve_stages: Option<Vec<StageReport>>,
    /// Present when thresholds were evaluated after the run. `null` otherwise.
    pub thresholds: Option<ThresholdReport>,
}

// ── RunMeta ───────────────────────────────────────────────────────────────────

/// Top-level metadata about the run's execution mode and timings.
#[derive(Serialize, Debug)]
pub struct RunMeta {
    /// Execution mode: `"fixed"` or `"curve"`.
    pub mode: String,
    /// Total wall-clock elapsed time for the run in milliseconds.
    pub elapsed_ms: f64,
    /// Total curve duration in milliseconds. `null` in fixed mode.
    pub curve_duration_ms: Option<f64>,
    /// Time spent pre-generating template bodies in milliseconds. `null` when no
    /// request template was used.
    pub template_generation_ms: Option<f64>,
}

// ── RequestSummary ────────────────────────────────────────────────────────────

/// Aggregated request counts and derived throughput / error rate metrics.
///
/// `total` and `failed` come from `SamplingState` — they are the unsampled,
/// authoritative counts regardless of reservoir sampling.
#[derive(Serialize, Debug)]
pub struct RequestSummary {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    /// Fraction of failed requests: `failed / total`. `0.0` when `total == 0`.
    pub error_rate: f64,
    /// Requests per second: `total / elapsed_seconds`. `0.0` when elapsed is zero.
    pub throughput_rps: f64,
}

// ── LatencyStats ──────────────────────────────────────────────────────────────

/// Snapshot of latency percentiles and summary statistics in milliseconds.
///
/// All values are `f64` milliseconds. Fields are named with the `_ms` suffix
/// to make the unit self-documenting for downstream consumers.
#[derive(Serialize, Debug)]
pub struct LatencyStats {
    pub min_ms: f64,
    pub p10_ms: f64,
    pub p25_ms: f64,
    pub p50_ms: f64,
    pub p75_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
}

// ── SamplingInfo ──────────────────────────────────────────────────────────────

/// Describes the VU-threshold + reservoir sampling state at end of run.
///
/// Consumers should display a disclaimer when `sampled == true`, as percentile
/// values are based on a random sample of the full request population.
#[derive(Serialize, Debug)]
pub struct SamplingInfo {
    /// `true` if `min_sample_rate < 1.0` at any point during the run.
    pub sampled: bool,
    /// VU-threshold sample rate at end of run (`1.0` = no threshold active).
    pub final_sample_rate: f64,
    /// Lowest sample rate observed; drives the "percentiles are approximate" warning.
    pub min_sample_rate: f64,
    /// Configured reservoir cap (`--result-buffer`).
    pub reservoir_size: usize,
    /// Actual results in the reservoir buffer (≤ `reservoir_size`).
    pub results_collected: usize,
}

// ── ResponseStatsReport ───────────────────────────────────────────────────────

/// Summary of response body field analysis from a response template.
///
/// This is derived from `ResponseStats` in the response template domain.
/// `HashMaps` are promoted to `BTreeMap`s for stable JSON key ordering.
#[derive(Serialize, Debug)]
pub struct ResponseStatsReport {
    /// Number of results in the reservoir whose response body was valid JSON and
    /// matched the response template. Reflects only the parsed sample, not the full
    /// unsampled population when sampling was active.
    pub responses_parsed: usize,
    /// Distribution of string-valued field extractions. Outer key is the field path,
    /// inner key is the extracted value, value is the count.
    pub string_fields: BTreeMap<String, BTreeMap<String, usize>>,
    /// Summary statistics for float-valued field extractions.
    pub float_fields: BTreeMap<String, FloatFieldSummary>,
    /// Count of responses where a tracked field could not be extracted.
    pub mismatch_counts: BTreeMap<String, usize>,
}

/// Summary statistics for a float response field.
#[derive(Serialize, Debug)]
pub struct FloatFieldSummary {
    pub min: f64,
    pub avg: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

// ── StageReport ───────────────────────────────────────────────────────────────

/// Per-stage metrics for a curve-mode run. Stage indices are 0-based.
#[derive(Serialize, Debug)]
pub struct StageReport {
    /// 0-based stage index in the load curve.
    pub index: usize,
    /// Configured stage duration in milliseconds.
    pub duration_ms: f64,
    /// Configured target VU count for this stage.
    pub target_vus: u32,
    /// Ramp type: `"linear"` or `"step"`.
    pub ramp: String,
    pub requests: usize,
    pub ok: usize,
    pub failed: usize,
    /// Fraction of failed requests within this stage.
    pub error_rate: f64,
    /// Requests per second within this stage's duration window.
    pub throughput_rps: f64,
    pub latency: LatencyStats,
}
