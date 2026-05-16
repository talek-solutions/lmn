---
icon: material/lightning-bolt
---

# Recipes

Copy-paste solutions for specific tasks — no explanation, just the answer. For how things work, see [Guides](../guides/index.md).

| Recipe | What it does |
|---|---|
| [Quick GET test](quick-get.md) | Hit a URL and see latency stats |
| [POST with inline JSON](post-inline.md) | Send a fixed JSON body from the command line |
| [POST with a template](post-template.md) | Generate unique per-request data from a JSON template |
| [Auth headers & secrets](auth-headers.md) | Attach headers without hardcoding secrets |
| [Run from a config file](config-file.md) | Define the full test in `lmn.yaml` |
| [CI gate: GitHub Actions](ci-github.md) | Fail your pipeline when performance regresses |
| [CI gate: GitLab CI](ci-gitlab.md) | Same, for GitLab pipelines |
| [Traffic ramp-up](load-curve.md) | Staged virtual users over time |
| [CI artifact output](ci-artifact.md) | Write JSON results for dashboards and artifact storage |
| [Track error codes in responses](response-template.md) | Extract and aggregate API error codes from response bodies |
| [Multi-step scenarios](scenarios.md) | Define weighted user flows with multiple endpoints per VU |
