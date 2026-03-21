# Dynamic Request Bodies

Generate a unique JSON body per request using typed placeholders.

## Why use templates?

Static bodies send the same payload on every request. Templates let you generate realistic, varied data — different user IDs, random amounts, unique identifiers — so your load test exercises server-side logic rather than hitting a cache.

## Basic example

Create `template.json`:

```json
{
  "userId": "{{user_id}}",
  "amount": "{{amount}}",
  "_lmn_metadata_templates": {
    "user_id": {
      "type": "string",
      "details": { "choice": ["user-001", "user-002", "user-003"] }
    },
    "amount": {
      "type": "float",
      "min": 1.0,
      "max": 500.0,
      "details": { "decimals": 2 }
    }
  }
}
```

Run it:

```bash
lmn run -H https://api.example.com/orders -M post -T ./template.json
```

Each request body will have a randomly chosen `userId` and a random `amount` between 1.00 and 500.00.

## Placeholder types

**String — choice list:**
```json
"user_id": { "type": "string", "details": { "choice": ["alice", "bob", "carol"] } }
```

**String — generated:**
```json
"session_id": {
  "type": "string",
  "details": {
    "length": { "min": 8, "max": 16 },
    "uppercase": 2,
    "lowercase": 4
  }
}
```

**Float:**
```json
"price": { "type": "float", "min": 0.01, "max": 999.99, "details": { "decimals": 2 } }
```

## Once placeholders

Use `:once` to generate a value once at startup and reuse it across all requests:

```json
{ "session": "{{session_id:once}}" }
```

Useful for correlation IDs or session tokens that should be consistent across a run.

## Environment variable injection

Use `{{ENV:VAR_NAME}}` to inject environment variables at startup — no definition needed:

```json
{ "apiKey": "{{ENV:API_KEY}}" }
```

!!! tip
    `{{ENV:VAR_NAME}}` values are resolved once at template load time and are never logged or included in output.

## Full placeholder reference

See [Template Placeholders](../reference/template-placeholders.md) for all types, fields, constraints, and validation rules.
