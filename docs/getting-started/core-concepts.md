# Core Concepts

A brief mental model before you dive into the guides.

## Virtual Users (VUs)

A **virtual user** is a concurrent worker that sends requests in a loop. If you set `concurrency: 50`, lmn runs 50 workers simultaneously, each firing requests as fast as the server responds.

- More VUs = more concurrency = more pressure on the server
- VU count is fixed in **fixed mode** and varies over time in **curve mode**
- In **scenario mode**, each VU executes a multi-step sequence instead of a single request

## Fixed Mode vs. Curve Mode

**Fixed mode** — run a set number of requests at a fixed concurrency level. Simple and deterministic.

```yaml
execution:
  request_count: 1000
  concurrency: 50
```

**Curve mode** — drive VU count up and down over time using a load curve. Models real traffic patterns: ramp up, sustain, ramp down.

```yaml
execution:
  stages:
    - duration: 30s
      target_vus: 0
    - duration: 1m
      target_vus: 50
      ramp: linear
    - duration: 30s
      target_vus: 0
      ramp: linear
```

You cannot mix `stages` with `request_count`/`concurrency` — pick one mode per run.

## RPS cap (`rps`)

`concurrency` controls how many requests can be **in flight**; it does not directly cap throughput. If responses come back in 10 ms, 50 VUs will produce ~5,000 req/s. To pin throughput regardless of server speed, set an aggregate **requests-per-second** cap:

```yaml
execution:
  request_count: 5000
  concurrency: 50
  rps: 200            # ≤ 200 req/s across all VUs, smoothed
```

- Works in both fixed and curve mode
- Implemented as a shared token bucket — output is paced, not bursted at the boundary of each second
- Omit (or set `null`) for no rate limit; VUs run at full throttle
- In scenario mode the cap applies **per HTTP request**, not per iteration: a 5-step scenario at `rps: 50` produces ~10 iterations/sec

## Scenarios

A **scenario** is a named sequence of HTTP steps that a VU executes in order. Instead of every VU hitting the same endpoint, you can model realistic user flows — login, browse, checkout — each with its own host, method, headers, and templates.

```yaml
scenarios:
  - name: checkout
    weight: 3
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
      - name: pay
        host: https://api.example.com/checkout
        method: post
  - name: browse
    weight: 1
    steps:
      - name: list
        host: https://api.example.com/products
```

Key concepts:

- **Weight** controls VU distribution. With weights `[3, 1]` and 4 VUs: 3 run "checkout", 1 runs "browse"
- **Budget** counts scenario iterations, not individual requests. `request_count: 100` means 100 full loops through all steps
- **`on_step_failure`** controls what happens when a step fails: `continue` (default) finishes all steps, `abort_iteration` skips the rest and starts over
- Scenarios work with both **fixed** and **curve** modes
- Scenarios are **mutually exclusive** with `run.host` / `run.method` — each step defines its own target

See [Config File Reference](../reference/config.md#scenarios) for the full schema.

## Thresholds

A **threshold** is a pass/fail rule on a metric. When a run completes, lmn evaluates all thresholds and exits with code `2` if any fail.

```yaml
thresholds:
  - metric: latency_p99
    operator: lt
    value: 500.0    # p99 must be under 500ms
  - metric: error_rate
    operator: lt
    value: 0.01     # fewer than 1% errors
```

Available metrics: `latency_p50`, `latency_p75`, `latency_p90`, `latency_p95`, `latency_p99`, `error_rate`, `throughput_rps`.

## Request Templates

A **template** is a JSON file that defines how request bodies are generated. Each request gets a freshly generated body with typed random values.

```json
{
  "user_id": "{{user_id}}",
  "_lmn_metadata_templates": {
    "user_id": { "type": "string", "details": { "choice": ["alice", "bob", "carol"] } }
  }
}
```

See [Dynamic Request Bodies](../guides/request-bodies.md) for the full reference.

## Sampling

When running at high concurrency or large request counts, lmn uses **two-stage sampling** to bound memory usage:

1. **VU-threshold gate** — when active VUs exceed a threshold (default: 50), results are collected at a reduced rate proportional to `threshold / vus`
2. **Reservoir** — a fixed-size buffer (default: 100,000) using Vitter's Algorithm R ensures the collected sample is statistically uniform

Unsampled totals (total requests, failures) are always tracked accurately — sampling only affects the latency percentile distribution. The output table flags when sampling was active.

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Run completed, all thresholds passed (or no thresholds defined) |
| `1` | Error — invalid config, unreachable host, or other failure |
| `2` | Run completed, one or more thresholds failed |
