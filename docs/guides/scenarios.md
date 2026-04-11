# Scenarios

Scenarios let you model realistic user flows where each virtual user executes a sequence of HTTP steps — login, browse, add to cart, checkout — instead of hammering a single endpoint.

## When to use scenarios

| Use case | Single request | Scenarios |
|---|---|---|
| API smoke test | Yes | |
| Endpoint latency benchmark | Yes | |
| Multi-step user journey | | Yes |
| Mixed read/write workloads | | Yes |
| Weighted traffic simulation | | Yes |

## How scenarios work

Each scenario is a named list of steps. Every VU assigned to a scenario loops through its steps sequentially, sending one HTTP request per step.

```yaml
scenarios:
  - name: checkout
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
      - name: add_to_cart
        host: https://api.example.com/cart
        method: post
      - name: pay
        host: https://api.example.com/checkout
        method: post
```

When `scenarios` is present, `run.host` and `run.method` are not used — each step defines its own target.

## Weighted VU distribution

The `weight` field controls what proportion of VUs run each scenario. Weights are relative integers (default: 1).

```yaml
scenarios:
  - name: checkout
    weight: 3          # 75% of VUs
    steps: [...]
  - name: browse
    weight: 1          # 25% of VUs
    steps: [...]
```

With `concurrency: 8` and weights `[3, 1]`: 6 VUs run "checkout", 2 run "browse". Assignment is deterministic by VU index.

## Budget and request counting

In **fixed mode**, `request_count` counts **scenario iterations**, not individual HTTP requests. A 3-step scenario with `request_count: 100` produces 100 full iterations = 300 HTTP requests.

```yaml
execution:
  request_count: 100    # 100 full iterations
  concurrency: 10
```

In **curve mode**, there is no budget — VUs loop until the stage ends.

## Step failure handling

The `on_step_failure` field controls what happens when a step receives a non-2xx response:

| Value | Behaviour |
|---|---|
| `continue` (default) | Complete all remaining steps in the iteration |
| `abort_iteration` | Skip remaining steps, start the next iteration |

```yaml
scenarios:
  - name: checkout
    on_step_failure: abort_iteration
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
      - name: pay            # skipped if login fails
        host: https://api.example.com/checkout
        method: post
```

Failed steps always emit a `RequestRecord` with `success: false` — they still show up in metrics and status code counts.

## Header merging

Headers merge in three layers, with case-insensitive last-wins:

1. **Global** — `run.headers` + CLI `--header` flags
2. **Scenario** — `headers` on the scenario
3. **Step** — `headers` on the step

```yaml
run:
  headers:
    Authorization: "Bearer ${API_TOKEN}"     # applied everywhere

scenarios:
  - name: checkout
    headers:
      X-Session: "abc123"                    # applied to all steps in this scenario
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
        headers:
          Content-Type: application/json      # only on this step
```

## Per-step templates

Each step can have its own request and response templates:

```yaml
steps:
  - name: login
    host: https://api.example.com/auth
    method: post
    request_template: templates/login_body.json
    response_template: templates/login_resp.json
  - name: browse
    host: https://api.example.com/products
    method: get
```

## Scenarios with load curves

Scenarios work with curve mode. The curve controls how many VUs are active; each VU runs its assigned scenario.

```yaml
scenarios:
  - name: api_flow
    steps:
      - name: read
        host: https://api.example.com/data
      - name: write
        host: https://api.example.com/data
        method: post

execution:
  stages:
    - duration: 30s
      target_vus: 10
      ramp: linear
    - duration: 2m
      target_vus: 50
    - duration: 30s
      target_vus: 0
      ramp: linear
```

## Output

When scenarios are used, the results include per-scenario and per-step breakdowns:

```
 Scenario: checkout ──────────────────────────────────
  requests   150  (148 ok · 2 failed)
  throughput 25.0 req/s
  latency    p50 45.2ms · p95 120.3ms · p99 245.1ms
  steps
    login              50 req    0.0% err  p95 89.2ms
    add_to_cart        50 req    2.0% err  p95 110.5ms
    pay                50 req    2.0% err  p95 120.3ms
```

JSON output includes the same data under the `scenarios` key.

## Full reference

See [Config File Reference](../reference/config.md#scenarios) for every field, type, and constraint.
