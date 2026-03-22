# Lumen

**Fast HTTP load testing CLI** — dynamic templates, threshold-gated CI, and load curves.

Most load testers answer "how fast is my API?" Lumen also answers "did this release break performance?" — by letting you define pass/fail thresholds and wiring the exit code directly into CI.

```bash
lmn run -H https://api.example.com/orders \
  --header "Authorization: Bearer ${API_TOKEN}" \
  -f lmn.yaml
# exits 0 if thresholds pass, 2 if they fail
```

```yaml
# lmn.yaml
execution:
  request_count: 1000
  concurrency: 50

thresholds:
  - metric: error_rate
    operator: lt
    value: 0.01        # < 1% errors
  - metric: latency_p99
    operator: lt
    value: 500.0       # p99 < 500ms
```

## Key Features

- **Single binary** — no runtime, no dependencies, install with `cargo install lmn`
- **Declarative YAML config** — version-control your load test alongside your code
- **Threshold-gated CI** — exit code `2` on failure, plug into any pipeline
- **Dynamic request bodies** — per-request JSON generation with typed placeholders
- **Load curves** — ramp VUs up and down over time with linear or step profiles
- **Response tracking** — extract and aggregate fields from response bodies
- **OpenTelemetry** — stream traces to Grafana, Tempo, or any OTLP backend

## Where to go next

- [Installation](installation.md) — cargo, Docker, or pre-built binary
- [Quickstart](quickstart.md) — your first load test in 2 minutes
- [Core Concepts](core-concepts.md) — VUs, thresholds, curves explained
- [CLI Reference](../reference/cli.md) — every flag and subcommand
