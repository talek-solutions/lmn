# POST with a template file

Generate unique data per request from a JSON template. Useful for testing APIs that require varied payloads — different user IDs, amounts, session tokens.

Save your template file anywhere in your project — pass the path with `-T`. A common convention is to keep templates in a `templates/` directory at the project root.

## 1. Create a template file

Save this as `templates/order.json`:

{% raw %}
```json
{
  "userId": "{{user_id}}",
  "amount": "{{amount}}",
  "_loadtest_metadata_templates": {
    "user_id": {
      "type": "string",
      "details": {
        "choice": ["user-001", "user-002", "user-003", "user-004", "user-005"]
      }
    },
    "amount": {
      "type": "float",
      "min": 1,
      "max": 500,
      "details": {
        "decimals": 2
      }
    }
  }
}
```
{% endraw %}

Each request gets a freshly generated `userId` and `amount`. The `_loadtest_metadata_templates` block is stripped before sending.

## 2. Run with the template

```bash
lmn run -H https://api.example.com/orders \
  -M post \
  -T ./templates/order.json
```

## Placeholder types

| Type | What it generates |
|---|---|
| `string` with `choice` | Randomly picks from the list |
| `string` with `exact` | Fixed-length alphanumeric string |
| `string` with `min`/`max` | Random-length alphanumeric string |
| `float` with `min`/`max` | Random float in range |
| `float` with `exact` | Always emits the same float value |
| `object` with `composition` | Nested object composed from other placeholders |

## Static values (same across all requests)

{% raw %}
Add `:global` to generate a value once and reuse it across all requests:

```json
{
  "sessionId": "{{session_id:global}}",
  "item": "{{item_name}}"
}
```
{% endraw %}

## Inject environment variables

{% raw %}
Use `{{ENV:VAR_NAME}}` to pull a value from the environment — no template definition needed:

```json
{
  "apiKey": "{{ENV:API_KEY}}",
  "userId": "{{user_id}}"
}
```
{% endraw %}

If the variable is not set, `lmn` exits immediately with an error before any requests fire.

See [Template Placeholders](../reference/template-placeholders.md) for the full reference.
