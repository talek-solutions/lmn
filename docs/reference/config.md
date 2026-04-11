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

## `scenarios`

An array of named scenarios, each with one or more steps. Scenarios are **mutually exclusive** with `run.host`, `run.method`, and the top-level `request_template`/`response_template` fields — use one or the other.

When scenarios are present, VUs are assigned to scenarios proportionally by `weight`. Each iteration of a VU runs all steps in its assigned scenario in order.

```yaml
scenarios:
  - name: checkout
    weight: 3
    on_step_failure: continue
    headers:
      X-Session: abc
    steps:
      - name: login
        host: https://api.example.com/auth/login
        method: post
        headers:
          Content-Type: application/json
        request_template: templates/login.json
        response_template: templates/login_resp.json
      - name: add_to_cart
        host: https://api.example.com/cart
        method: post
  - name: browse
    weight: 1
    steps:
      - name: list_products
        host: https://api.example.com/products
```

### Scenario fields

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | Yes | Unique scenario name. Must not be empty |
| `weight` | int | No | Relative weight for VU assignment. Default: `1`. Must be >= 1 |
| `on_step_failure` | string | No | What to do when a step fails. Values: `continue` (default), `abort_iteration` |
| `headers` | map | No | Headers applied to all steps in this scenario. Merged on top of `run.headers` |
| `steps` | array | Yes | Ordered list of steps. At least one step is required |

### Step fields

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | Yes | Unique step name within the scenario. Must not be empty |
| `host` | string | Yes | Full URL for this step including scheme. Supports `${ENV_VAR}` substitution |
| `method` | string | No | HTTP method. Default: `get`. Values: `get`, `post`, `put`, `patch`, `delete` |
| `headers` | map | No | Step-level headers. Merged on top of scenario headers (last-wins, case-insensitive) |
| `request_template` | path | No | Path to a JSON request body template for this step |
| `response_template` | path | No | Path to a JSON response extraction template for this step |

### Header merge order

Headers are merged in this priority order (later overrides earlier, case-insensitive):

1. `run.headers` — global headers applied to all scenarios and steps
2. Scenario `headers` — applied to all steps within the scenario
3. Step `headers` — applied to this step only

### VU assignment

VUs are distributed across scenarios proportionally to their `weight`. With weights `[3, 1]` and 4 VUs: 3 VUs run `checkout`, 1 VU runs `browse`. With 8 VUs the pattern repeats: 6 run `checkout`, 2 run `browse`.

In fixed mode, the `execution.request_count` budget is shared across all VUs regardless of scenario, counting each step individually.
