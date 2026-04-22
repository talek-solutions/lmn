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

## Step chaining

Steps can capture values from response bodies and inject them into subsequent steps. This enables realistic multi-step flows where each request depends on a previous response — login returns a token, subsequent steps use it.

### Capturing values

Add a `capture` map to a step. Each entry maps an alias to a JSON path (`$.`-prefixed):

```yaml
- name: login
  host: https://api.example.com/auth
  method: post
  body: '{"email": "test@example.com", "password": "secret"}'
  capture:
    token: "$.data.access_token"
    user_id: "$.data.user.id"
```

If the response body is `{"data": {"access_token": "abc", "user": {"id": "42"}}}`, then `token` = `"abc"` and `user_id` = `"42"`.

### Injecting captured values

Use `{% raw %}{{capture.KEY}}{% endraw %}` in header values or `body` strings of subsequent steps:

{% raw %}
```yaml
- name: get_profile
  host: https://api.example.com/me
  method: get
  headers:
    Authorization: "Bearer {{capture.token}}"

- name: update_profile
  host: https://api.example.com/users
  method: put
  body: '{"user_id": "{{capture.user_id}}", "name": "New Name"}'
```
{% endraw %}

### Inline body vs request template

Steps support two mutually exclusive ways to provide a request body:

{% raw %}
- **`request_template`** — a JSON template file with randomised placeholders (e.g. `{{username}}`). Use this when each request needs a unique payload.
- **`body`** — a static string defined directly in the config. Use this for simple or capture-driven payloads.

Both support `{{capture.KEY}}` injection. Specifying both on the same step is a config error.
{% endraw %}

### Full example

{% raw %}
```yaml
scenarios:
  - name: checkout
    on_step_failure: abort_iteration
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
        body: '{"email": "test@example.com", "password": "secret"}'
        capture:
          token: "$.data.access_token"
          user_id: "$.data.user.id"

      - name: add_to_cart
        host: https://api.example.com/cart
        method: post
        headers:
          Authorization: "Bearer {{capture.token}}"
        body: '{"user_id": "{{capture.user_id}}", "item": "widget"}'
        capture:
          cart_id: "$.id"

      - name: pay
        host: https://api.example.com/checkout
        method: post
        headers:
          Authorization: "Bearer {{capture.token}}"
        body: '{"cart_id": "{{capture.cart_id}}"}'

execution:
  request_count: 1000
  concurrency: 10
```
{% endraw %}

### Failure handling

If a captured value is missing at injection time (the earlier step failed or the JSON path didn't match), the current iteration **aborts immediately** regardless of `on_step_failure`. Remaining steps are marked as skipped — they appear in metrics with `skipped: true` but don't affect latency or error rate.

Capture references are validated at config load time: referencing `{% raw %}{{capture.token}}{% endraw %}` without a preceding step that defines `token` in its `capture` map is a startup error.

### Constraints

- Only response bodies are supported — no capture from response headers.
- JSON paths use dot notation only (`$.data.user.id`) — no array indexing (`$.items[0]`).
- All captured values are stored as strings. JSON numbers and booleans are stringified.
- Capture aliases must match `[a-zA-Z0-9_]+`.

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

## How it works under the hood

For the engine-level view — VU-to-scenario assignment, iteration loop, capture state lifecycle, header merging, and startup validation pipeline (with diagrams) — see [Scenarios Internals](scenarios-internals.md).
