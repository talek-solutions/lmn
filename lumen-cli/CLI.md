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
| `-W` | `--threads` | `1` | Number of CPU threads (workers) |
| `-C` | `--concurrency` | `100` | Max in-flight requests at any time |
| `-M` | `--method` | `get` | HTTP method (`get`, `post`, `put`, `patch`, `delete`) |
| `-B` | `--body` | — | Inline JSON request body |
| `-T` | `--request-template` | — | Path to a request template file |
| `-A` | `--request-alias` | — | Alias of a stored request template |
| `-S` | `--response-template` | — | Path to a response template file |
| `-E` | `--response-alias` | — | Alias of a stored response template |

### Conflicts

- `-B`, `-T`, `-A` are mutually exclusive (only one request body source allowed)
- `-S` and `-E` are mutually exclusive (only one response template source allowed)

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
lumen run -H http://localhost:3000/api -M post -R 1000 -W 4 -C 50 -A my-alias -E my-response
```

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
