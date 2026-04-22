# Config Files

Store your load test configuration in a YAML file so it can be version-controlled alongside your code.

## Why use a config file?

- Reproducible runs — same config, same test every time
- Version control — track changes to your performance requirements
- CI integration — check in `lmn.yaml` next to your service and run it on every PR

## Basic structure

```yaml
run:
  host: https://api.example.com
  method: post
  headers:
    Authorization: "Bearer ${API_TOKEN}"
    Content-Type: application/json

execution:
  request_count: 1000
  concurrency: 50

thresholds:
  - metric: latency_p99
    operator: lt
    value: 500.0
  - metric: error_rate
    operator: lt
    value: 0.01
```

Run it:

```bash
lmn run -f lmn.yaml
```

## CLI flags override config

Any flag passed on the command line takes precedence over the config file. This lets you use a base config and override specific values:

```bash
# Override host for local testing
lmn run -f lmn.yaml -H http://localhost:8080
```

## Using a template

Point to a request body template file:

```yaml
run:
  host: https://api.example.com
  method: post
  request_template: ./templates/order.json
```

Or use a saved alias (see [Template Aliases](../guides/config-files.md)):

```yaml
run:
  host: https://api.example.com
  method: post
  alias: my-order
```

## Curve mode

Replace `request_count`/`concurrency` with `stages` for time-based execution:

```yaml
execution:
  stages:
    - duration: 30s
      target_vus: 10
    - duration: 2m
      target_vus: 50
      ramp: linear
    - duration: 30s
      target_vus: 0
      ramp: linear
```

## Scenarios

Define multi-step user flows instead of a single endpoint. When `scenarios` is present, `run.host` and `run.method` are not used — each step defines its own target.

```yaml
run:
  headers:
    Authorization: "Bearer ${API_TOKEN}"    # global headers apply to all steps

scenarios:
  - name: checkout
    weight: 3                               # 75% of VUs run this scenario
    on_step_failure: abort_iteration        # skip remaining steps on failure
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
      - name: pay
        host: https://api.example.com/checkout
        method: post
  - name: browse
    weight: 1                               # 25% of VUs
    steps:
      - name: list
        host: https://api.example.com/products

execution:
  request_count: 1000                       # 1000 scenario iterations (not requests)
  concurrency: 20
```

Headers merge in three layers: **global** (`run.headers` + CLI) → **scenario** → **step**, with case-insensitive last-wins. Each step can also have its own `request_template` and `response_template`.

Scenarios work with both fixed and curve modes. See [Config File Reference](../reference/config.md#scenarios) for the full schema.

## Full reference

See [Config File Reference](../reference/config.md) for every field, type, default, and constraint.
