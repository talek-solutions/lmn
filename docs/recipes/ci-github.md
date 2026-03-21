# Gate CI — GitHub Actions

Fail your pipeline automatically when performance thresholds are not met. Uses the `lmn` Docker image — no Rust toolchain required.

## Workflow

{% raw %}
```yaml
# .github/workflows/load-test.yml

name: Load Test

on:
  # Run after your deploy workflow completes
  workflow_run:
    workflows: ["Deploy"]
    types: [completed]
    branches: [main]

  # Allow manual runs from the Actions tab
  workflow_dispatch:

jobs:
  load-test:
    name: Threshold-gated load test
    runs-on: ubuntu-latest

    # Only run if the triggering deploy succeeded
    if: ${{ github.event.workflow_run.conclusion == 'success' || github.event_name == 'workflow_dispatch' }}

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Run load test
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/workspace" \
            -w /workspace \
            -e API_TOKEN="${{ secrets.API_TOKEN }}" \
            ghcr.io/talek-solutions/lmn:latest \
            run -f lmn.yaml \
            --output-file /workspace/lmn-result.json

      - name: Upload result artifact
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: lmn-result
          path: lmn-result.json
          retention-days: 14
```
{% endraw %}

## `lmn.yaml` for CI

```yaml
run:
  host: https://api.example.com/health
  method: get
  headers:
    Authorization: "Bearer ${API_TOKEN}"

execution:
  request_count: 200
  concurrency: 10

thresholds:
  - metric: error_rate
    operator: lt
    value: 0.01       # less than 1% errors
  - metric: latency_p99
    operator: lt
    value: 1000.0     # p99 under 1 second
```

## How it works

- **Exit code `0`** — all thresholds passed, job succeeds
- **Exit code `2`** — a threshold failed, job fails and blocks the pipeline
- **Exit code `1`** — config or connectivity error, job fails

The JSON artifact is uploaded with `if: always()` so you can inspect the report even when the test fails — that is when you most need it.

## Prerequisites

- Add `API_TOKEN` as a repository secret under **Settings → Secrets and variables → Actions**
- Commit `lmn.yaml` to the repository root (or adjust the `-f` path)
