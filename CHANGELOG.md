# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`--rps` flag and `execution.rps` config field** — cap aggregate requests-per-second across all VUs. Implemented as a shared token-bucket limiter (via `governor`), so output is smoothed rather than bursted at the boundary of each second. Works in both fixed and curve modes; in scenario mode the cap applies per HTTP request, not per iteration. Omit for no rate limit (full-throttle behaviour unchanged).
- Functional test coverage for `--rps` (CLI flag with timed-elapsed assertion + YAML config form).

## [0.3.0]

### Added

- **Multi-step scenarios** — define named sequences of HTTP steps (login → browse → checkout) that VUs execute in order. Each step has its own host, method, headers, and request/response templates.
- **Weighted VU distribution** — assign relative weights to scenarios to control what proportion of VUs run each flow. Assignment is deterministic by VU index.
- **Step failure handling** — configurable `on_step_failure` per scenario: `continue` (default) completes all steps, `abort_iteration` skips remaining steps and starts the next iteration.
- **Three-layer header merging** — headers cascade global → scenario → step with case-insensitive last-wins semantics.
- **Per-scenario and per-step metrics** — CLI output and JSON reports include latency, throughput, error rate, and status code breakdowns at both scenario and step granularity.
- **`scenarios` field in JSON output** — new top-level array in the report schema with nested step data.
- **ScenarioVu execution engine** — new VU type that loops through steps sequentially, with budget claiming per iteration (not per request) in fixed mode.
- **ScenarioResolver** — structured config resolution with `${ENV_VAR}` expansion, method parsing, and template loading per step.
- **Scenario config validation** — scenarios are mutually exclusive with `run.host`/`run.method`; unique names enforced; weight in [1, 10_000]; scenarios count capped at 64.
- **Step chaining with response captures** — new `capture` map on steps extracts values from response bodies via JSON paths (`$.data.access_token`) into a per-iteration, per-VU `CaptureState`. Captured values are injected into subsequent step headers, inline bodies, and template output via `{{capture.KEY}}` placeholders. Captures are string-valued (objects/arrays stringified as compact JSON).
- **Inline `body` field on steps** — mutually exclusive with `request_template`. Supports `{{capture.KEY}}` injection and is capped at 1 MiB.
- **Startup static validation of capture references** — during config resolution, every `{{capture.KEY}}` reference in step headers and bodies is checked against the cumulative set of aliases defined by preceding steps. Undefined references fail the run before any load is generated.
- **Skipped-step accounting** — new `skipped: bool` field on `RequestRecord` and `total_skipped: u64` on `RequestStats`. Steps skipped due to unresolvable captures or `abort_iteration` emit skipped records that contribute to request counts but not to latency histograms or status-code breakdowns. Surfaced in both CLI output (`0 skip`) and JSON output (`requests.skipped`).
- **Dependency-aware iteration abort** — if a step references `{{capture.KEY}}` that isn't in the capture state (prior step failed or server omitted the field), the iteration aborts immediately regardless of `on_step_failure`.
- Scenarios guide, recipe, and config reference documentation (including the capture feature and size caps).
- 5 new functional tests covering scenarios in fixed, curve, abort, JSON output, and per-step stats modes.

### Changed

- **`RequestSpec` is now an enum** — `Single { ... }` for single-endpoint mode, `Scenarios(Vec<ResolvedScenario>)` for multi-step mode. This is a breaking change for programmatic users of `lmn-core`.
- **`DrainMetricsAccumulator`** — extracted shared drain logic from both fixed and curve executors, eliminating code duplication.
- **HashMap keys use `Arc<str>`** — scenario/step accumulator maps avoid per-request `String` heap allocation on the hot path.
- **Single canonical sort** — scenarios and steps are sorted once in `into_stats()` rather than redundantly in three layers.
- JSON output schema version bumped to `2`.

## [0.2.0]

### Breaking

- **JSON output**: `sampling` field removed from the output schema. Latency percentiles are now exact (computed from a full HDR histogram) — no approximation, no sampling state to report.
- **Config**: `result_buffer` and `sample_threshold` config fields removed. The old reservoir sampling pipeline has been replaced; these settings have no effect and will cause a parse error if present.

### Changed

- Histogram-based statistics pipeline — exact latency percentiles for any run size, replacing the reservoir sampling approach
- Worker-pool VU model with Arc-based clone reduction — lower memory overhead at high concurrency
- `CompiledTemplate` struct — template compilation errors are now reported at startup, before any requests fire
- Write-to-buffer renderer with pre-resolved globals — faster per-request body generation
- Replaced `expect`/`unwrap` with a typed `RunError` — no more panics during a run, errors surface cleanly

### Fixed

- CLI reference: default value for `--concurrency` corrected to `10` (was incorrectly documented as `100`)

## [0.1.7]

### Changed
- Restore publish pipeline
- Drop Windows from release targets
- Bump CI dependencies: `actions/setup-python`, `docker/login-action`, `docker/build-push-action`

## [0.1.5]

### Changed
- Improve documentation structure and content
- Set custom domain `lmn.talek.cloud` for docs site
- Fix cargo-deb release pipeline

## [0.1.4]

### Changed
- Update docs URLs to `lmn.talek.cloud`

## [0.1.3]

### Added
- Custom request headers via `--header` CLI flag and `headers:` config section
- `${ENV_VAR}` interpolation in header values for secret management
- `.env` file loading at startup via `dotenvy`
- `SensitiveString` type — secrets are redacted in debug output
- `{{ENV:VAR_NAME}}` built-in template placeholder — resolved from environment at parse time
- Load curve support — time-based VU scaling with `stages:` config
- Threshold-gated CI — exit code `2` on threshold failure
- Response template extraction — track specific fields from response bodies
- OpenTelemetry tracing support via `OTEL_EXPORTER_OTLP_ENDPOINT`
- JSON output mode (`--output json`) and file artifact (`--output-file`)
- GitHub Actions workflows — CI, release (5-target cross-compiled binaries), publish, audit
- Apache 2.0 license
