# Guides

In-depth walkthroughs of lmn's features. Each guide explains how a feature works, when to use it, and how to make good decisions.

For copy-paste-ready commands and config snippets, see [Recipes](../recipes/index.md).

| Guide | What you'll learn |
|---|---|
| [Your First Load Test](first-load-test.md) | End-to-end walkthrough from zero to a threshold-gated CI test |
| [Config Files](config-files.md) | How to define tests in `lmn.yaml`, override values from the CLI, and version-control your test suite |
| [Headers & Authentication](headers-auth.md) | How to attach headers, manage secrets with `${ENV_VAR}`, and use `.env` files |
| [Dynamic Request Bodies](request-bodies.md) | How the template system works, placeholder types, and when to use inline vs. template vs. alias |
| [Thresholds & CI Gating](thresholds-ci.md) | How thresholds work, available metrics, operators, and exit codes |
| [Load Curves](load-curves.md) | How curve mode works, how to design a VU ramp, and when to use curves vs. fixed mode |
| [Scenarios](scenarios.md) | How to define multi-step user flows with weighted VU distribution |
