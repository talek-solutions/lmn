# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.7]

### Changed
- Restore publish pipeline
- Drop Windows from release targets
- Bump CI dependencies: `actions/setup-python`, `docker/login-action`, `docker/build-push-action`

## [0.1.5]

### Changed
- Improve documentation structure and content
- Set custom domain `lmn.talek.cloud` for docs site
- Fix cargo-deb release pipeline

## [0.1.4]

### Changed
- Update docs URLs to `lmn.talek.cloud`

## [0.1.3]

### Added
- Custom request headers via `--header` CLI flag and `headers:` config section
- `${ENV_VAR}` interpolation in header values for secret management
- `.env` file loading at startup via `dotenvy`
- `SensitiveString` type — secrets are redacted in debug output
- `{{ENV:VAR_NAME}}` built-in template placeholder — resolved from environment at parse time
- Load curve support — time-based VU scaling with `stages:` config
- Threshold-gated CI — exit code `2` on threshold failure
- Response template extraction — track specific fields from response bodies
- OpenTelemetry tracing support via `OTEL_EXPORTER_OTLP_ENDPOINT`
- JSON output mode (`--output json`) and file artifact (`--output-file`)
- GitHub Actions workflows — CI, release (5-target cross-compiled binaries), publish, audit
- Apache 2.0 license
