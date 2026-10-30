# Navigating the OpenTraderWorld API

## When to use

Before calling an endpoint you have not already used successfully in this conversation, and
whenever a call comes back with an error. Not needed for a path you just used and that worked.

## Preconditions

You have `otw_catalog` and `otw_read` (and `otw_write` if you were granted write anywhere). If
you do not, you have no data access this run: say so instead of guessing at URLs.

## Procedure

1. **Catalog first.** `otw_catalog` with no argument lists the modules you can reach. `module`
   (e.g. `"journal"`) lists that module's endpoints. `endpoint` (e.g. `"POST /api/backtest/run"`)
   returns the compact entry: description, a working example, and the body field table.
   `endpoints: [...]` returns several entries in one call. Add `full: true` with one endpoint
   only, and only when the field table did not answer: it is the raw JSON Schema, many times
   larger. If an endpoint is not in the catalog, you cannot reach it.
2. **Read the whole entry.** Descriptions carry the traps: units, required query parameters,
   response size. Enum values: `GET /api/vocab` lists every closed vocabulary,
   `GET /api/vocab/{field}` one field. Backtest settings: `GET /api/backtest/manifest`.
3. **Call with a concrete path.** `otw_read` takes the path plus its query string, e.g.
   `/api/journal/trades?ticker=AAPL`. Path parameters get real ids, never placeholders. Query
   parameters are not body fields: `view` on `POST /api/backtest/run` goes in the body.
4. **Check before you run or write.** `otw_write` with `validate_only: true` checks the body
   and answers ok or every problem, without running or storing anything. `POST
   /api/backtest/run` also takes `dry_run: true`: its checks, the effective settings, warm-up
   and bar count, no simulation.
5. **Trim big responses.** `pick` takes dot-paths (`["result.var_hist"]`); when the body is a
   single wrapper key (`{"strategy":{...}}`), a pick that misses at the root is read under it.
   `head` caps every array at N elements and reports totals.
6. **Write only after confirming the target.** State what you are about to change, then call
   `otw_write`.

## Reading a result

- **The first lines are the ones that matter.** `server warnings: ...` repeats the response's
  `warnings`; `warning: the server ignored ...` names body fields it did not use (a typo, a
  field from another endpoint). Either means the result is not what you asked for: fix the call
  and rerun before reporting. Some bodies refuse unknown fields outright with a 400.
- **Read the error.** It usually names the field, the valid values, or the closest real path.
  Retrying the identical call gets the identical error.
- **Absence is a permission boundary.** A module missing from your catalog was not granted.
  Tell the user which module you would need.

## Verification

After a write, read the affected resource back and report what it now says. "Created
successfully" without a read-back is a claim about a response code, not about the data.

## Report shape

Say what you fetched and from where, in plain language ("from your 47 journal trades since
March"). The user should be able to tell which numbers are measured and which are derived.
