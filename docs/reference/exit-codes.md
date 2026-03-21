# Exit Codes

lmn uses exit codes to signal the result of a run, making it easy to integrate with CI/CD pipelines.

| Code | Meaning |
|---|---|
| `0` | Run completed successfully. All thresholds passed (or no thresholds were defined). |
| `1` | Fatal error before or during the run. Examples: invalid config, unknown flags, template parse failure, host unreachable. |
| `2` | Run completed but one or more thresholds failed. The results table marks the failing metrics. |

## Using exit codes in CI

Any CI system that checks command exit codes will automatically catch threshold failures:

```bash
lmn run -f lmn.yaml
# $? is 0, 1, or 2
```

```yaml
# GitHub Actions — step fails automatically on exit code != 0
- name: Load test
  run: lmn run -f lmn.yaml
```

```bash
# Shell — explicit check
lmn run -f lmn.yaml
if [ $? -eq 2 ]; then
  echo "Performance thresholds failed — blocking deploy"
  exit 1
fi
```

!!! tip
    Exit code `2` is distinct from `1` so you can differentiate "the test ran and performance regressed" from "the test couldn't run at all" in your pipeline logic.
