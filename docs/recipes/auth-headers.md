# Auth headers & secrets

## CLI

```bash
lmn run -H https://api.example.com/orders \
  --header "Authorization: Bearer ${API_TOKEN}" \
  --header "X-Tenant-ID: acme"
```

## `.env` file

```bash
# .env
API_TOKEN=my-secret-token
TENANT_ID=acme
```

```bash
lmn run -H https://api.example.com/orders \
  --header "Authorization: Bearer ${API_TOKEN}" \
  --header "X-Tenant-ID: ${TENANT_ID}"
```

## Config file

```yaml
run:
  host: https://api.example.com/orders
  headers:
    Authorization: "Bearer ${API_TOKEN}"
    X-Tenant-ID: "${TENANT_ID}"
```

See [Headers & Authentication](../guides/headers-auth.md) for how env var resolution, `.env` loading, and CLI override precedence work.
