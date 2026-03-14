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

## 1. Simple GET

100 requests, 1 thread, default concurrency.

```bash
cargo run -- run -H https://httpbin.org/get
```

---

## 2. POST with an inline body

```bash
cargo run -- run \
  -H https://httpbin.org/post \
  -M post \
  -B '{"name":"alice","email":"alice@example.com"}'
```

---

## 3. Higher load — multiple threads and concurrency

1000 requests spread across 4 threads, 50 in-flight at a time.

```bash
cargo run -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -W 4 \
  -C 50 \
  -B '{"item":"widget","qty":1}'
```

---

## 4. Run with a request template

Generates a unique body per request using the placeholder template.

```bash
cargo run -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 500 \
  -W 2 \
  -T .templates.example/json/placeholder.json
```

To store it as a reusable alias first:

```bash
cargo run -- configure-request \
  -A create-order \
  -T .templates.example/json/placeholder.json

cargo run -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 500 \
  -W 2 \
  -A create-order
```

---

## 5. Track a response field

httpbin echoes the request body back under a `json` key. The example response
template extracts a nested field from it.

```bash
cargo run -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 200 \
  -T .templates.example/json/placeholder.json \
  -S .templates.example/json/responses/error-code.json
```

---

## 6. Full example

```bash
cargo run -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -W 4 \
  -C 50 \
  -T .templates.example/json/placeholder.json \
  -S .templates.example/json/responses/error-code.json
```
