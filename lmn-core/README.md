# lmn-core

Core engine for [lmn](https://github.com/talek-solutions/lmn) — a fast HTTP load testing tool.

This crate provides the building blocks for running load tests programmatically: HTTP execution, dynamic request templating, load curve definitions, result sampling, threshold evaluation, and report generation. The `lmn` CLI is built entirely on top of this crate.

> Full documentation at [https://lmn.talek.cloud](https://lmn.talek.cloud)

## Features

- **Fixed and curve-based execution** — run N requests at fixed concurrency, or drive VU counts dynamically over time with linear or step ramps
- **Dynamic request templates** — JSON templates with typed placeholder generators (strings, floats), `:once` placeholders, and `${ENV_VAR}` secret injection
- **Response field tracking** — extract and aggregate typed fields from response bodies across all requests
- **Two-stage sampling** — VU-threshold gate + Vitter's Algorithm R reservoir to bound memory while preserving statistical accuracy
- **Threshold evaluation** — pass/fail rules on latency percentiles, error rate, and throughput
- **Structured reports** — serializable `RunReport` with percentiles, per-stage breakdowns, status code distribution, and sampling metadata
- **OpenTelemetry tracing** — all major operations are instrumented with named spans

## Usage

```toml
[dependencies]
lmn-core = "0.1"
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

## Key types

| Type | Module | Purpose |
|---|---|---|
| `RunCommand` | `command::run` | Entry point — owns request spec, execution mode, and sampling config |
| `ExecutionMode` | `command::run` | `Fixed { request_count, concurrency }` or `Curve(LoadCurve)` |
| `RequestSpec` | `command::run` | Host, method, body, template paths, headers |
| `SamplingConfig` | `command::run` | VU threshold and reservoir size |
| `RunStats` | `command::run` | Raw output of a completed run |
| `RunReport` | `output` | Serializable report built from `RunStats` |
| `LoadCurve` | `load_curve` | Staged VU ramp definition (parses from JSON) |
| `Threshold` | `threshold` | Single pass/fail rule on a metric |

---

### Minimal example — fixed load test

```rust,no_run
use std::time::Instant;

use lmn_core::command::{Command, Commands, HttpMethod};
use lmn_core::command::run::{ExecutionMode, RequestSpec, RunCommand, SamplingConfig};
use lmn_core::output::{RunReport, RunReportParams};

#[tokio::main]
async fn main() {
    let cmd = RunCommand {
        request: RequestSpec {
            host: "https://example.com/api/ping".to_string(),
            method: HttpMethod::Get,
            body: None,
            template_path: None,
            response_template_path: None,
            headers: vec![],
        },
        execution: ExecutionMode::Fixed {
            request_count: 1000,
            concurrency: 50,
        },
        sampling: SamplingConfig {
            sample_threshold: 50,
            result_buffer: 100_000,
        },
    };

    if let Ok(Some(stats)) = Commands::Run(cmd).execute().await {
        let report = RunReport::from_params(RunReportParams {
            stats: &stats,
            reservoir_size: 100_000,
            run_start: Instant::now(),
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }
}
```

### Load curves

Define time-based VU ramps using `LoadCurve`, which parses from JSON:

```rust,no_run
use lmn_core::load_curve::LoadCurve;
use lmn_core::command::run::ExecutionMode;

let curve: LoadCurve = r#"{
    "stages": [
        { "duration": "30s", "target_vus": 5 },
        { "duration": "2m",  "target_vus": 50, "ramp": "linear" },
        { "duration": "30s", "target_vus": 0,  "ramp": "linear" }
    ]
}"#.parse().unwrap();

let execution = ExecutionMode::Curve(curve);
```

Stage `ramp` defaults to `"linear"` if omitted. Use `"step"` for an immediate jump.

### Request templates

Load a JSON template file with typed placeholder definitions:

```rust,ignore
use lmn_core::request_template::Template;

let template = Template::parse(Path::new("request.json")).unwrap();

// Pre-generate N bodies at startup (used in fixed mode)
let bodies = template.pre_generate(1000);

// Or generate on demand (thread-safe, used in curve mode)
let body = template.generate_one();
```

Template files embed placeholder definitions under `_loadtest_metadata_templates`:

```json
{
  "user_id": "{{user_id}}",
  "amount":  "{{price}}",
  "_loadtest_metadata_templates": {
    "user_id": {
      "type": "string",
      "details": { "length": { "min": 8, "max": 16 } }
    },
    "price": {
      "type": "float",
      "min": 1.0,
      "max": 999.99,
      "details": { "decimals": 2 }
    }
  }
}
```

Environment variables are resolved at template load time:

```json
{ "token": "{{ENV:API_TOKEN}}" }
```

See [Template Placeholders](https://lmn.talek.cloud/reference/template-placeholders/) for the full reference.

### Thresholds

```rust,ignore
use lmn_core::threshold::{evaluate, EvaluateParams, parse_thresholds};

let thresholds = parse_thresholds(r#"{
    "thresholds": [
        { "metric": "latency_p99", "operator": "lt", "value": 200.0 },
        { "metric": "error_rate",  "operator": "lt", "value": 0.01  }
    ]
}"#).unwrap();

let result = evaluate(EvaluateParams { report: &report, thresholds: &thresholds });
if !result.all_passed() {
    eprintln!("thresholds failed: {:?}", result);
}
```

`parse_thresholds` accepts both JSON and YAML strings. Available metrics: `latency_p50`, `latency_p75`, `latency_p90`, `latency_p95`, `latency_p99`, `error_rate`, `throughput_rps`. Operators: `lt`, `lte`, `gt`, `gte`, `eq`.

## Configuration

If you prefer YAML-based configuration, `lmn-core` exposes a full config parser:

```rust,ignore
use lmn_core::config::parse_config;

let config = parse_config(yaml_str).unwrap();
```

See the [Config File Reference](https://lmn.talek.cloud/reference/config/) for the full YAML schema.

## License

Apache-2.0
