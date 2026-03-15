# Monitoring

The `monitoring` module provides a central registry of tracing span names and structured instrumentation across the loadtest execution pipeline.

Tracing is built on the [`tracing`](https://docs.rs/tracing) crate, exported via OpenTelemetry to a Grafana Tempo backend. See the project root `docker-compose.yml` to spin up the local observability stack.

---

## Span Registry

All named spans are defined as constants on `SpanName` in `loadtest-core/src/monitoring/spans.rs`.

| Constant | Span name | Where emitted |
|---|---|---|
| `SpanName::RUN` | `loadtest.run` | `loadtest-cli/src/main.rs` — root span for the entire process |
| `SpanName::TEMPLATE_PARSE` | `loadtest.template.parse` | `#[instrument]` on `Template::parse()` |
| `SpanName::TEMPLATE_RENDER` | `loadtest.template.render` | `#[instrument]` on `Template::pre_generate()` |
| `SpanName::TEMPLATE_VALIDATE_PLACEHOLDERS` | `loadtest.template.validate_placeholders` | `#[instrument]` on `renderer::validate_placeholders()` |
| `SpanName::TEMPLATE_CHECK_CIRCULAR_REFS` | `loadtest.template.check_circular_refs` | `#[instrument]` on `definition::check_circular_refs()` |
| `SpanName::RESPONSE_TEMPLATE_PARSE` | `loadtest.response_template.parse` | `#[instrument]` on `ResponseTemplate::parse()` |
| `SpanName::REQUESTS` | `loadtest.requests` | `loadtest-core/src/command/run.rs` — wraps the full worker dispatch and result collection |

---

## Instrumented Functions

The following functions carry `#[instrument]` attributes and produce spans automatically when called inside an active span context.

### Template parsing

| Function | Module | Span name | Fields |
|---|---|---|---|
| `Template::parse` | `template` | `loadtest.template.parse` | `path` |
| `Template::pre_generate` | `template` | `loadtest.template.render` | `n` (request count) |
| `renderer::validate_placeholders` | `template::renderer` | `loadtest.template.validate_placeholders` | `def_count` |
| `definition::check_circular_refs` | `template::definition` | `loadtest.template.check_circular_refs` | `def_count` |

### Response template parsing

| Function | Module | Span name | Fields |
|---|---|---|---|
| `ResponseTemplate::parse` | `response_template` | `loadtest.response_template.parse` | `path` |

### Generation

| Function | Module | Instrumentation |
|---|---|---|
| `GeneratorContext::generate_by_name` | `template::generator` | `debug!` event when an unknown placeholder resolves to `null` |

`render()` and `resolve()` are intentionally not spanned — they are called once per placeholder per request body and the overhead would exceed the work done.

---

## Span Hierarchy

During a run with a request template and response template, the span tree looks like:

```
loadtest.run                                  ← root, main.rs
  loadtest.template.parse                     ← #[instrument]
    loadtest.template.validate_placeholders   ← #[instrument]
    loadtest.template.check_circular_refs     ← #[instrument]
  loadtest.template.render                    ← #[instrument]
  loadtest.response_template.parse            ← #[instrument]
  loadtest.requests                           ← manual span, run.rs
```

> Note: `loadtest.requests` and its worker threads run in separate OS threads and do not inherit the `loadtest.run` span context. They appear as a sibling span in the same trace only if span context is explicitly propagated — this is a known limitation and planned for a future improvement.

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

1. Add a `pub const` to `SpanName` in `loadtest-core/src/monitoring/spans.rs` following the `loadtest.<domain>.<operation>` naming convention
2. Prefer `#[instrument(name = "loadtest.<domain>.<operation>", ...)]` on the function directly; use a manual `info_span!` only for spans that don't map cleanly to a single function
3. Update the span registry table and hierarchy in this file
