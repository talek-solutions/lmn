# POST with inline JSON

Send a fixed JSON body directly from the command line — no files needed.

```bash
lmn run -H https://api.example.com/orders \
  -M post \
  -B '{"item":"widget","qty":1}'
```

`-M post` sets the HTTP method. `-B` sets the body — every request sends the same payload.

## With a content-type header

```bash
lmn run -H https://api.example.com/orders \
  -M post \
  -B '{"item":"widget","qty":1}' \
  --header "Content-Type: application/json"
```

## When to use this vs a template

Use `-B` when you want to send the **same body every request** — it is the fastest way to get started.

Use a [template file](post-template.md) when you need **unique data per request** (different user IDs, amounts, tokens).
