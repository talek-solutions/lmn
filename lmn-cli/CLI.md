# CLI Reference

> For guides, recipes, and getting started, see [lmn.talek.cloud](https://lmn.talek.cloud).

## Subcommands

- [`run`](#run) — Execute a load test
- [`configure-request`](#configure-request) — Store a reusable request template
- [`configure-response`](#configure-response) — Store a reusable response template

---

## `run`

Execute a load test against a target host.

```
lmn run [OPTIONS] -H <HOST>
```

### Flags

| Short | Long | Default | Description |
|-------|------|---------|-------------|
| `-H` | `--host` | required | Target host URL |
| `-R` | `--request-count` | `100` | Total number of requests to send |
| `-C` | `--concurrency` | `100` | Max in-flight requests at any time |
| `-M` | `--method` | `get` | HTTP method (`get`, `post`, `put`, `patch`, `delete`) |
| `-B` | `--body` | — | Inline JSON request body |
| `-T` | `--request-template` | — | Path to a request template file |
| `-A` | `--request-alias` | — | Alias of a stored request template |
| `-S` | `--response-template` | — | Path to a response template file |
| `-E` | `--response-alias` | — | Alias of a stored response template |
| `-L` | `--load-curve` | — | Path to a load curve JSON file (time-based VU scaling mode) |
| — | `--output` | `table` | Output format: `table` (default) or `json` |
| — | `--output-file` | — | Write JSON result to `<path>` (always JSON regardless of `--output`) |
| `-f` | `--config` | — | Path to a YAML config file. CLI flags take precedence over config values. |
| — | `--header` | — | Custom HTTP header in `'Name: Value'` format (repeatable) |

### Conflicts

- `-B`, `-T`, `-A` are mutually exclusive (only one request body source allowed)
- `-S` and `-E` are mutually exclusive (only one response template source allowed)
- `-L` conflicts with `-R` and `-C` (curve mode is time-based, not count-based)

### Custom Headers

Use `--header` (repeatable) to attach static HTTP headers to every request:

```bash
lmn run -H http://localhost:3000/api --header 'Authorization: Bearer mytoken' --header 'X-Request-ID: abc123'
```

Headers can also be set in the config file under `run.headers`:

```yaml
run:
  host: https://api.example.com
  headers:
    Authorization: "Bearer ${API_TOKEN}"
    X-Custom-Header: "static-value"
```

**Precedence:** CLI `--header` wins over config `headers:` on the same key (case-insensitive match). Duplicate entries for the same key are removed before the CLI value is added.

**Secret management:** Use `${ENV_VAR}` syntax in header values to avoid hardcoding secrets. The variable must use uppercase letters, digits, and underscores only. A `.env` file in the working directory is loaded automatically at startup (silently ignored if absent). If the referenced variable is not set, lmn exits with an error.

```bash
# .env file
API_TOKEN=my-secret-token

# config file
run:
  headers:
    Authorization: "Bearer ${API_TOKEN}"
```

A startup warning is printed to stderr when a header with a security-sensitive name (e.g. `Authorization`, `X-Api-Key`) contains a plain string value longer than 4 characters without `${` — this is a reminder to use env var substitution instead.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Run completed successfully; all thresholds satisfied (or no thresholds configured) |
| `1` | Run error — invalid arguments, unreachable host, config parse failure, or I/O error |
| `2` | Run completed but one or more threshold rules were not satisfied |

Exit code `2` is only possible when `--config`/`-f` is supplied with a YAML file that contains a `thresholds` section.

### Config File Format

When `--config`/`-f` is supplied, lmn loads a YAML file before the run. CLI flags always take precedence over values in the config file.

**Supported config fields:**

Run parameters are nested under a `run:` section. Execution strategy is configured under `execution:`. The `thresholds:` section is top-level.

**`run:` section**

| Field | Type | CLI equivalent | Description |
|-------|------|---------------|-------------|
| `host` | string | `-H` / `--host` | Target host URL |
| `method` | string | `-M` / `--method` | HTTP method (`get`, `post`, `put`, `patch`, `delete`) |
| `output` | string | `--output` | Output format (`table` or `json`) |
| `output_file` | string | `--output-file` | Path to write JSON report |
| `headers` | map | `--header` | Static HTTP headers sent with every request (key: value pairs) |

**`execution:` section**

| Field | Type | CLI equivalent | Description |
|-------|------|---------------|-------------|
| `request_count` | number | `-R` / `--request-count` | Total requests to send (fixed mode) |
| `concurrency` | number | `-C` / `--concurrency` | Max in-flight requests (fixed mode) |
| `stages` | list | `-L` / `--load-curve` | Load curve stages (curve mode — cannot be combined with `request_count`/`concurrency`) |

When `execution.stages` is present, lmn runs in curve mode. Otherwise it runs in fixed mode using `execution.request_count` and `execution.concurrency`.

**Threshold rule fields:**

| Field | Type | Description |
|-------|------|-------------|
| `metric` | string | One of: `error_rate`, `throughput_rps`, `latency_min`, `latency_avg`, `latency_p50`, `latency_p75`, `latency_p90`, `latency_p95`, `latency_p99`, `latency_max` |
| `operator` | string | One of: `lt`, `lte`, `gt`, `gte`, `eq` |
| `value` | number | Threshold value to compare against |

**Fixed-mode example:**

```yaml
run:
  host: https://httpbin.org
  method: post
  output: table

execution:
  request_count: 500
  concurrency: 50

thresholds:
  - metric: error_rate
    operator: lt
    value: 0.01
  - metric: latency_p99
    operator: lt
    value: 500.0
```

**Curve-mode example:**

```yaml
run:
  host: https://httpbin.org/post
  method: post

execution:
  stages:
    - duration: 30s
      target_vus: 5
      ramp: linear
    - duration: 1m
      target_vus: 20
      ramp: linear
    - duration: 30s
      target_vus: 0
      ramp: linear

thresholds:
  - metric: error_rate
    operator: lt
    value: 0.005
  - metric: latency_p95
    operator: lt
    value: 2000.0
```

### Output Behaviour Matrix

| `--output` | `--output-file` | stdout | stderr | file |
|-----------|----------------|--------|--------|------|
| `table` (default) | not set | ASCII table | progress | — |
| `json` | not set | JSON document | progress | — |
| `table` | set | ASCII table | progress | JSON document |
| `json` | set | JSON document | progress | JSON document (same content) |

- When `--output json` and no `--output-file`: JSON goes to stdout; ASCII table is suppressed.
- When `--output-file` is set: JSON is always written to the file regardless of `--output`. This allows `--output table --output-file run.json` for users who want both a readable terminal table and a machine-readable artifact.
- All run-time messages (shutdown notice, errors) always go to stderr.

### Examples

```bash
# Inline body
lmn run -H http://localhost:3000/api -M post -B '{"name":"test"}'

# From a template file with placeholders
lmn run -H http://localhost:3000/api -M post -T ./my-template.json

# From a stored request alias
lmn run -H http://localhost:3000/api -M post -A my-alias

# With a stored response template to track response fields
lmn run -H http://localhost:3000/api -A my-alias -E my-response

# Full example
lmn run -H http://localhost:3000/api -M post -R 1000 -C 50 -A my-alias -E my-response

# Load curve (time-based VU scaling)
lmn run -H http://localhost:3000/api -M post -L ./my-curve.json

# Emit JSON result to stdout instead of ASCII table
lmn run -H http://localhost:3000/api --output json

# Emit ASCII table to terminal AND write JSON artifact to a file
lmn run -H http://localhost:3000/api --output-file run.json

# Both JSON to stdout and to file
lmn run -H http://localhost:3000/api --output json --output-file run.json

# Custom headers (repeatable)
lmn run -H http://localhost:3000/api --header 'Authorization: Bearer mytoken' --header 'X-Trace-ID: 123'

# Using env var substitution for secrets (API_TOKEN must be set)
lmn run -H http://localhost:3000/api --header 'Authorization: Bearer ${API_TOKEN}'
```

---

## Load Curve JSON Format

When using `-L`/`--load-curve`, provide a JSON file with the following structure:

```json
{
  "stages": [
    { "duration": "30s", "target_vus": 10 },
    { "duration": "1m", "target_vus": 50, "ramp": "linear" },
    { "duration": "30s", "target_vus": 0, "ramp": "step" }
  ]
}
```

### Stage Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `duration` | string | yes | — | Stage duration: `"30s"`, `"2m"`, `"1m30s"` |
| `target_vus` | number | yes | — | Target virtual user count at end of stage |
| `ramp` | string | no | `"linear"` | Ramp type: `"linear"` or `"step"` |

### Ramp Types

- `linear` — Smoothly interpolates VU count from the previous stage's target to this stage's `target_vus` over the stage duration.
- `step` — Immediately jumps to `target_vus` at the start of the stage.

---

## `configure-request`

Store a reusable request body template under an alias.
Templates are saved to `.templates/requests/<alias>.json`.

```
lmn configure-request -A <ALIAS> [OPTIONS]
```

### Flags

| Short | Long | Description |
|-------|------|-------------|
| `-A` | `--alias` | Template alias (required) |
| `-B` | `--body` | Inline JSON body to store |
| `-T` | `--template-path` | Path to an existing JSON file to store |

### Conflicts

- `-B` and `-T` are mutually exclusive

### Examples

```bash
# Store an inline body
lmn configure-request -A my-alias -B '{"name":"test"}'

# Store from an existing file
lmn configure-request -A my-alias -T ./payload.json
```

---

## `configure-response`

Store a reusable response template under an alias.
Templates are saved to `.templates/responses/<alias>.json`.

```
lmn configure-response -A <ALIAS> [OPTIONS]
```

### Flags

| Short | Long | Description |
|-------|------|-------------|
| `-A` | `--alias` | Template alias (required) |
| `-B` | `--body` | Inline JSON body to store |
| `-T` | `--template-path` | Path to an existing JSON file to store |

### Conflicts

- `-B` and `-T` are mutually exclusive

### Examples

```bash
# Store an inline response shape
lmn configure-response -A my-response -B '{"status":"ok","id":0}'

# Store from an existing file
lmn configure-response -A my-response -T ./response-shape.json
```

---

## Alias Resolution

When using `--request-alias` or `--response-alias`, the name resolves to:

```
.templates/requests/<alias>.json   # for request aliases
.templates/responses/<alias>.json  # for response aliases
```

The `.json` extension is optional — both `my-alias` and `my-alias.json` are accepted.
