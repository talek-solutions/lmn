# Gate CI — GitLab CI

Fail your pipeline automatically when performance thresholds are not met. Uses the `lmn` Docker image directly as the job image — no Rust toolchain required.

## Pipeline snippet

Add this to your `.gitlab-ci.yml`:

```yaml
stages:
  - deploy      # your existing deploy stage
  - load-test

load-test:
  stage: load-test
  image: ghcr.io/talek-solutions/lmn:latest

  variables:
    # Injected from project CI/CD variables (Settings → CI/CD → Variables).
    # Mark it as masked and protected.
    API_TOKEN: $API_TOKEN

  script:
    - lmn run -f lmn.yaml --output-file lmn-result.json

  artifacts:
    name: "lmn-result-$CI_COMMIT_SHORT_SHA"
    when: always
    paths:
      - lmn-result.json
    expire_in: 14 days

  # Only run on the main branch after a successful deploy
  rules:
    - if: $CI_COMMIT_BRANCH == "main"
      when: on_success
```

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
- **Exit code `2`** — a threshold failed, job fails and blocks downstream stages or merge requests
- **Exit code `1`** — config or connectivity error, job fails

The artifact is collected with `when: always` so the JSON report is available even when the job fails.

## Prerequisites

- Add `API_TOKEN` to **Settings → CI/CD → Variables**, marked as masked and protected
- Commit `lmn.yaml` to the repository root (or adjust the `-f` path)
