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
cargo run -p lmn -- run -H https://httpbin.org/get
```

---

## 2. Fixed-count run

Fires exactly 100 requests, then exits.

```bash
cargo run -p lmn -- run -H https://httpbin.org/get -R 100
```

---

## 3. High-VU run with sampling flags

Above the VU threshold, results are sampled to bound memory. Use `--sample-threshold`
to set the VU count at which sampling activates, and `--result-buffer` to cap the
in-memory reservoir size.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -L examples/load-curves/ramp.json \
  --sample-threshold 50 \
  --result-buffer 100000
```

---

## 4. POST with an inline body

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -B '{"name":"alice","email":"alice@example.com"}'
```

---

## 5. Higher load — concurrency

1000 requests with 50 in-flight at a time.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -C 50 \
  -B '{"item":"widget","qty":1}'
```

---

## 6. Run with a request template

Generates a unique body per request using the placeholder template.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 500 \
  -W 2 \
  -T examples/request-bodies/string-and-float.json
```

To store it as a reusable alias first:

```bash
cargo run -p lmn -- configure-request \
  -A create-order \
  -T examples/request-bodies/string-and-float.json

cargo run -p lmn -- run \
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
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 200 \
  -T examples/request-bodies/string-and-float.json \
  -S examples/response-extractions/nested-string.json
```

---

## 8. Full example

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -W 4 \
  -C 50 \
  -T examples/request-bodies/string-and-float.json \
  -S examples/response-extractions/nested-string.json
```

---

## 9. Load curve — ramp up, hold, ramp down

Gradually increases to 50 VUs over 30s, holds for 1 minute, then ramps back to 0.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -L examples/load-curves/ramp.json
```

---

## 10. Load curve — spike

Runs at 20 VUs, instantly spikes to 100 for 10 seconds, then drops back to 20.
Useful for verifying recovery after a burst event.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -L examples/load-curves/spike.json
```

---

## 11. Load curve — stepped

Steps through 10 → 50 → 100 VUs in 30-second increments to find the concurrency
level at which the service degrades.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -L examples/load-curves/stepped.json
```

---

## 12. Load curve — with request template

Combines a ramp curve with per-VU dynamic request body generation.
`-R` and `-C` must not be used alongside `-L`.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -T examples/request-bodies/string-and-float.json \
  -L examples/load-curves/ramp.json
```

---

## 13. JSON output to stdout

Emits a versioned JSON document instead of the ASCII table. Useful for piping into
`jq` or any other tool that consumes structured data.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -R 500 \
  --output json
```

Extract a single metric with `jq`:

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -R 500 \
  --output json | jq '.requests.error_rate'
```

Use it as a CI pass/fail gate:

```bash
error_rate=$(cargo run -p lmn -- run \
  -H https://api.example.com/health \
  -R 1000 \
  --output json | jq '.requests.error_rate')

if (( $(echo "$error_rate > 0.01" | bc -l) )); then
  echo "error rate exceeded threshold: $error_rate"
  exit 1
fi
```

---

## 14. Save JSON report to file

Writes the JSON report to `report.json` while the human-readable table still
appears in the terminal. The file is always JSON regardless of `--output`.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -R 1000 \
  --output-file report.json
```

---

## 15. JSON to stdout and file simultaneously

Produces JSON on stdout for piping and saves a copy to disk in one run.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/post \
  -M post \
  -R 1000 \
  -C 50 \
  --output json \
  --output-file ci-report.json
```

---

## 16. Load curve with JSON output

Runs a ramp curve and emits a JSON report. The `curve_stages` field in the output
contains per-stage latency, throughput, and error rate.

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -L examples/load-curves/ramp.json \
  --output json | jq '.curve_stages[] | {index, target_vus, throughput_rps: .throughput_rps}'
```

Save the full report for later comparison:

```bash
cargo run -p lmn -- run \
  -H https://httpbin.org/get \
  -L examples/load-curves/stepped.json \
  --output-file stepped-report.json
```

---

## 17. Run with a YAML config file

Loads host, request count, and threshold rules from a config file. CLI flags
can still override individual values — here `-R 200` overrides `request_count`
from the config.

```bash
cargo run -p lmn -- run \
  -f examples/configs/ci-pipeline.yaml \
  -R 200
```

The process exits with code 0 when all thresholds pass, or code 2 when one or
more fail. Use `$?` to check in shell scripts:

```bash
cargo run -p lmn -- run \
  -f examples/configs/ci-pipeline.yaml
echo "exit code: $?"
```

---

## 18. Threshold-gated CI gate with config file

Runs a fixed-count test driven entirely by the CI pipeline config, then fails
the pipeline if any threshold is exceeded.

```bash
cargo run -p lmn -- run \
  -f examples/configs/ci-pipeline.yaml \
  --output json \
  --output-file ci-report.json

if [ $? -eq 2 ]; then
  echo "load test thresholds failed — see ci-report.json for details"
  exit 1
fi
```

To combine a load curve with threshold enforcement:

```bash
cargo run -p lmn -- run \
  -H https://api.example.com \
  -f examples/configs/ci-pipeline.yaml \
  -L examples/load-curves/ramp.json \
  --output-file ramp-report.json
```
