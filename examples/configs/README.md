# Config File Examples

Used with `-f` / `--config`. Each file bundles run parameters, load curve, and threshold rules into a single YAML file. CLI flags always take precedence over values in the config file.

| File | Description |
|------|-------------|
| `minimal.yaml` | Minimal config: host and basic thresholds only |
| `ci-pipeline.yaml` | CI-optimized: tight thresholds, ramp curve, JSON output |
| `full.yaml` | All supported fields documented with inline comments |
