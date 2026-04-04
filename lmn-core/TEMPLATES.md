# Request Body Templates

Templates let you define JSON request bodies with dynamic placeholders. Each request in a run receives a freshly generated body, pre-computed before any requests fire.

---

## Usage

```
lmn run -H <host> -M post -T path/to/template.json
```

`-T` / `--request-template` is mutually exclusive with `-B` / `--body`.

---

## File Structure

A template file is a standard JSON object with two parts:

```json
{
  "<field>": "{{<placeholder>}}",
  "_lmn_metadata_templates": {
    "<placeholder>": { ... }
  }
}
```

| Part | Purpose |
|---|---|
| Body fields | The JSON shape sent in each request. Values that look like `{{name}}` are substituted at generation time. Non-placeholder values are copied as-is. |
| `_lmn_metadata_templates` | Defines how each placeholder generates its value. Stripped from the request body automatically. |

---

## Placeholder Syntax

| Syntax | Behaviour |
|---|---|
| `"{{name}}"` | Generate a fresh value for each request |
| `"{{name:global}}"` | Generate once at startup — same value across all requests in the run |

Placeholders can appear anywhere a JSON string value can appear, including inside nested objects and arrays.

---

## Built-in Placeholders

### `{{ENV:VAR_NAME}}`

Injects the value of an environment variable at template parse time.

```json
{
  "authorization": "{{ENV:API_TOKEN}}"
}
```

**Behaviour:**

- No `_lmn_metadata_templates` entry is required or allowed — `ENV:` placeholders are built-in.
- The env var is read once when the template is parsed, before any requests fire (fail-closed).
- If the named env var is not set, lmn exits immediately with an error.
- If the var name is empty (`{{ENV:}}`), lmn exits immediately with an error.
- `{{ENV:VAR_NAME:global}}` is **not supported**. The `:global` suffix is reserved for regular generator placeholders. Use `{{ENV:VAR_NAME}}` — it is already resolved once at startup and the same value is reused across all requests in the run.

---

## Supported Types

### `string`

Generates a string value. Two strategies are available: **choice** and **generated**.

**Choice** — pick randomly from a fixed list (takes priority over generation constraints):

```json
"currency": {
  "type": "string",
  "details": {
    "choice": ["EUR", "USD", "JPY", "CHF"]
  }
}
```

**Generated** — build a string from character constraints:

```json
"token": {
  "type": "string",
  "exact": 12,
  "details": {
    "uppercase_count": 4,
    "lowercase_count": 6,
    "special_chars": ["@", "_", "-"]
  }
}
```

| Field | Type | Description |
|---|---|---|
| `exact` | number | Exact string length |
| `min` | number | Minimum length (used when `exact` is absent) |
| `max` | number | Maximum length (used when `exact` is absent) |
| `details.choice` | string[] | If present, picks randomly from this list — all other constraints ignored |
| `details.uppercase_count` | number | Number of uppercase letters to include |
| `details.lowercase_count` | number | Number of lowercase letters to include |
| `details.special_chars` | string[] | Pool of special characters to fill remaining slots. If absent and slots remain, fills with alphanumeric characters |

**Constraints:** `uppercase_count + lowercase_count` must not exceed the minimum length. `min` must not exceed `max`. Max length is capped at 10,000.

Character positions are shuffled — no ordering is guaranteed within the generated string.

---

### `float`

Generates a floating-point number.

**Exact value:**
```json
"fee": {
  "type": "float",
  "exact": 1.50,
  "details": { "decimals": 2 }
}
```

**Random in range:**
```json
"amount": {
  "type": "float",
  "min": 5.0,
  "max": 100.0,
  "details": { "decimals": 2 }
}
```

| Field | Type | Description |
|---|---|---|
| `exact` | number | Always emit this value. If set, `min`/`max` are ignored |
| `min` | number | Lower bound (inclusive) |
| `max` | number | Upper bound (inclusive) |
| `details.decimals` | number | Decimal places to round to. Defaults to `2` |

**Constraints:** `min` must not exceed `max`. Both `min` and `max` are required when `exact` is absent.

---

### `object`

Composes other placeholders into a nested JSON object. Each field in `composition` maps an output key to a placeholder reference.

```json
"money": {
  "type": "object",
  "composition": {
    "amount": "{{amount}}",
    "currency": "{{currency}}"
  }
}
```

| Field | Type | Description |
|---|---|---|
| `composition` | `{ field: "{{name}}" }` | Maps output field names to placeholder references |

**Constraints:** All referenced placeholders must be defined. Circular references are detected and rejected at startup.

`:global` is supported inside `composition` values — the referenced placeholder's `:global` behaviour still applies.

---

## Full Example

```json
{
  "name": "{{username:global}}",
  "payment": "{{money}}",
  "_lmn_metadata_templates": {
    "username": {
      "type": "string",
      "details": {
        "choice": ["alice", "bob", "carol", "dave"]
      }
    },
    "amount": {
      "type": "float",
      "min": 1.0,
      "max": 500.0,
      "details": { "decimals": 2 }
    },
    "currency": {
      "type": "string",
      "details": {
        "choice": ["EUR", "USD", "GBP"]
      }
    },
    "money": {
      "type": "object",
      "composition": {
        "amount": "{{amount}}",
        "currency": "{{currency}}"
      }
    }
  }
}
```

In this example `username` is fixed for the entire run (`:global`), while `payment.amount` and `payment.currency` vary per request.

---

## Validation

All of the following are checked at startup before any request fires:

- Template file exists and is valid JSON
- Every `{{placeholder}}` in the body has a definition in `_lmn_metadata_templates`
- Every `composition` reference points to a defined placeholder
- No circular references between `object` compositions
- All numeric constraints are coherent (`min ≤ max`, `uppercase_count + lowercase_count ≤ min_length`, etc.)
- String lengths do not exceed the 10,000 character cap

---

---

# Response Templates

Response templates let you track specific fields from response bodies. You define a JSON shape that mirrors the fields you care about — everything else in the response is ignored. Extracted values are aggregated and displayed in the run statistics.

---

## Usage

```
lmn run -H <host> -M post -T path/to/template.json -S path/to/response-template.json
```

`-S` / `--response-template` is optional and independent of `-T` / `--request-template`.

---

## File Structure

A response template is a JSON object that mirrors the expected response shape. Leaf values are `{{TYPE}}` placeholders indicating which fields to extract and how to aggregate them.

```json
{
  "<field>": "{{TYPE}}",
  "<nested>": {
    "<field>": "{{TYPE}}"
  }
}
```

The template acts as a loose schema — the response may contain additional fields, but any field referenced in the template that is **missing** from a response is tracked as a mismatch.

---

## Supported Extraction Types

### `{{STRING}}`

Extracts a string value and tracks the frequency distribution of distinct values.

**Stats output:** count per unique value.

### `{{FLOAT}}`

Extracts a numeric value and tracks aggregate statistics.

**Stats output:** min, max, avg, percentiles.

---

## Nested Paths

The template structure mirrors the response JSON. To track a deeply nested field, nest the template accordingly:

```json
{
  "error": {
    "code": "{{STRING}}"
  }
}
```

Given a response `{"error": {"code": "NOT_FOUND", "message": "..."}, "request_id": "..."}`, this extracts `error.code` as `"NOT_FOUND"` and ignores `error.message` and `request_id`.

---

## Mismatches

When a response does not contain a field defined in the template, it is **not** treated as an error — the request still counts as normal. Instead, mismatches are tracked separately and reported in the statistics, so you can see how many responses did not conform to the expected shape.

A mismatch occurs when:
- A templated field path does not exist in the response
- A templated field has a value type that does not match the extraction type (e.g. `{{FLOAT}}` on a string value)

---

## Full Example

**Response template:**
```json
{
  "error": {
    "code": "{{STRING}}"
  }
}
```

**CLI:**
```
lmn run -H https://api.example.com/pay -M post -T request.json -S response.json
```

After the run, the statistics section will include:
- Distribution of `error.code` values across all responses
- Count of responses where `error.code` was missing or had an unexpected type

---

## Extending the Template System

### Adding a new generator type

1. Add a variant to `RawTemplateDef` in `lmn-core/src/request_template/definition.rs` with its raw serde fields
2. Add a corresponding validated struct and variant to `TemplateDef`
3. Implement validation in the `validate()` function
4. Implement `Generate` for the new type in `lmn-core/src/request_template/generator.rs`
5. Add a match arm in `GeneratorContext::generate_def`

### Adding a new body format (e.g. XML, form-data)

The `BodyFormat` enum in `lmn-core/src/command/run.rs` is the extension point:

1. Add a new variant to `BodyFormat`
2. Add a corresponding CLI value to the format selector (when introduced)
3. Add the `Content-Type` mapping in the `match format` arm inside `run_concurrent_requests`
4. Add a `value_parser` for the new format in `lmn-cli/src/cli/command.rs` (e.g. XML validation)
