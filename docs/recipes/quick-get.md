# Fire a quick GET test

Run 100 GET requests at 10 concurrency and see a full latency breakdown.

```bash
lmn run -H https://api.example.com/health
```

## Increase load

```bash
lmn run -H https://api.example.com/health -R 1000 -C 50
```

`-R` sets the total request count, `-C` sets concurrency.

## Cap throughput

```bash
lmn run -H https://api.example.com/health -R 1000 -C 50 --rps 100
```

`--rps` caps aggregate requests-per-second across all VUs. Useful when you want to probe a service without exceeding its quota or rate limit.

## What you'll see

```
 Results ──────────────────────────────────────────────────────────────────────────
  mode       fixed
  requests   100  (100 ok · 0 failed)
  duration   2.07s
  throughput 48.3 req/s

 Latency ──────────────────────────────────────────────────────────────────────────
  min    142.0ms
  p50    198.0ms
  p95    312.0ms
  p99    401.0ms
  max    487.3ms
  avg    203.1ms

 Status codes ───────────────────────────────────────────────────────────────────────
  200    100  ████████████████████████████
```
