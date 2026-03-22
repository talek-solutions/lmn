# Save results as a CI artifact

Write a JSON results file alongside the default table output. Use it for artifact storage, dashboards, Slack notifications, or downstream parsing.

## Write a JSON file

```bash
lmn run -H https://api.example.com/health --output-file results.json
```

The ASCII table is still printed to stdout. `--output-file` writes the machine-readable report in addition.

## JSON-only output (suppress table)

```bash
lmn run -H https://api.example.com/health --output json
```

## In a config file

```yaml
run:
  host: https://api.example.com/health
  method: get
  output_file: lmn-result.json
```

## Upload as a GitHub Actions artifact

```yaml
- name: Run load test
  run: lmn run -f lmn.yaml --output-file lmn-result.json

- name: Upload result
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: lmn-result
    path: lmn-result.json
    retention-days: 14
```

Use `if: always()` so the artifact is uploaded even when thresholds fail — that is when you most need the report.

## Upload as a GitLab CI artifact

```yaml
load-test:
  script:
    - lmn run -f lmn.yaml --output-file lmn-result.json
  artifacts:
    when: always
    paths:
      - lmn-result.json
    expire_in: 14 days
```

## Parse results in shell

```bash
lmn run -f lmn.yaml --output json | jq '.latency.p99'
```

See [CLI Reference](../reference/cli.md) for all output flags.
