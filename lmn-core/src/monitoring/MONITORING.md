# Monitoring

The `monitoring` module provides a central registry of tracing span names and structured instrumentation across the loadtest execution pipeline.

Tracing is built on the [`tracing`](https://docs.rs/tracing) crate, exported via OpenTelemetry to a Grafana Tempo backend. See the project root `docker-compose.yml` to spin up the local observability stack.

---

## Span Registry

All named spans are defined as constants on `SpanName` in `lmn-core/src/monitoring/spans.rs`.

| Constant | Span name | Where emitted |
|---|---|---|
| `SpanName::RUN` | `lmn.run` | `lmn-cli/src/main.rs` — root span for the entire process |
| `SpanName::TEMPLATE_PARSE` | `lmn.template.parse` | `#[instrument]` on `Template::parse()` |
| `SpanName::TEMPLATE_VALIDATE_PLACEHOLDERS` | `lmn.template.validate_placeholders` | `#[instrument]` on `renderer::validate_placeholders()` |
| `SpanName::TEMPLATE_CHECK_CIRCULAR_REFS` | `lmn.template.check_circular_refs` | `#[instrument]` on `definition::check_circular_refs()` |
| `SpanName::RESPONSE_TEMPLATE_PARSE` | `lmn.response_template.parse` | `#[instrument]` on `ResponseTemplate::parse()` |
| `SpanName::REQUESTS` | `lmn.requests` | `lmn-core/src/execution/fixed/mod.rs` — wraps the full worker dispatch and result collection |

---

## Instrumented Functions

The following functions carry `#[instrument]` attributes and produce spans automatically when called inside an active span context.

### Template parsing

| Function | Module | Span name | Fields |
|---|---|---|---|
| `Template::parse` | `request_template` | `lmn.template.parse` | `path` |
| `renderer::validate_placeholders` | `request_template::renderer` | `lmn.template.validate_placeholders` | `def_count` |
| `definition::check_circular_refs` | `request_template::definition` | `lmn.template.check_circular_refs` | `def_count` |

### Response template parsing

| Function | Module | Span name | Fields |
|---|---|---|---|
| `ResponseTemplate::parse` | `response_template` | `lmn.response_template.parse` | `path` |

---

## Design: No Per-Request Tracing

Per-request body generation (`Template::generate_one`) and placeholder resolution are intentionally **not** instrumented. Creating a span per HTTP request would add overhead to the hot path and distort the latency measurements that lmn exists to collect.

Errors during request execution (template serialization failures, capture injection failures) are reported via `eprintln!` so they are always visible to the operator regardless of whether an OTLP collector is running.

---

## Span Hierarchy

During a run with a request template and response template, the span tree looks like:

```
lmn.run                                  ← root, main.rs
  lmn.template.parse                     ← #[instrument]
    lmn.template.validate_placeholders   ← #[instrument]
    lmn.template.check_circular_refs     ← #[instrument]
  lmn.response_template.parse            ← #[instrument]
  lmn.requests                           ← manual span, fixed/mod.rs
```

> Note: `lmn.requests` and its worker threads run in separate OS threads and do not inherit the `lmn.run` span context. They appear as a sibling span in the same trace only if span context is explicitly propagated — this is a known limitation and planned for a future improvement.

---

## Enabling Output

The OTLP exporter is wired in `main.rs` and sends spans to the endpoint configured via the `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable (default: `http://localhost:4318`).

Start the local stack:
```bash
docker compose up -d
# Grafana at http://localhost:3000 → Explore → Tempo
```

Point at a different backend:
```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://my-collector:4318 cargo run -- run ...
```

---

## Adding New Spans

1. Add a `pub const` to `SpanName` in `lmn-core/src/monitoring/spans.rs` following the `lmn.<domain>.<operation>` naming convention
2. Prefer `#[instrument(name = "lmn.<domain>.<operation>", ...)]` on the function directly; use a manual `info_span!` only for spans that don't map cleanly to a single function
3. Avoid instrumenting hot-path code (anything called once per request) — the overhead distorts measurements
4. Update the span registry table and hierarchy in this file
