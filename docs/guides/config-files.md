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

## Full reference

See [Config File Reference](../reference/config.md) for every field, type, default, and constraint.
