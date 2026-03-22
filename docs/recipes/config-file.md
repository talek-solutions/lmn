# Run from a config file

## Minimal

```yaml
# lmn.yaml
run:
  host: https://api.example.com/health
  method: get

execution:
  request_count: 500
  concurrency: 25
```

```bash
lmn run -f lmn.yaml
```

## With headers and POST body

```yaml
run:
  host: https://api.example.com/orders
  method: post
  body: '{"item":"widget","qty":1}'
  headers:
    Authorization: "Bearer ${API_TOKEN}"

execution:
  request_count: 1000
  concurrency: 50
```

## With a template file

```yaml
run:
  host: https://api.example.com/orders
  method: post
  template_path: ./template.json
  headers:
    Authorization: "Bearer ${API_TOKEN}"

execution:
  request_count: 1000
  concurrency: 50
```

## With thresholds

```yaml
run:
  host: https://api.example.com/orders
  method: post
  headers:
    Authorization: "Bearer ${API_TOKEN}"

execution:
  request_count: 500
  concurrency: 25

thresholds:
  - metric: error_rate
    operator: lt
    value: 0.01
  - metric: latency_p99
    operator: lt
    value: 500.0
```

## Override a value from CLI

```bash
lmn run -f lmn.yaml -R 100
```

See [Config Files](../guides/config-files.md) for full field reference and override precedence.
