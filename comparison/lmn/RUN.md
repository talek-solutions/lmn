# lmn — Test Runs

All commands assume the repository root as working directory.

## Test 1 — Fixed execution, GET

10,000 GET requests against a flaky endpoint at concurrency 20. Closed-loop:
each of 20 VUs pulls from a shared budget of 10,000 iterations. Measures raw
request-per-second throughput and tail latency under a steady, saturating
workload.

- Endpoint: `GET /load-test/random-error` (~50% 500s)
- Thresholds: `error_rate < 0.7`, `latency_p95 < 2s`, `throughput_rps >= 10`

```
cargo run -q -p lmn -- run --config comparison/lmn/1_config.yaml
```

## Test 2 — Curve execution, GET

Two-minute load curve: 30s ramp 0 → 20 VUs, 1m hold at 20 VUs, 30s ramp
20 → 0. Same endpoint and thresholds as Test 1. Measures behaviour during
warm-up and cool-down — VU spawn cost, throughput shape over time, tail
latency under non-steady-state load.

- Endpoint: `GET /load-test/random-error`
- Stages: `30s → 20`, `1m @ 20`, `30s → 0` (linear ramps)
- Thresholds: `error_rate < 0.7`, `latency_p95 < 2s`, `throughput_rps >= 10`

```
cargo run -q -p lmn -- run --config comparison/lmn/2_config.yaml
```

## Test 3 — POST with generated body

5,000 POST requests at concurrency 20 with a per-request randomised JSON
body. Body fields are generated declaratively from `3_request.json` using
lmn's template placeholders (`string` choice lists, `float` ranges, length
constraints). Stresses both the HTTP engine and the request-template
generator path.

- Endpoint: `POST /load-test/process`
- Body template: `comparison/lmn/3_request.json`
- Thresholds: `error_rate < 0.01`, `latency_p95 < 2s`, `throughput_rps >= 10`

```
cargo run -q -p lmn -- run --config comparison/lmn/3_config.yaml
```

## Output

Each run writes:

- Console table summary (requests, throughput, latency histogram, status
  codes, threshold pass/fail).
- JSON report at `comparison/lmn/result/<N>_run_report.json`.
