# Request Body Examples

Used with `-T` / `--request-template`. Each file defines a JSON body template with optional placeholder fields that generate unique values per request.

| File | Description |
|------|-------------|
| `static-body.json` | Plain JSON body with no placeholders — sent as-is on every request |
| `string-and-float.json` | Demonstrates string choice lists, float ranges, nested objects, and `:once` semantics |
| `error-simulation.json` | Uses choice lists to inject error-triggering values for fault testing |
| `with-env-token.json` | Mixes `{{ENV:VAR_NAME}}` built-in placeholders (resolved from environment at startup) with regular per-request generators |

See [TEMPLATES.md](../../lumen-cli/TEMPLATES.md) for the full placeholder syntax reference.
