# Thresholds & CI Gating

Set pass/fail criteria for your load test and wire the result into your CI pipeline.

## Defining thresholds

Add a `thresholds` section to your config file:

```yaml
thresholds:
  - metric: latency_p99
    operator: lt
    value: 500.0     # p99 latency must be under 500ms

  - metric: error_rate
    operator: lt
    value: 0.01      # error rate must be under 1%

  - metric: throughput_rps
    operator: gt
    value: 100.0     # must sustain over 100 req/s
```

## Available metrics

| Metric | Description | Unit |
|---|---|---|
| `latency_p50` | 50th percentile latency | ms |
| `latency_p75` | 75th percentile latency | ms |
| `latency_p90` | 90th percentile latency | ms |
| `latency_p95` | 95th percentile latency | ms |
| `latency_p99` | 99th percentile latency | ms |
| `error_rate` | Fraction of non-2xx responses | 0.0–1.0 |
| `throughput_rps` | Requests per second | req/s |

## Operators

| Operator | Meaning |
|---|---|
| `lt` | less than |
| `lte` | less than or equal |
| `gt` | greater than |
| `gte` | greater than or equal |
| `eq` | equal (within floating point epsilon) |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Run completed, all thresholds passed |
| `1` | Error — invalid config, unreachable host |
| `2` | Run completed, one or more thresholds failed |

## CI integration

Exit code `2` automatically fails a CI step, blocking the pipeline. For ready-to-use workflow files see:

- [Gate CI — GitHub Actions](../recipes/ci-github.md)
- [Gate CI — GitLab CI](../recipes/ci-gitlab.md)

!!! tip
    Store `lmn.yaml` in your service repository alongside your code. Treat performance requirements as code — review and version them like any other config.
