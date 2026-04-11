# Multi-step scenarios

## Minimal two-step flow

```yaml
scenarios:
  - name: read_write
    steps:
      - name: write
        host: https://api.example.com/data
        method: post
      - name: read
        host: https://api.example.com/data

execution:
  request_count: 500
  concurrency: 20
```

```bash
lmn run -f scenarios.yaml
```

## Weighted scenarios

75% of VUs run the heavy flow, 25% run the light flow:

```yaml
scenarios:
  - name: checkout
    weight: 3
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
      - name: pay
        host: https://api.example.com/checkout
        method: post

  - name: browse
    weight: 1
    steps:
      - name: list
        host: https://api.example.com/products

execution:
  request_count: 1000
  concurrency: 40
```

## Abort on failure

Skip remaining steps when a step fails:

```yaml
scenarios:
  - name: transactional
    on_step_failure: abort_iteration
    steps:
      - name: auth
        host: https://api.example.com/auth
        method: post
      - name: transfer
        host: https://api.example.com/transfer
        method: post

execution:
  request_count: 200
  concurrency: 10
```

## Scenarios with a load curve

```yaml
scenarios:
  - name: api_flow
    steps:
      - name: health
        host: https://api.example.com/health
      - name: query
        host: https://api.example.com/search
        method: post

execution:
  stages:
    - duration: 30s
      target_vus: 5
      ramp: linear
    - duration: 2m
      target_vus: 30
    - duration: 30s
      target_vus: 0
      ramp: linear
```

## With global auth headers

```yaml
run:
  headers:
    Authorization: "Bearer ${API_TOKEN}"

scenarios:
  - name: authenticated_flow
    steps:
      - name: profile
        host: https://api.example.com/me
      - name: orders
        host: https://api.example.com/orders

execution:
  request_count: 500
  concurrency: 25
```

## With per-step templates

```yaml
scenarios:
  - name: order_flow
    steps:
      - name: create_order
        host: https://api.example.com/orders
        method: post
        request_template: templates/order.json
        response_template: templates/order_resp.json
      - name: check_status
        host: https://api.example.com/orders/status

execution:
  request_count: 300
  concurrency: 15
```

For how scenarios work, see [Scenarios guide](../guides/scenarios.md).
