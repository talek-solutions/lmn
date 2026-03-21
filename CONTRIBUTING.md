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

## Commit Messages

Use conventional commit prefixes: `feat:`, `fix:`, `refactor:`, `chore:`, `docs:`
