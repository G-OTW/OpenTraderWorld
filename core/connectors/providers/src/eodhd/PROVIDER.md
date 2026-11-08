# EODHD

Adapter version: 0.1.0        Provider API version: `/api`        Maturity: M2

Sources:
- End-of-day: https://eodhd.com/financial-apis/api-for-historical-data-and-volumes
- Intraday: https://eodhd.com/financial-apis/intraday-historical-data-api
- Search: https://eodhd.com/financial-apis/search-api-for-stocks-etfs-mutual-funds-and-indices
- Limits: https://eodhd.com/financial-apis/api-limits

M2: mock catalog green; parity with the legacy connector on mock (bars, search, refusals).
Real shadow ready with a key (`OTW_EODHD_API_KEY`).

## Endpoints

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `eod` | `GET /api/eod/{CODE.EXCH}?from&to&period=d\|w&fmt=json` | `RequestHistory{Candles}` 1d, 1w | 1 | none: the window in one answer |
| `intraday` | `GET /api/intraday/{CODE.EXCH}?interval&from&to&fmt=json` | `RequestHistory{Candles}` 1m, 5m, 1h | 5 calls on EODHD's meter | none |
| `search` | `GET /api/search/{query}?fmt=json&limit=30` | `SearchInstruments` | 1 | none |

## Auth

`api_token` query parameter, added by the signer (`ApiKeyQuerySigner`). Secret: `api_key`.

## Rate limits

A daily call allowance by plan (intraday costs more per request); 429 when spent. OTW's
worker paces at 250 ms. No bucket declared (the allowance is daily).

## Symbols

Native id = `CODE.EXCHANGE` (`AAPL.US`, `BMW.XETRA`, `EURUSD.FOREX`), as the bar endpoints
take it. Search answers `Code` and `Exchange` separately; they are joined (`US` when the
exchange is empty). Discovery is search only.

## Enum tables

| Interval | Endpoint / token |
| --- | --- |
| 1m, 5m, 1h | intraday `1m 5m 1h` |
| 1d, 1w | eod `period=d`, `period=w` |

| Search `Type` | Market |
| --- | --- |
| `ETF`, `FUND`, `Mutual Fund` | Etf |
| `Currency` / `FX`, `Crypto...`, `Index`, `Option`, `Future` | Forex, Spot, Index, Option, Future |
| anything else (`Common Stock`) | Equity |

| Status | Canonical |
| --- | --- |
| 401 | Auth |
| 402 (plan) | Permission |
| 404 (`Ticker Not Found.`) | NotFound ("bad symbol") |
| 429 | RateLimited |
| 5xx | Transient |

## Units and time

- Prices and volumes as JSON numbers, read from their text.
- End-of-day `date` (`2024-01-02`) is the day at UTC midnight; `adjusted_close` goes to
  `Candle.adjusted.close`.
- Intraday `timestamp` in Unix seconds (UTC).
- No forming flag (`history.candles.final_flag = false`): the engine finalizes.

## Quirks

- Both window ends are inclusive on both endpoints, and the answer for a window ending now
  includes the forming bar: the adapter keeps `[from, to)`.
- 15m is not served (OTW never offered it here).

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` with closed bars only.
- A plan refusal (402) reads as a permission refusal.
- Intraday calls now go through the API-rate dashboard (legacy sent them around it).

## Regions and terms

No regional restriction published; terms cap the daily call volume per plan.
