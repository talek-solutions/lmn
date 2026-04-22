# Contributing to lmn

Thank you for your interest in contributing.

## Reporting Issues

- Search [existing issues](https://github.com/talek-solutions/lmn/issues) before opening a new one
- Use the bug report or feature request templates
- For security vulnerabilities, see [SECURITY.md](SECURITY.md)

## Development Setup

```bash
git clone https://github.com/talek-solutions/lmn.git
cd lmn
cargo build
cargo test --workspace
```

**Install pre-commit hooks** (one-time, per clone):

```bash
git config core.hooksPath .githooks
```

The hook runs `cargo fmt` and `cargo clippy --fix` automatically before each commit and re-stages any fixed files.

A `.env` file in the working directory is loaded automatically — copy `.env.example` if present.

## Running the Observability Stack

```bash
docker compose up -d
# Grafana at http://localhost:3000 → Explore → Tempo
```

## Making Changes

1. Fork the repository and create a branch from `master`
2. Write tests for any new functionality
3. Ensure `cargo fmt`, `cargo clippy`, and `cargo test --workspace` all pass
4. Open a pull request against `master`

## Code Style

- `cargo fmt` (enforced by CI)
- `cargo clippy -- -D warnings` (enforced by CI)
- Prefer structs over long parameter lists for extensibility
- No unsafe code without explicit justification

## Snapshot Tests

Tests under `lmn-core/tests/` use [`insta`](https://insta.rs) to snapshot
serialized output such as `RunReport`. A snapshot mismatch surfaces as a
failing test with a readable diff.

Install the CLI once per workstation:

```bash
cargo install cargo-insta
```

When a snapshot diff is expected (you changed the output on purpose):

1. Run `cargo insta review` at the workspace root.
2. Inspect each pending snapshot and either accept (`a`) or reject (`r`).
3. Commit the updated `.snap` file alongside the code change.

CI runs with `INSTA_UPDATE=no` — snapshot changes must be accepted and
committed locally, never silently updated in CI. Treat snapshot diffs in
review with the same scrutiny as code diffs.

## Commit Messages

Use conventional commit prefixes: `feat:`, `fix:`, `refactor:`, `chore:`, `docs:`
