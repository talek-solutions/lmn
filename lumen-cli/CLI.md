# CLI Reference

## Subcommands

- [`run`](#run) — Execute a load test
- [`configure-request`](#configure-request) — Store a reusable request template
- [`configure-response`](#configure-response) — Store a reusable response template

---

## `run`

Execute a load test against a target host.

```
lumen run [OPTIONS] -H <HOST>
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
| — | `--sample-threshold` | `50` | VU count below which all results are collected (0 = disabled) |
| — | `--result-buffer` | `100000` | Max results to retain for percentile computation |
| — | `--output` | `table` | Output format: `table` (default) or `json` |
| — | `--output-file` | — | Write JSON result to `<path>` (always JSON regardless of `--output`) |

### Conflicts

- `-B`, `-T`, `-A` are mutually exclusive (only one request body source allowed)
- `-S` and `-E` are mutually exclusive (only one response template source allowed)
- `-L` conflicts with `-R` and `-C` (curve mode is time-based, not count-based)

### Output Behaviour Matrix

| `--output` | `--output-file` | stdout | stderr | file |
|-----------|----------------|--------|--------|------|
| `table` (default) | not set | ASCII table | progress | — |
| `json` | not set | JSON document | progress | — |
| `table` | set | ASCII table | progress | JSON document |
| `json` | set | JSON document | progress | JSON document (same content) |

- When `--output json` and no `--output-file`: JSON goes to stdout; ASCII table is suppressed.
- When `--output-file` is set: JSON is always written to the file regardless of `--output`. This allows `--output table --output-file run.json` for users who want both a readable terminal table and a machine-readable artifact.
- All run-time messages (shutdown notice, sampling warnings, errors) always go to stderr.

### Examples

```bash
# Inline body
lumen run -H http://localhost:3000/api -M post -B '{"name":"test"}'

# From a template file with placeholders
lumen run -H http://localhost:3000/api -M post -T ./my-template.json

# From a stored request alias
lumen run -H http://localhost:3000/api -M post -A my-alias

# With a stored response template to track response fields
lumen run -H http://localhost:3000/api -A my-alias -E my-response

# Full example
lumen run -H http://localhost:3000/api -M post -R 1000 -C 50 -A my-alias -E my-response

# Load curve (time-based VU scaling)
lumen run -H http://localhost:3000/api -M post -L ./my-curve.json

# Emit JSON result to stdout instead of ASCII table
lumen run -H http://localhost:3000/api --output json

# Emit ASCII table to terminal AND write JSON artifact to a file
lumen run -H http://localhost:3000/api --output-file run.json

# Both JSON to stdout and to file
lumen run -H http://localhost:3000/api --output json --output-file run.json
```

---

#---

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
lumen configure-request -A <ALIAS> [OPTIONS]
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
lumen configure-request -A my-alias -B '{"name":"test"}'

# Store from an existing file
lumen configure-request -A my-alias -T ./payload.json
```

---

## `configure-response`

Store a reusable response template under an alias.
Templates are saved to `.templates/responses/<alias>.json`.

```
lumen configure-response -A <ALIAS> [OPTIONS]
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
lumen configure-response -A my-response -B '{"status":"ok","id":0}'

# Store from an existing file
lumen configure-response -A my-response -T ./response-shape.json
```

---

## Alias Resolution

When using `--request-alias` or `--response-alias`, the name resolves to:

```
.templates/requests/<alias>.json   # for request aliases
.templates/responses/<alias>.json  # for response aliases
```

The `.json` extension is optional — both `my-alias` and `my-alias.json` are accepted.
