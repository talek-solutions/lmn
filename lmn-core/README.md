# lmn-core

Core engine for [lmn](https://github.com/talek-solutions/lmn) — a fast HTTP load testing tool.

This crate provides the building blocks for running load tests programmatically: HTTP execution, dynamic request templating, load curve definitions, result sampling, threshold evaluation, and report generation. The `lmn` CLI is built entirely on top of this crate.

## Features

- **Fixed and curve-based execution** — run N requests at fixed concurrency, or drive VU counts dynamically over time with linear or step ramps
- **Dynamic request templates** — JSON templates with typed placeholder generators (strings, floats, objects), `:once` placeholders, and `${ENV_VAR}` secret injection
- **Response field tracking** — extract and aggregate typed fields from response bodies across all requests
- **Two-stage sampling** — VU-threshold gate + Vitter's Algorithm R reservoir to bound memory while preserving statistical accuracy
- **Threshold evaluation** — pass/fail rules on latency percentiles, error rate, and throughput
- **Structured reports** — serializable `RunReport` with percentiles, per-stage breakdowns, status code distribution, and sampling metadata
- **OpenTelemetry tracing** — all major operations are instrumented with named spans

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
lmn-core = "0.1"
```

### Minimal example — fixed load test

```rust
use lmn_core::command::{Commands, HttpMethod, RunCommand, RunMode, RequestSpec};
use lmn_core::sampling::SamplingParams;

#[tokio::main]
async fn main() {
    let cmd = RunCommand {
        request: RequestSpec {
            host: "https://example.com".to_string(),
            path: "/api/ping".to_string(),
            method: HttpMethod::Get,
            headers: vec![],
            body: None,
            response_template: None,
        },
        mode: RunMode::Fixed {
            request_count: 1000,
            concurrency: 50,
        },
        sampling: SamplingParams::default(),
    };

    if let Some(stats) = Commands::Run(cmd).execute().await {
        let report = lmn_core::output::RunReport::from_params((&stats).into());
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }
}
```

### Load curves

Define time-based VU ramps using the `LoadCurve` type or parse them from JSON:

```rust
use lmn_core::load_curve::LoadCurve;

let curve: LoadCurve = r#"[
    { "duration": "30s", "target_vus": 50, "ramp": "linear" },
    { "duration": "1m",  "target_vus": 50 },
    { "duration": "30s", "target_vus": 0,  "ramp": "linear" }
]"#.parse().unwrap();
```

### Request templates

Load a JSON template file with typed placeholder definitions:

```rust
use lmn_core::request_template::Template;

let template = Template::parse(Path::new("request.json")).unwrap();

// Pre-generate 100 request bodies at startup
let bodies = template.pre_generate(100);

// Or generate on demand (thread-safe)
let body = template.generate_one();
```

Template files embed placeholder definitions under the `_lmn_metadata_templates` key:

```json
{
  "user_id": "{{user_id}}",
  "amount": "{{price}}",
  "_lmn_metadata_templates": {
    "user_id": { "type": "string", "strategy": "generated", "length": 12 },
    "price":   { "type": "float",  "strategy": "range", "min": 1.0, "max": 999.99, "decimals": 2 }
  }
}
```

Secrets are injected from environment variables at template load time:

```json
{ "token": "{{ENV:API_TOKEN}}" }
```

### Thresholds

```rust
use lmn_core::threshold::{evaluate, EvaluateParams, parse_thresholds};

let thresholds = parse_thresholds(vec![
    serde_json::json!({ "metric": "p99", "op": "<", "value": 200.0 }),
    serde_json::json!({ "metric": "error_rate", "op": "<", "value": 0.01 }),
]).unwrap();

let result = evaluate(EvaluateParams { report: &report, thresholds: &thresholds });
if !result.all_passed() {
    eprintln!("thresholds failed: {:?}", result);
}
```

## Configuration

If you prefer YAML-based configuration, `lmn-core` exposes a full config parser:

```rust
use lmn_core::config::parse_config;

let config = parse_config(yaml_str).unwrap();
```

See the [lmn CLI documentation](https://github.com/talek-solutions/lmn/blob/master/lmn-cli/CLI.md) for the full YAML schema.

## License

Apache-2.0
