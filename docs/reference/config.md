# Config File Reference

Full YAML schema for `lmn.yaml`.

## Top-level fields

```yaml
run:       # Request configuration
execution: # How many requests / how long
thresholds: # Pass/fail rules (optional)
```

## `run`

| Field | Type | Required | Description |
|---|---|---|---|
| `host` | string | Yes* | Target URL including scheme. `*` Required unless passed via `-H` |
| `method` | string | No | HTTP method. Default: `get`. Values: `get`, `post`, `put`, `patch`, `delete` |
| `headers` | map | No | Key-value headers. Values support `${ENV_VAR}` substitution |
| `request_template` | path | No | Path to a JSON request body template |
| `response_template` | path | No | Path to a JSON response extraction template |
| `alias` | string | No | Name of a saved request template alias |
| `output` | string | No | Output format. Values: `table` (default), `json` |
| `output_file` | path | No | Write JSON report to this file (in addition to table output) |

## `execution`

**Fixed mode** — `request_count` and `concurrency` together:

| Field | Type | Required | Description |
|---|---|---|---|
| `request_count` | int | Yes | Total requests to send. Max: `1000000` |
| `concurrency` | int | Yes | Concurrent workers. Max: `1000` |

**Curve mode** — `stages` only (cannot mix with fixed mode fields):

| Field | Type | Required | Description |
|---|---|---|---|
| `stages` | array | Yes | List of load curve stages |

Each stage:

| Field | Type | Required | Description |
|---|---|---|---|
| `duration` | string | Yes | Stage duration. Format: `30s`, `2m`, `1m30s` |
| `target_vus` | int | Yes | Target VU count at end of stage. Max: `1000` |
| `ramp` | string | No | Ramp type. Values: `linear` (default), `step` |

## `thresholds`

Array of threshold rules:

| Field | Type | Required | Description |
|---|---|---|---|
| `metric` | string | Yes | Metric name. See [Thresholds & CI Gating](../guides/thresholds-ci.md) |
| `operator` | string | Yes | Comparison operator: `lt`, `lte`, `gt`, `gte`, `eq` |
| `value` | float | Yes | Threshold value. Must be finite. `error_rate` must be between 0.0 and 1.0 |
