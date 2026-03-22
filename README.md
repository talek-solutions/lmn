<p align="center">
  <img src="logo.svg" alt="Lumen" width="200" />
</p>

<h1 align="center">Lumen</h1>

<p align="center">
  Fast HTTP load testing CLI — dynamic templates, threshold-gated CI, and load curves.
</p>

<p align="center">
  <a href="https://github.com/talek-solutions/lmn/actions/workflows/ci.yml"><img src="https://github.com/talek-solutions/lmn/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://crates.io/crates/lmn"><img src="https://img.shields.io/crates/v/lmn.svg" alt="crates.io" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License" /></a>
  <a href="https://docs.lmn.dev"><img src="https://img.shields.io/badge/docs-docs.lmn.dev-blue.svg" alt="Docs" /></a>
</p>

> Full documentation at [docs.lmn.dev](https://docs.lmn.dev)

---

## Why Lumen

Most load testers answer "how fast is my API?" Lumen also answers "did this release break performance?" — by letting you define pass/fail thresholds and wiring the exit code into CI.

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

---

## Installation

```bash
cargo install lmn
```

**Docker (zero-install):**

```bash
docker run --rm ghcr.io/talek-solutions/lmn:latest run -H http://host.docker.internal:3000/api
```

Homebrew and pre-built binaries: see [Installation docs](https://docs.lmn.dev/getting-started/installation/).

---

## Quick Start

```bash
# 100 GET requests, see latency table
lmn run -H https://httpbin.org/get

# POST with an inline JSON body
lmn run -H https://httpbin.org/post -M post -B '{"name":"alice"}'

# Run from a YAML config file
lmn run -f lmn.yaml
```

See the [Quickstart guide](https://docs.lmn.dev/getting-started/quickstart/) for a full walkthrough.

---

## Features

- **[Dynamic request bodies](https://docs.lmn.dev/guides/request-bodies/)** — per-request random data from typed JSON templates
- **[Threshold-gated CI](https://docs.lmn.dev/guides/thresholds-ci/)** — exit code `2` on p99/error-rate/throughput failures; wires into any pipeline
- **[Load curves](https://docs.lmn.dev/guides/load-curves/)** — staged virtual user ramp-up with linear or step profiles
- **[Auth & headers](https://docs.lmn.dev/guides/headers-auth/)** — `${ENV_VAR}` secret injection, `.env` auto-load, repeatable headers
- **[Response tracking](https://docs.lmn.dev/recipes/response-template/)** — extract and aggregate fields from response bodies (e.g. API error codes)
- **[JSON output](https://docs.lmn.dev/reference/json-output/)** — machine-readable report for dashboards and CI artifacts
- **Config files** — full YAML config with CLI flag precedence

---

## Observability

Stream traces to any OpenTelemetry-compatible backend:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://my-collector:4318
lmn run -H https://api.example.com
```

Start a local Tempo + Grafana stack from `lmn-cli/`:

```bash
docker compose up -d
# Grafana at http://localhost:3000 → Explore → Tempo
```

---

## Reference

- [CLI reference](https://docs.lmn.dev/reference/cli/) — full flag and config reference
- [Template placeholders](https://docs.lmn.dev/reference/template-placeholders/) — request and response template reference
- [JSON output schema](https://docs.lmn.dev/reference/json-output/) — machine-readable report structure

---

## Project Structure

```
lmn/
├── lmn-core/     # engine, templates, HTTP, thresholds (library crate)
└── lmn-cli/      # CLI entry point, OTel setup (binary crate)
```

```bash
cargo build
cargo test
```

---

## License

Apache-2.0 — see [LICENSE](LICENSE).
