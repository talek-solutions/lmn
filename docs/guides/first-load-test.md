# Your First Load Test

A walkthrough from zero to a threshold-gated result — and how to interpret what you see.

## What we're testing

We'll use [httpbin.org](https://httpbin.org) as a safe public target. It echoes requests back so you can inspect what lmn sends.

## Step 1: Run a baseline

```bash
lmn run -H https://httpbin.org/get -R 200 -C 20
```

This fires 200 GET requests at 20 concurrent workers. You'll see a results table when it completes.

**Reading the output:**

- **p50 (median)** — half your requests were faster than this. This is your typical experience.
- **p99** — 99% of requests completed faster than this. This is your worst-case tail latency.
- **Error Rate** — percentage of non-2xx responses. Anything above 0% is worth investigating.
- **Throughput** — requests per second sustained over the run.

!!! tip
    p99 is the number to watch for SLA purposes. A p50 of 100ms with a p99 of 2000ms means occasional users are hitting a very slow path.

## Step 2: Set a threshold

Now make the run fail if performance degrades. Create `lmn.yaml`:

```yaml
run:
  host: https://httpbin.org/get
  method: get

execution:
  request_count: 200
  concurrency: 20

thresholds:
  - metric: latency_p99
    operator: lt
    value: 2000.0   # p99 must be under 2 seconds
  - metric: error_rate
    operator: lt
    value: 0.0      # zero errors
```

```bash
lmn run -f lmn.yaml
echo "Exit: $?"
```

If thresholds pass, you'll see a green summary and exit code `0`. If they fail, exit code is `2` and the table marks the failing metrics.

## Step 3: Understand what causes failures

**High p99** is usually caused by:
- Server-side slow paths (database queries, external calls)
- Resource contention at high concurrency — try reducing `-C` to isolate

**High error rate** is usually caused by:
- Rate limiting (429s) — reduce concurrency or add a lower request count
- Server overload (503s) — your concurrency is too high for the server
- Auth failures (401s) — check your headers

## Step 4: Wire it into CI

{% raw %}
```yaml
# .github/workflows/load-test.yml
- name: Run load test
  run: lmn run -f lmn.yaml
  env:
    API_TOKEN: ${{ secrets.API_TOKEN }}
```
{% endraw %}

Exit code `2` will fail the workflow step. See [Thresholds & CI Gating](thresholds-ci.md) for a complete example.
