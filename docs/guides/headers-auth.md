# Headers & Authentication

Attach headers to every request — including auth tokens, tenant IDs, and API keys.

## CLI flags

Use `--header` (or `-H` is taken by host, use the long form) — repeatable:

```bash
lmn run -H https://api.example.com \
  --header "Authorization: Bearer my-token" \
  --header "X-Tenant-ID: acme"
```

## Environment variable substitution

Use `${ENV_VAR}` in header values to avoid hardcoding secrets:

```bash
lmn run -H https://api.example.com \
  --header "Authorization: Bearer ${API_TOKEN}"
```

lmn resolves `${ENV_VAR}` from the environment at startup. If the variable is not set, lmn prints a warning and sends an empty string.

## .env file

A `.env` file in the working directory is loaded automatically:

```bash
# .env
API_TOKEN=my-secret-token
TENANT_ID=acme
```

```bash
lmn run -H https://api.example.com \
  --header "Authorization: Bearer ${API_TOKEN}" \
  --header "X-Tenant-ID: ${TENANT_ID}"
```

!!! warning
    Never commit `.env` files containing real secrets. Add `.env` to your `.gitignore`.

## Config file headers

Headers can also be defined in your YAML config:

```yaml
run:
  host: https://api.example.com
  headers:
    Authorization: "Bearer ${API_TOKEN}"
    X-Tenant-ID: acme
    Content-Type: application/json
```

## Precedence

When the same header key appears in both CLI `--header` flags and config `headers:`, the **CLI flag wins**.

## Secrets in templates

For secrets embedded in request bodies, use `{{ENV:VAR_NAME}}` in your template file — resolved once at startup, not stored in the template definition:

```json
{
  "api_key": "{{ENV:API_KEY}}"
}
```

See [Dynamic Request Bodies](request-bodies.md) for details.
