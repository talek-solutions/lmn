# Lumen

HTTP load testing CLI — part of the [Talek Solutions](https://github.com/talek-solutions) portfolio.

Send concurrent HTTP requests, generate dynamic request bodies from templates, track response fields, and stream traces to any OpenTelemetry-compatible backend.

---

## Features

- Concurrent requests with configurable thread count and in-flight limit
- Dynamic request body generation via JSON templates with typed placeholders
- Response field tracking — extract and aggregate values across all responses
- Graceful shutdown on `Ctrl+C`
- OpenTelemetry tracing over OTLP/HTTP — plug into Tempo, Jaeger, or any OTEL collector

---

## Installation

**From source:**

```bash
cargo install --path lumen-cli
```

**Docker:**

```bash
docker build -f lumen-cli/Dockerfile -t lumen .
docker run --rm lumen run -H http://host.docker.internal:3000/api
```

---

## Quick Start

```bash
# 100 GET requests
lumen run -H https://httpbin.org/get

# POST with an inline body
lumen run -H https://httpbin.org/post -M post -B '{"name":"alice"}'

# 1000 requests, 4 threads, 50 in-flight
lumen run -H https://httpbin.org/post -M post -R 1000 -W 4 -C 50 -B '{"item":"widget"}'

# From a request template (unique body per request)
lumen run -H https://httpbin.org/post -M post -T ./my-template.json

# With response field tracking
lumen run -H https://httpbin.org/post -M post -T ./my-template.json -S ./my-response.json
```

---

## Request Templates

Define a JSON file with typed placeholders — each request gets a freshly generated body:

```json
{
  "amount": "{{amount}}",
  "currency": "{{currency}}",
  "_lumen_metadata_templates": {
    "amount": { "type": "float", "min": 1.0, "max": 500.0, "details": { "decimals": 2 } },
    "currency": { "type": "string", "details": { "choice": ["EUR", "USD", "GBP"] } }
  }
}
```

Store a template as a reusable alias:

```bash
lumen configure-request -A my-order -T ./my-template.json
lumen run -H https://api.example.com/orders -M post -A my-order
```

Supported placeholder types: `string`, `float`, `object`. See [`lumen-core/TEMPLATES.md`](lumen-core/TEMPLATES.md) for the full reference.

---

## Response Templates

Track specific fields from response bodies across all requests:

```json
{
  "error": {
    "code": "{{STRING}}"
  }
}
```

```bash
lumen run -H https://api.example.com/pay -M post -A my-order -S ./response.json
```

After the run, the stats output includes value distributions and mismatch counts for every tracked field.

---

## Observability

Start the local Tempo + Grafana stack from `lumen-cli/`:

```bash
cd lumen-cli && docker compose up -d
```

Traces appear in Grafana at [http://localhost:3000](http://localhost:3000) → Explore → Tempo.

To point at a different collector:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://my-collector:4318
```

---

## CLI Reference

See [`lumen-cli/CLI.md`](lumen-cli/CLI.md) for the full flag and subcommand reference.

---

## Development

```
lumen/
├── lumen-core/     # engine, templates, HTTP, monitoring (library crate)
└── lumen-cli/      # CLI entry point, OTel setup, docker-compose (binary crate)
```

```bash
cargo build
cargo test
```
