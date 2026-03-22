# Quickstart

Get your first load test running in under 2 minutes.

## 1. Run a basic GET test

```bash
lmn run -H https://httpbin.org/get
```

lmn runs 100 requests at 10 concurrency by default and prints a results summary:

```
 Results ──────────────────────────────────────────────────────────────────────────
  mode       fixed
  requests   100  (100 ok · 0 failed)
  duration   2.07s
  throughput 48.3 req/s

 Latency ──────────────────────────────────────────────────────────────────────────
  min    142.0ms
  p10    161.3ms
  p25    178.8ms
  p50    198.0ms
  p75    234.5ms
  p90    289.2ms
  p95    312.0ms
  p99    401.0ms
  max    487.3ms
  avg    203.1ms

 Histogram ─────────────────────────────────────────────────────────────────────────
   142.0ms  ███▌                           8
   184.1ms  ████████████████████████████  28
   226.2ms  ████████████████████          20
   268.4ms  ████████████▌                 14
   310.5ms  █████████                     10
   352.6ms  ████▌                          9
   394.7ms  ██▊                            6
   436.8ms  █▌                             3
   479.0ms  ▊                              1
   487.3ms  ▏                              1

 Status codes ───────────────────────────────────────────────────────────────────────
  200    100  ████████████████████████████
```

## 2. POST with a body

```bash
lmn run -H https://httpbin.org/post -M post -B '{"name":"alice"}'
```

## 3. Increase concurrency and request count

```bash
lmn run -H https://httpbin.org/get -R 1000 -C 50
```

## 4. Add thresholds

Create a config file `lmn.yaml`:

```yaml
run:
  host: https://httpbin.org/get
  method: get

execution:
  request_count: 500
  concurrency: 25

thresholds:
  - metric: error_rate
    operator: lt
    value: 0.01
  - metric: latency_p99
    operator: lt
    value: 1000.0
```

Run it:

```bash
lmn run -f lmn.yaml
echo "Exit code: $?"
```

If all thresholds pass, exit code is `0`. If any fail, exit code is `2`.

## Next Steps

- [Core Concepts](core-concepts.md) — understand VUs, thresholds, and load curves
- [Your First Load Test](../guides/first-load-test.md) — a deeper walkthrough with real output
- [Config Files](../guides/config-files.md) — full YAML config reference
