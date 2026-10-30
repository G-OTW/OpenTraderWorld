# Getting market data

## When to use

Before any backtest or analysis, when the dataset you need is missing, too short, or stale.
Not needed if the dataset already exists and covers the period you want: check first.

## Procedure

1. **Resolve it in one call.** `POST /api/histdata/resolve` with `ticker`, `asset_type`,
   `timeframe` (exact, lowercase) and `from`/`to` (a plain `YYYY-MM-DD` works here). It returns
   `stored` (datasets of that ticker at that timeframe, each with `coverage_pct` of your
   window), `connectors` granted to histdata (`ready`, `missing_secrets`, `unsupported`,
   `history_limit_days`, `requests`, `quota`), `alternatives` (the same ticker stored at other
   timeframes) and a one-line `recommendation`. If a stored dataset covers the window, stop
   here. Otherwise download through a `ready` connector (step 4). An alternative timeframe is
   an option to offer the user, never a silent substitute.
2. **Pick a ready connector.** A connector can serve the request when its provider lists the
   `asset_type` and the `timeframe` and every one of its `required_secrets` is set: resolve
   reports that as `ready`. Pick a connector with `ready: true` from resolve; one with
   `unsupported` or `missing_secrets` will be refused. Only when resolve left a question, `GET /api/connectors/providers`
   lists each provider's `asset_types`, `timeframes`, rate limits and `required_secrets`. A
   combination that is not listed is rejected, so do not guess. Keyless: Binance, Coinbase and
   Kraken for crypto, Yahoo and IBKR for equities (Yahoo 1h covers the last 730 days). A
   provider whose key is missing is not a reason to stop while another ready connector fits.
3. **Check the symbol** when you are not certain of its provider spelling:
   `GET /api/histdata/symbols?q=&asset_type=` searches every connector granted to the chart and
   returns each hit's provider-native symbol. `POST /api/histdata/preview` fetches a trailing
   window without storing anything, which also answers "does this ticker exist here" for a
   provider that has no search endpoint.
4. **Queue the download.** `POST /api/histdata/downloads` with `connector_id` (preferred: the `id` of a
   `ready` connector from resolve) or `provider`, plus `asset_type`, `ticker`, `timeframe`, `from`, `to`.
   It returns `dataset_id` and `job_id` immediately; the download runs in the background.
5. **Wait for it.** `GET /api/histdata/jobs` (the 50 most recent) until the job reads `done`.
   Live statuses are `queued`, `running`, `waiting` (parked: `wait_reason` is `quota` or
   `rate_limit`, `resume_at` says when it restarts) and `cancelling`; terminal ones are `done`,
   `error` and `cancelled`. Progress is `chunks_done`/`chunks_total` and `bars_written`. A
   parked job is not a failed job: say it is waiting, and say until when. `DELETE
   /api/histdata/jobs/{id}` stops one (bars already written are kept).
6. **Verify before using it.** Re-read the dataset and check the bar count and the actual date
   range against what you asked for. Providers routinely return less than requested.

## When nothing fits the full request

Download through any ready connector. If none fits the full request, run on the closest data
available (a shorter window, or a stored higher timeframe), state what differs and offer the
alternative. Ask the user only for a choice with no viable default, or for credentials when no
connector can serve.

## One request, many downloads

The same endpoint queues a batch: `tickers` (extra symbols) and `timeframes` (extra timeframes)
alongside the base pair, every combination on the same connector and the same window, up to 100
jobs per submission. The response then carries `batch_id` and a `jobs` array (one `ticker` /
`timeframe` / `job_id` / `dataset_id` per pair); the top-level `job_id` / `dataset_id` stay the
first pair, so a single download reads exactly as before. `DELETE
/api/histdata/jobs/batch/{id}` cancels whatever is left of it.

An unsupported timeframe inside a batch is **skipped, not fatal**: it comes back in `errors`
while the rest is queued. Read `errors`: a batch that "succeeded" may have queued half of what
you asked for.

**Cost it before you queue it.** Every response (and `estimate_only: true`, which queues
nothing) carries `estimate`: `jobs`, `requests` (provider round trips), `seconds` (the worker's
own pacing floor), the connector's quota use, and `over_quota` when the batch cannot fit in
what is left of the window. `over_quota` does not refuse the batch: the worker downloads what
it can, then parks until the quota rolls over. Say that before starting a run that waits on it.

## When a run refuses the data

A fresh download from an independent provider helps only when the raw bars are wrong (a price
that is not a number, a high below the low, a gap the check reports). An engine refusal on a
bar whose raw values are consistent is not a data problem.

## Pitfalls

- **`from`/`to` here must be RFC3339**, not a plain date: `2019-01-01T00:00:00Z`, not
  `2019-01-01`. A plain date returns `invalid from (need RFC3339)`. (The backtest `/run`
  endpoint is the lenient one and takes both: the two are not the same.)
- **Never silently substitute.** Running on the closest data is fine only when you say what
  differs, next to the numbers. Quietly backtesting 4h data when the user said 1h, or ETHUSDT
  when they said ETH-USD, produces a confident answer to a question nobody asked.
- **Short history on fine timeframes.** Some providers cap how far back intraday data goes
  (Kraken returns ~720 bars for fine timeframes). Check the range you actually got: a "5-year"
  1m request may return a fortnight.
- **Symbol conventions differ per provider**: `BTCUSDT` on Binance, `BTC-USD` on Coinbase.
  Use the provider's own convention.
- **Rate limits are real.** A batch is paced, parked and resumed by the worker, not retried
  blindly, but it still spends the connector's quota: read `estimate` first and prefer one
  batch over a dozen separate submissions.
- **Stale rather than missing** is the quiet failure: a dataset that stops six months ago will
  backtest happily and tell you nothing about now. Check the last bar's date, and use
  `POST /api/histdata/datasets/{id}/append` to extend it.

## Verification

State the ticker, timeframe, bar count and the true first/last dates before you use a dataset
for anything. If any of them differ from what was asked, say so in the same breath.

## Report shape

"BTCUSDT 1d, 2557 bars, 2019-01-01 → 2025-12-31": the identity of the data, always, so every
number that follows is anchored to a known span.
