# JSON Output Schema

`lmn` can write a machine-readable JSON report in addition to (or instead of) the default ASCII table output.

```bash
# Write JSON to a file (table still prints to stdout)
lmn run -f lmn.yaml --output-file result.json

# Write JSON to stdout only (no table)
lmn run -f lmn.yaml --output json
```

The schema is versioned. The current version is `2`.

---

## Top-level object

| Field | Type | Always present | Description |
|---|---|---|---|
| `version` | integer | Yes | Schema version. Currently `2`. |
| `run` | object | Yes | Execution mode and timing metadata. |
| `requests` | object | Yes | Aggregated request counts and derived metrics. |
| `latency` | object | Yes | Latency percentiles and summary statistics. All values in **milliseconds**. |
| `status_codes` | object | Yes | HTTP status code counts. Keys are string codes (`"200"`, `"404"`). The key `"error"` covers connection errors with no HTTP response. |
| `response_stats` | object \| null | No | Response body field analysis. Present only when `--response-template` was used. |
| `curve_stages` | array \| null | No | Per-stage breakdown. Present only when `mode == "curve"`. |
| `scenarios` | array \| null | No | Per-scenario breakdown. Present only when scenarios were configured. |
| `thresholds` | object \| null | No | Threshold evaluation results. Present only when thresholds were configured. |

---

## `run`

| Field | Type | Description |
|---|---|---|
| `mode` | string | `"fixed"` or `"curve"` |
| `elapsed_ms` | float | Total wall-clock run time in milliseconds |
| `curve_duration_ms` | float \| null | Total curve duration in milliseconds. `null` in fixed mode. |
| `template_generation_ms` | float \| null | Time spent pre-generating request bodies. `null` when no template was used. |

---

## `requests`

| Field | Type | Description |
|---|---|---|
| `total` | integer | Total requests sent |
| `ok` | integer | Requests with a 2xx response |
| `failed` | integer | Requests with a non-2xx response or connection error |
| `error_rate` | float | `failed / total`. `0.0` when `total == 0` |
| `throughput_rps` | float | `total / elapsed_seconds`. `0.0` when elapsed is zero |

---

## `latency`

All values are in **milliseconds** (`f64`). Field names carry the `_ms` suffix.

| Field | Description |
|---|---|
| `min_ms` | Minimum observed latency |
| `p10_ms` | 10th percentile |
| `p25_ms` | 25th percentile |
| `p50_ms` | 50th percentile (median) |
| `p75_ms` | 75th percentile |
| `p90_ms` | 90th percentile |
| `p95_ms` | 95th percentile |
| `p99_ms` | 99th percentile |
| `max_ms` | Maximum observed latency |
| `avg_ms` | Mean latency |

---

## `curve_stages`

Present only when `mode == "curve"`. An array of stage objects in order.

Each stage object:

| Field | Type | Description |
|---|---|---|
| `index` | integer | 0-based stage index |
| `duration_ms` | float | Configured stage duration in milliseconds |
| `target_vus` | integer | Configured target VU count for this stage |
| `ramp` | string | `"linear"` or `"step"` |
| `requests` | integer | Requests sent during this stage |
| `ok` | integer | 2xx responses in this stage |
| `failed` | integer | Non-2xx or error responses in this stage |
| `error_rate` | float | `failed / requests` for this stage |
| `throughput_rps` | float | Requests per second within this stage's duration window |
| `latency` | object | Same structure as the top-level [`latency`](#latency) object, scoped to this stage |

---

## `scenarios`

Present only when scenarios were configured. An array of scenario objects sorted by name.

Each scenario object:

| Field | Type | Description |
|---|---|---|
| `name` | string | Scenario name |
| `requests` | object | Same structure as the top-level [`requests`](#requests) object, scoped to this scenario |
| `latency` | object | Same structure as the top-level [`latency`](#latency) object, scoped to this scenario |
| `status_codes` | object | HTTP status code counts for this scenario |
| `steps` | array | Per-step breakdown (see below) |

Each step object in `steps`:

| Field | Type | Description |
|---|---|---|
| `name` | string | Step name |
| `requests` | object | Same structure as [`requests`](#requests), scoped to this step |
| `latency` | object | Same structure as [`latency`](#latency), scoped to this step |
| `status_codes` | object | HTTP status code counts for this step |

---

## `thresholds`

Present only when thresholds were configured. Contains aggregate counts and per-rule results.

| Field | Type | Description |
|---|---|---|
| `total` | integer | Total number of threshold rules evaluated |
| `passed` | integer | Number of rules that passed |
| `failed` | integer | Number of rules that failed |
| `results` | array | Per-rule evaluation results (see below) |

Each item in `results`:

| Field | Type | Description |
|---|---|---|
| `threshold.metric` | string | Metric name (e.g. `"latency_p99"`, `"error_rate"`) |
| `threshold.operator` | string | Operator (`"lt"`, `"lte"`, `"gt"`, `"gte"`, `"eq"`) |
| `threshold.value` | float | Configured threshold value |
| `actual` | float | Observed metric value |
| `passed` | boolean | Whether `actual` satisfied the threshold |

---

## `response_stats`

Present only when `--response-template` was used.

| Field | Type | Description |
|---|---|---|
| `responses_parsed` | integer | Number of responses successfully matched against the template |
| `string_fields` | object | Distribution of string-valued field extractions. Outer key: field path. Inner key: extracted value. Value: count. |
| `float_fields` | object | Summary statistics for float-valued field extractions. Key: field path. |
| `mismatch_counts` | object | Count of responses where a tracked field could not be extracted. Key: field path. |

Each entry in `float_fields`:

| Field | Description |
|---|---|
| `min` | Minimum observed value |
| `avg` | Mean |
| `p50` | Median |
| `p95` | 95th percentile |
| `p99` | 99th percentile |
| `max` | Maximum observed value |

---

## Full example

```json
{
  "version": 1,
  "run": {
    "mode": "fixed",
    "elapsed_ms": 2074.3,
    "curve_duration_ms": null,
    "template_generation_ms": null
  },
  "requests": {
    "total": 100,
    "ok": 100,
    "failed": 0,
    "error_rate": 0.0,
    "throughput_rps": 48.3
  },
  "latency": {
    "min_ms": 142.0,
    "p10_ms": 161.3,
    "p25_ms": 178.8,
    "p50_ms": 198.0,
    "p75_ms": 234.5,
    "p90_ms": 289.2,
    "p95_ms": 312.0,
    "p99_ms": 401.0,
    "max_ms": 487.3,
    "avg_ms": 203.1
  },
  "status_codes": {
    "200": 100
  },
  "response_stats": null,
  "curve_stages": null,
  "scenarios": null,
  "thresholds": {
    "total": 2,
    "passed": 2,
    "failed": 0,
    "results": [
      {
        "threshold": { "metric": "latency_p99", "operator": "lt", "value": 500.0 },
        "actual": 401.0,
        "passed": true
      },
      {
        "threshold": { "metric": "error_rate", "operator": "lt", "value": 0.01 },
        "actual": 0.0,
        "passed": true
      }
    ]
  }
}
```
