#!/usr/bin/env bash
# Functional tests for the lmn CLI against https://httpbin.org
# Usage: ./lmn-cli/tests/functional.sh
# Requires: cargo, jq

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="$REPO_ROOT/target/debug/lmn"
TMPDIR="$(mktemp -d)"
PASS=0
FAIL=0

cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

# ── Build ─────────────────────────────────────────────────────────────────────

echo "Building lmn (debug)..."
cargo build -p lmn --manifest-path "$REPO_ROOT/Cargo.toml" -q
echo "Build OK"
echo

# ── Helpers ───────────────────────────────────────────────────────────────────

run_test() {
    local name="$1"
    local expected_exit="$2"
    shift 2

    local actual_exit=0
    "$BIN" "$@" > "$TMPDIR/stdout.txt" 2> "$TMPDIR/stderr.txt" || actual_exit=$?

    if [[ "$actual_exit" -eq "$expected_exit" ]]; then
        echo "  PASS  $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  $name (expected exit $expected_exit, got $actual_exit)"
        echo "        stdout: $(head -5 "$TMPDIR/stdout.txt")"
        echo "        stderr: $(head -5 "$TMPDIR/stderr.txt")"
        FAIL=$((FAIL + 1))
    fi
}

run_test_with_check() {
    local name="$1"
    local expected_exit="$2"
    local check_fn="$3"
    shift 3

    local actual_exit=0
    "$BIN" "$@" > "$TMPDIR/stdout.txt" 2> "$TMPDIR/stderr.txt" || actual_exit=$?

    if [[ "$actual_exit" -ne "$expected_exit" ]]; then
        echo "  FAIL  $name (expected exit $expected_exit, got $actual_exit)"
        echo "        stderr: $(head -5 "$TMPDIR/stderr.txt")"
        FAIL=$((FAIL + 1))
        return
    fi

    if $check_fn "$TMPDIR/stdout.txt"; then
        echo "  PASS  $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  $name (exit code OK but output check failed)"
        echo "        stdout: $(head -10 "$TMPDIR/stdout.txt")"
        FAIL=$((FAIL + 1))
    fi
}

# ── Fixtures ──────────────────────────────────────────────────────────────────

# Request template with string + float placeholders
cat > "$TMPDIR/request-template.json" << 'EOF'
{
  "name": "{{username}}",
  "amount": "{{price}}",
  "_lmn_metadata_templates": {
    "username": {
      "type": "string",
      "details": { "choice": ["alice", "bob", "carol"] }
    },
    "price": {
      "type": "float",
      "min": 1.0,
      "max": 99.99,
      "details": { "decimals": 2 }
    }
  }
}
EOF

# Short load curve: ramp 0→2 VUs over 5s, hold 2 VUs for 5s, ramp down to 0 over 2s
cat > "$TMPDIR/curve.json" << 'EOF'
{
  "stages": [
    { "duration": "5s", "target_vus": 2, "ramp": "linear" },
    { "duration": "5s", "target_vus": 2, "ramp": "step" },
    { "duration": "2s", "target_vus": 0, "ramp": "linear" }
  ]
}
EOF

# YAML config — fixed mode
cat > "$TMPDIR/config-fixed.yaml" << 'EOF'
run:
  host: https://httpbin.org/get
  method: get

execution:
  request_count: 10
  concurrency: 2
EOF

# YAML config — curve mode
cat > "$TMPDIR/config-curve.yaml" << 'EOF'
run:
  host: https://httpbin.org/get
  method: get

execution:
  stages:
    - duration: "5s"
      target_vus: 2
      ramp: linear
    - duration: "3s"
      target_vus: 0
      ramp: linear
EOF

# YAML config — thresholds that should PASS (very lenient)
cat > "$TMPDIR/config-thresholds-pass.yaml" << 'EOF'
run:
  host: https://httpbin.org/get
  method: get

execution:
  request_count: 10
  concurrency: 2

thresholds:
  - metric: error_rate
    operator: lt
    value: 1.0
  - metric: latency_p99
    operator: lt
    value: 30000.0
EOF

# YAML config — thresholds that should FAIL (impossible values)
cat > "$TMPDIR/config-thresholds-fail.yaml" << 'EOF'
run:
  host: https://httpbin.org/get
  method: get

execution:
  request_count: 10
  concurrency: 2

thresholds:
  - metric: latency_p50
    operator: lt
    value: 0.001
EOF

# ── Tests ─────────────────────────────────────────────────────────────────────

echo "Running functional tests..."
echo

# 1. Fixed GET — no body
run_test "fixed GET no body" 0 \
    run --host https://httpbin.org/get \
    --request-count 20 --concurrency 3

# 2. Fixed POST — inline JSON body
run_test "fixed POST inline body" 0 \
    run --host https://httpbin.org/post \
    --method post \
    --body '{"hello":"world"}' \
    --request-count 20 --concurrency 3

# 3. Fixed POST — request template file
run_test "fixed POST request template" 0 \
    run --host https://httpbin.org/post \
    --method post \
    --request-template "$TMPDIR/request-template.json" \
    --request-count 20 --concurrency 3

# 4. Curve mode
run_test "curve mode" 0 \
    run --host https://httpbin.org/get \
    --load-curve "$TMPDIR/curve.json"

# 5. Custom header
run_test "custom header" 0 \
    run --host https://httpbin.org/get \
    --header "X-Test: lmn-functional" \
    --request-count 10 --concurrency 2

# 6. YAML config — fixed
run_test "yaml config fixed" 0 \
    run --config "$TMPDIR/config-fixed.yaml"

# 7. YAML config — curve
run_test "yaml config curve" 0 \
    run --config "$TMPDIR/config-curve.yaml"

# 8. JSON output format — assert valid JSON with total_requests field
check_json_output() {
    local file="$1"
    jq -e '.requests.total > 0' "$file" > /dev/null 2>&1
}
run_test_with_check "json output format" 0 check_json_output \
    run --host https://httpbin.org/get \
    --request-count 10 --concurrency 2 \
    --output json

# 9. JSON output to file
run_test "json output to file" 0 \
    run --host https://httpbin.org/get \
    --request-count 10 --concurrency 2 \
    --output-file "$TMPDIR/report.json"

check_output_file() {
    jq -e '.requests.total > 0' "$TMPDIR/report.json" > /dev/null 2>&1
}
if check_output_file; then
    echo "  PASS  json output file — report.json contains valid data"
    PASS=$((PASS + 1))
else
    echo "  FAIL  json output file — report.json missing or invalid"
    FAIL=$((FAIL + 1))
fi

# 10. Thresholds — PASS (lenient values, expect exit 0)
run_test "thresholds pass" 0 \
    run --config "$TMPDIR/config-thresholds-pass.yaml"

# 11. Thresholds — FAIL (impossible values, expect exit 2)
run_test "thresholds fail" 2 \
    run --config "$TMPDIR/config-thresholds-fail.yaml"

# ── Summary ───────────────────────────────────────────────────────────────────

echo
echo "Results: $PASS passed, $FAIL failed"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
