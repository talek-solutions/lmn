# Request Body Templates

Templates let you define JSON request bodies with dynamic placeholders. Each request in a run receives a freshly generated body, pre-computed before any requests fire.

---

## Usage

```
loadtest run -H <host> -M post -t path/to/template.json
```

`-t` / `--template` is mutually exclusive with `-B` / `--body`.

---

## File Structure

A template file is a standard JSON object with two parts:

```json
{
  "<field>": "{{<placeholder>}}",
  "_loadtest_metadata_templates": {
    "<placeholder>": { ... }
  }
}
```

| Part | Purpose |
|---|---|
| Body fields | The JSON shape sent in each request. Values that look like `{{name}}` are substituted at generation time. Non-placeholder values are copied as-is. |
| `_loadtest_metadata_templates` | Defines how each placeholder generates its value. Stripped from the request body automatically. |

---

## Placeholder Syntax

| Syntax | Behaviour |
|---|---|
| `"{{name}}"` | Generate a fresh value for each request |
| `"{{name:once}}"` | Generate once at startup — same value across all requests in the run |

Placeholders can appear anywhere a JSON string value can appear, including inside nested objects and arrays.

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

`:once` is supported inside `composition` values — the referenced placeholder's `:once` behaviour still applies.

---

## Full Example

```json
{
  "name": "{{username:once}}",
  "payment": "{{money}}",
  "_loadtest_metadata_templates": {
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

In this example `username` is fixed for the entire run (`:once`), while `payment.amount` and `payment.currency` vary per request.

---

## Validation

All of the following are checked at startup before any request fires:

- Template file exists and is valid JSON
- Every `{{placeholder}}` in the body has a definition in `_loadtest_metadata_templates`
- Every `composition` reference points to a defined placeholder
- No circular references between `object` compositions
- All numeric constraints are coherent (`min ≤ max`, `uppercase_count + lowercase_count ≤ min_length`, etc.)
- String lengths do not exceed the 10,000 character cap

---

## Extending the Template System

### Adding a new generator type

1. Add a variant to `RawTemplateDef` in `src/template/definition.rs` with its raw serde fields
2. Add a corresponding validated struct and variant to `TemplateDef`
3. Implement validation in the `validate()` function
4. Implement `Generate` for the new type in `src/template/generator.rs`
5. Add a match arm in `GeneratorContext::generate_def`

### Adding a new body format (e.g. XML, form-data)

The `BodyFormat` enum in `src/command/run.rs` is the extension point:

1. Add a new variant to `BodyFormat`
2. Add a corresponding CLI value to the format selector (when introduced)
3. Add the `Content-Type` mapping in the `match format` arm inside `run_concurrent_requests`
4. Add a `value_parser` for the new format in `src/cli/command.rs` (e.g. XML validation)
