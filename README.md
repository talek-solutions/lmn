<p align="center">
  <img src="logo.svg" alt="Lumen" width="200" />
</p>

<h1 align="center">Lumen</h1>

<p align="center">
  Fast HTTP load testing CLI — dynamic templates, threshold-gated CI, and load curves.
</p>

<p align="center">
  <a href="https://github.com/talek-solutions/lumen/actions/workflows/ci.yml"><img src="https://github.com/talek-solutions/lumen/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://crates.io/crates/lmn"><img src="https://img.shields.io/crates/v/lmn.svg" alt="crates.io" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License" /></a>
</p>

---

## Why Lumen

Most load testers answer "how fast is my API?" Lumen also answers "did this release break performance?" — by letting you define pass/fail thresholds and wiring the exit code into CI.

```bash
lmn run -H https://api.example.com/orders \
  --header "Authorization: Bearer ${API_TOKEN}" \
  -f lumen.yaml
# exits 0 if thresholds pass, 2 if they fail
```

```yaml
# lumen.yaml
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

**Homebrew / pre-built binary** *(coming soon)*

**From crates.io:**

```bash
cargo install lmn
```

**From source:**

```bash
cargo install --path lumen-cli
```

**Docker:**

```bash
docker build -f lmn-cli/Dockerfile -t lmn .
docker run --rm lmn run -H http://host.docker.internal:3000/api
```

---

## Quick Start

```bash
# 100 GET requests, see latency table
lmn run -H https://httpbin.org/get

# POST with an inline JSON body
lmn run -H https://httpbin.org/post -M post -B '{"name":"alice"}'

# 1000 requests, 50 concurrent
lmn run -H https://httpbin.org/post -M post -R 1000 -C 50 -B '{"item":"widget"}'

# Authenticated request using an env var
lmn run -H https://api.example.com/orders \
  --header "Authorization: Bearer ${API_TOKEN}"

# Run from a YAML config file
lmn run -f lumen.yaml
```

---

## Authentication and Headers

Attach headers to every request with `--header` (repeatable):

```bash
lmn run -H https://api.example.com \
  --header "Authorization: Bearer ${API_TOKEN}" \
  --header "X-Tenant-ID: acme"
```

Use `${ENV_VAR}` in header values to avoid hardcoding secrets. A `.env` file in the working directory is loaded automatically at startup.

```bash
# .env
API_TOKEN=my-secret-token
```

Headers can also live in the config file:

```yaml
run:
  host: https://api.example.com
  headers:
    Authorization: "Bearer ${API_TOKEN}"
    X-Tenant-ID: "acme"
```

CLI `--header` takes precedence over config `headers:` on the same key.

---

## Dynamic Request Bodies

Generate a unique request body per request from a JSON template:

```json
{
  "userId": "{{user_id}}",
  "amount": "{{amount}}",
  "apiKey": "{{ENV:API_KEY}}",
  "_lumen_metadata_templates": {
    "user_id": {
      "type": "string",
      "details": { "choice": ["user-001", "user-002", "user-003"] }
    },
    "amount": {
      "type": "float",
      "min": 1, "max": 500,
      "details": { "decimals": 2 }
    }
  }
}
```

- `{{placeholder}}` — generates a fresh value per request
- `{{placeholder:once}}` — generates once, reused across all requests
- `{{ENV:VAR_NAME}}` — resolved from environment at startup, no definition needed

```bash
lmn run -H https://api.example.com/orders -M post -T ./template.json
```

Store a template as a reusable alias:

```bash
lmn configure-request -A my-order -T ./template.json
lmn run -H https://api.example.com/orders -M post -A my-order
```

See [`lmn-core/TEMPLATES.md`](lmn-core/TEMPLATES.md) for the full placeholder reference.

---

## Load Curves

Scale virtual users over time with a staged load curve:

```yaml
# lumen.yaml
run:
  host: https://api.example.com
  method: post

execution:
  stages:
    - duration: 30s
      target_vus: 5
    - duration: 2m
      target_vus: 50
      ramp: linear
    - duration: 30s
      target_vus: 0
      ramp: linear

thresholds:
  - metric: latency_p95
    operator: lt
    value: 2000.0
```

```bash
lmn run -f lumen.yaml
```

---

## CI Integration

Use exit code `2` (threshold failure) to gate deployments:

```yaml
# .github/workflows/load-test.yml
name: Load Test

on:
  pull_request:

jobs:
  load-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install lmn
        run: cargo install lmn

      - name: Run load test
        run: lmn run -f lumen.yaml
        env:
          API_TOKEN: ${{ secrets.API_TOKEN }}
```

Exit codes:

| Code | Meaning |
|------|---------|
| `0` | Run completed, all thresholds passed |
| `1` | Error — invalid config, unreachable host |
| `2` | Run completed, one or more thresholds failed |

---

## Observability

Stream traces to any OpenTelemetry-compatible backend:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://my-collector:4318
lmn run -H https://api.example.com
```

Start a local Tempo + Grafana stack from `lmn-cli/`:

```bash
cd lumen-cli && docker compose up -d
# Grafana at http://localhost:3000 → Explore → Tempo
```

---

## Output

```bash
# ASCII table (default)
lmn run -H https://httpbin.org/get

# JSON to stdout
lmn run -H https://httpbin.org/get --output json

# ASCII table + JSON artifact
lmn run -H https://httpbin.org/get --output-file run.json
```

---

## Reference

- [`lmn-cli/CLI.md`](lmn-cli/CLI.md) — full flag and config reference
- [`lmn-core/TEMPLATES.md`](lmn-core/TEMPLATES.md) — template placeholder reference
- [`examples/`](examples/) — ready-to-use configs, templates, and load curves

---

## Project Structure

```
lumen/
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
