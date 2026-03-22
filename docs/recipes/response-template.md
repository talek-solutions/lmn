# Track error codes in responses

Use a response template to extract and aggregate fields from response bodies during a load test. The primary use case: find out what error codes your API is returning under load and how often.

## 1. Create a response template

Save this as `response.json`. It mirrors the shape of your API's error response:

```json
{
  "error": {
    "code": "{{STRING}}"
  }
}
```

`{{STRING}}` tells lmn to extract that field as a string and track the frequency distribution of all distinct values across every response.

## 2. Run with both templates

```bash
lmn run -H https://api.example.com/orders \
  -M post \
  -T ./template.json \
  -S ./response.json
```

`-T` is the request template (what to send). `-S` is the response template (what to track).

## 3. Read the output

After the run, lmn prints a breakdown under **Response**:

```
 Response: error.code ──────────────────────────────────────────────────────────────
  OK                    8741  ████████████████████████████
  INSUFFICIENT_BALANCE   987  ███▌
  NOT_FOUND              201  ▊
  RATE_LIMITED            71  ▎
```

You can immediately see that 10% of requests are hitting `INSUFFICIENT_BALANCE` — something that would be invisible from status codes alone if the API returns `200` for business errors.

## 4. Capture in JSON output

```bash
lmn run -f lmn.yaml -S ./response.json --output-file result.json
```

The `response_stats` field in the JSON report contains the full distribution. See [JSON Output Schema](../reference/json-output.md#response_stats).

## Config file equivalent

```yaml
run:
  host: https://api.example.com/orders
  method: post
  template_path: ./template.json
  response_template_path: ./response.json

execution:
  request_count: 1000
  concurrency: 25
```

See [Template Placeholders](../reference/template-placeholders.md) for `{{FLOAT}}` extraction and nested field tracking.
