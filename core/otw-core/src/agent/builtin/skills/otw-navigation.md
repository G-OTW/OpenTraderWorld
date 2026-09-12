# Navigating the OpenTraderWorld API

## When to use

Before calling an endpoint you have not already used successfully in this conversation, and
whenever a call comes back with an error. Not needed for a path you just used and that worked.

## Preconditions

You have `otw_catalog` / `otw_read` (and `otw_write` if you were granted write anywhere). If you
do not, you have no data access this run — say so instead of guessing at URLs.

## Procedure

1. **Catalog first.** `otw_catalog` with no argument lists the modules you can reach. Pass
   `module` (e.g. `"journal"`) for that module's concrete endpoints, their methods, and their
   request bodies. The catalog is the authority: if an endpoint is not in it, you cannot reach
   it, no matter what you remember about the platform.
2. **Read the whole entry before calling.** Descriptions carry the traps — units, required
   query parameters, response-size warnings. They are there because that call is easy to get
   wrong.
3. **Call with a concrete path.** `otw_read` takes the path plus its query string, e.g.
   `/api/journal/trades?limit=20`. Path parameters get real ids, never placeholders.
4. **Trim big responses.** `pick` takes dot-paths and returns only those fields
   (`["result.cvar"]`); `head` truncates every array to its first N elements and reports the
   totals. Use them whenever the user asked for a few figures — pulling a full response and
   summarising it wastes the context you will need later in the task.
5. **Write only after confirming the target.** Check the endpoint's body shape in the catalog,
   state what you are about to change, then call `otw_write`.

## Pitfalls

- **Guessing a path.** A plausible-looking URL that does not exist returns an error, and a
  plausible-looking URL that exists but does something else returns a wrong answer confidently.
  Neither is recoverable by trying harder — go back to the catalog.
- **Guessing a field name** in a request body. Same failure, quieter: unknown fields may be
  ignored rather than rejected, so the call "succeeds" with your intent dropped.
- **Pulling everything.** Endpoints that return per-bar or per-trade arrays will fill your
  context. Look for a `view=summary` option or use `pick`/`head`.
- **Retrying an identical failed call.** Read the error first: it usually names the missing
  parameter or the bad value. A second identical call produces a second identical error.
- **Assuming absence means broken.** If a module is not in your catalog, it was not granted to
  you. That is a permission boundary, not a bug — tell the user which module you would need.

## Verification

After a write, read the affected resource back and report what it now says. "Created
successfully" without a read-back is a claim about a response code, not about the data.

## Report shape

State what you fetched and from where, in plain language ("from your 47 journal trades since
March"). The user should be able to tell which numbers are measured and which are derived.
