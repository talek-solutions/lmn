# Core Concepts

A brief mental model before you dive into the guides.

## Virtual Users (VUs)

A **virtual user** is a concurrent worker that sends requests in a loop. If you set `concurrency: 50`, lmn runs 50 workers simultaneously, each firing requests as fast as the server responds.

- More VUs = more concurrency = more pressure on the server
- VU count is fixed in **fixed mode** and varies over time in **curve mode**

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
