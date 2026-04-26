# k6 — Test Runs

All commands assume the repository root as working directory.

## Test 1 — Fixed execution, GET

10,000 GET requests against a flaky endpoint at 20 VUs. Closed-loop:
the `shared-iterations` executor distributes 10,000 iterations across
20 VUs. Measures raw request-per-second throughput and tail latency
under a steady, saturating workload.

- Endpoint: `GET /load-test/random-error` (~50% 500s)
- Thresholds: `http_req_failed < 0.7`, `http_req_duration p(95) < 2s`,
  `http_reqs >= 10/s`

```
k6 run --summary-export=comparison/k6/result/1_summary.json comparison/k6/1_script.js
```

## Test 2 — Curve execution, GET

Two-minute load curve via the `ramping-vus` executor: 30s ramp 0 → 20 VUs,
1m hold at 20, 30s ramp 20 → 0. Same endpoint and thresholds as Test 1.
Measures behaviour during warm-up and cool-down — VU spawn cost,
throughput shape over time, tail latency under non-steady-state load.

- Endpoint: `GET /load-test/random-error`
- Stages: `30s → 20`, `1m @ 20`, `30s → 0`
- Thresholds: `http_req_failed < 0.7`, `http_req_duration p(95) < 2s`,
  `http_reqs >= 10/s`

```
k6 run --summary-export=comparison/k6/result/2_summary.json comparison/k6/2_script.js
```

## Test 3 — POST with generated body

5,000 POST requests at 20 VUs with a per-request randomised JSON body.
Body fields are generated imperatively in `buildBody()` (string choice
lists via `pick()`, integer/float ranges via `intBetween()` /
`floatBetween()`, lowercase strings of variable length). Mirrors the
field domains defined in `comparison/lmn/3_request.json`.

- Endpoint: `POST /load-test/process`
- Body: built per iteration in JS
- Thresholds: `http_req_failed < 0.01`, `http_req_duration p(95) < 2s`,
  `http_reqs >= 10/s`

```
k6 run --summary-export=comparison/k6/result/3_summary.json comparison/k6/3_script.js
```

## Output

Each run writes:

- Console summary (`http_req_duration` percentiles, `http_reqs`,
  `http_req_failed`, threshold pass/fail, data sent/received).
- JSON summary at `comparison/k6/result/<N>_summary.json` (via
  `--summary-export`).
