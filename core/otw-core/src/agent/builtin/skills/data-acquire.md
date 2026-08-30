# Getting market data

## When to use

Before any backtest or analysis, when the dataset you need is missing, too short, or stale.
Not needed if the dataset already exists and covers the period you want — check first.

## Procedure

1. **See what you have.** `GET /api/histdata/datasets` lists downloaded datasets with their
   ticker, timeframe and bar count. If one covers your period, stop here.
2. **Check what the providers support.** `GET /api/connectors/providers` returns each provider's
   `asset_types` and `timeframes`, plus its rate limits and whether it needs a key
   (`required_secrets`). A provider/asset_type/timeframe combination that is not listed will
   be rejected — do not guess. Keyless crypto providers (Binance, Coinbase, Kraken) need no
   setup; equity/FX providers may need a key the user has to supply.
3. **Queue the download.** `POST /api/histdata/downloads` with `provider`, `ticker`,
   `asset_type`, `timeframe`, `from`, `to`. It returns `dataset_id` and `job_id` immediately —
   the download runs in the background.
4. **Wait for it.** `GET /api/histdata/jobs` until the job reads `done`. A job can fail;
   report the error rather than proceeding as though data arrived.
5. **Verify before using it.** Re-read the dataset and check the bar count and the actual date
   range against what you asked for. Providers routinely return less than requested.

## Pitfalls

- **`from`/`to` here must be RFC3339**, not a plain date: `2019-01-01T00:00:00Z`, not
  `2019-01-01`. A plain date returns `invalid from (need RFC3339)`. (The backtest `/run`
  endpoint is the lenient one and takes both — the two are not the same.)
- **Never silently substitute.** If the ticker, timeframe, or provider you wanted is
  unavailable, say so and ask. Quietly backtesting 4h data when the user said 1h, or ETHUSDT
  when they said ETH-USD, produces a confident answer to a question nobody asked.
- **Short history on fine timeframes.** Some providers cap how far back intraday data goes
  (Kraken returns ~720 bars for fine timeframes). Check the range you actually got: a "5-year"
  1m request may return a fortnight.
- **Symbol conventions differ per provider** — `BTCUSDT` on Binance, `BTC-USD` on Coinbase.
  Use the provider's own convention.
- **Rate limits are real.** Downloads retry with backoff; do not queue a dozen at once.
- **Stale rather than missing** is the quiet failure: a dataset that stops six months ago will
  backtest happily and tell you nothing about now. Check the last bar's date, and use
  `POST /api/histdata/datasets/{id}/append` to extend it.

## Verification

State the ticker, timeframe, bar count and the true first/last dates before you use a dataset
for anything. If any of them differ from what was asked, say so in the same breath.

## Report shape

"BTCUSDT 1d, 2557 bars, 2019-01-01 → 2025-12-31" — the identity of the data, always, so every
number that follows is anchored to a known span.
