# External API fixtures

Responses of market-data, news, FX and broker APIs, as JSON, for tests that mock the HTTP
layer (`wiremock`) instead of calling the provider.

Rules on top of the root README:

- One folder per provider (`api/<provider>/`), one file per response shape, named after the
  case it covers (`quote_ok.json`, `bars_partial.json`, `rate_limited.json`).
- Recorded responses are scrubbed before they land here: no account id, order id, token,
  signature or e-mail; ids become placeholders and the scrub is described in the folder's
  README with the capture date.
- Hand-written responses say which documentation page they follow.
- Error cases (4xx, 429 with `Retry-After`, 5xx, malformed JSON, missing or `null` fields,
  empty body) are files too, so a test names the case it replays.

Existing provider fixtures that predate this folder (for example `core/otw-core/tests/golden/`
and the connector testkit) stay where they are.
