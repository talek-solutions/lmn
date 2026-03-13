# Monitoring

The `monitoring` module provides a central registry of tracing span names and structured instrumentation across the loadtest execution pipeline.

Tracing is built on the [`tracing`](https://docs.rs/tracing) crate. Spans and events are no-ops until a subscriber is registered — see [Enabling Output](#enabling-output).

---

## Span Registry

All named spans are defined as constants on `SpanName` in `src/monitoring/spans.rs`. Use these constants when creating spans manually to keep names consistent across the codebase.

| Constant | Span name | Where emitted |
|---|---|---|
| `SpanName::TEMPLATE_PARSE` | `loadtest.template.parse` | `run.rs` — wraps `Template::parse()` |
| `SpanName::TEMPLATE_RENDER` | `loadtest.template.render` | `run.rs` — wraps `Template::pre_generate()` |
| `SpanName::REQUEST` | `loadtest.request` | `run.rs` — wraps each outbound HTTP request |

---

## Instrumented Functions

The following functions carry `#[instrument]` attributes and produce child spans automatically when called inside an active span context.

### Template parsing

| Function | Module | Fields |
|---|---|---|
| `Template::parse` | `template` | `path` |
| `Template::pre_generate` | `template` | `n` (request count) |
| `renderer::validate_placeholders` | `template::renderer` | `def_count` |
| `definition::check_circular_refs` | `template::definition` | `def_count` |

### Response template parsing

| Function | Module | Fields |
|---|---|---|
| `ResponseTemplate::parse` | `response_template` | `path` |

### Generation

| Function | Module | Instrumentation |
|---|---|---|
| `GeneratorContext::generate_by_name` | `template::generator` | `debug!` event when an unknown placeholder resolves to `null` |

`render()` and `resolve()` are intentionally not spanned — they are called once per placeholder per request body and the overhead would exceed the work done.

---

## Span Hierarchy

During a run with a request template, the span tree looks like:

```
loadtest.template.parse
  template::parse           (instrument)
    renderer::validate_placeholders  (instrument)
    definition::check_circular_refs  (instrument)

loadtest.template.render
  template::pre_generate    (instrument)

loadtest.request            (one per HTTP request, async)
```

---

## Enabling Output

No subscriber is wired by default. Add one in `main.rs` before dispatching commands:

**Pretty (human-readable):**
```rust
tracing_subscriber::fmt::init();
```

**JSON (structured, for log ingestion):**
```rust
tracing_subscriber::fmt().json().init();
```

**Filter by level:**
```rust
tracing_subscriber::fmt()
    .with_env_filter("loadtest=debug")
    .init();
```

Add `tracing-subscriber` to `Cargo.toml`:
```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

---

## Adding New Spans

1. Add a `pub const` to `SpanName` in `src/monitoring/spans.rs` following the `loadtest.<domain>.<operation>` naming convention
2. Emit the span at the call site in `run.rs` (for top-level operations) or use `#[instrument]` on the function directly
3. Update the span registry table in this file
