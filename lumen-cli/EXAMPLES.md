# Examples

## Prerequisites

Start the local observability stack before running any load tests:

```bash
docker compose up -d
```

Traces will be available in Grafana at http://localhost:3000 (Explore → Tempo).

To point at a different collector:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://my-collector:4318
```

---

## 1. Simple endless run

Runs until you press Ctrl+C. Endless mode is the default — no `-R` flag needed.

```bash
cargo run -p lumen -- run -H https://httpbin.org/get
```

---

## 2. Fixed-count run

Fires exactly 100 requests, then exits.

```bash
cargo run -p lumen -- run -H https://httpbin.org/get -R 100
```

---

## 3. High-VU run with sampling flags

Above the VU threshold, results are sampled to bound memory. Use `--sample-threshold`
to set the VU count at which sampling activates, and `--result-buffer` to cap the
in-memory reservoir size.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/get \
  -L lumen-core/.templates.example/curves/ramp.json \
  --sample-threshold 50 \
  --result-buffer 100000
```

---

## 4. POST with an inline body

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -B '{"name":"alice","email":"alice@example.com"}'
```

---

## 5. Higher load — multiple threads and concurrency

1000 requests spread across 4 threads, 50 in-flight at a time.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -W 4 \
  -C 50 \
  -B '{"item":"widget","qty":1}'
```

---

## 6. Run with a request template

Generates a unique body per request using the placeholder template.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 500 \
  -W 2 \
  -T lumen-core/.templates.example/json/placeholder.json
```

To store it as a reusable alias first:

```bash
cargo run -p lumen -- configure-request \
  -A create-order \
  -T lumen-core/.templates.example/json/placeholder.json

cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 500 \
  -W 2 \
  -A create-order
```

---

## 7. Track a response field

httpbin echoes the request body back under a `json` key. The example response
template extracts a nested field from it.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 200 \
  -T lumen-core/.templates.example/json/placeholder.json \
  -S lumen-core/.templates.example/json/responses/error-code.json
```

---

## 8. Full example

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -W 4 \
  -C 50 \
  -T lumen-core/.templates.example/json/placeholder.json \
  -S lumen-core/.templates.example/json/responses/error-code.json
```

---

## 9. Load curve — ramp up, hold, ramp down

Gradually increases to 50 VUs over 30s, holds for 1 minute, then ramps back to 0.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/get \
  -L lumen-core/.templates.example/curves/ramp.json
```

---

## 10. Load curve — spike

Runs at 20 VUs, instantly spikes to 100 for 10 seconds, then drops back to 20.
Useful for verifying recovery after a burst event.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -L lumen-core/.templates.example/curves/spike.json
```

---

## 11. Load curve — stepped

Steps through 10 → 50 → 100 VUs in 30-second increments to find the concurrency
level at which the service degrades.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/get \
  -L lumen-core/.templates.example/curves/stepped.json
```

---

## 12. Load curve — with request template

Combines a ramp curve with per-VU dynamic request body generation.
`-R` and `-C` must not be used alongside `-L`.

```bash
cargo run -p lumen -- run \
  -H https://httpbin.org/post \
  -M post \
  -T lumen-core/.templates.example/json/placeholder.json \
  -L lumen-core/.templates.example/curves/ramp.json
```
