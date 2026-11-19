# Alpha Vantage

Adapter version: 0.1.0        Provider API version: `/query` functions        Maturity: M2

Sources:
- Reference: https://www.alphavantage.co/documentation/
- Daily series: https://www.alphavantage.co/documentation/#daily
- Intraday series: https://www.alphavantage.co/documentation/#intraday
- Symbol search: https://www.alphavantage.co/documentation/#symbolsearch
- Premium and limits: https://www.alphavantage.co/premium/

M2: mock catalog green; parity with the legacy connector on mock (bars, search, refusals).
Real shadow ready with a key (`OTW_ALPHAVANTAGE_API_KEY`).

## Endpoints

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `series` | `GET /query?function=TIME_SERIES_DAILY&symbol&outputsize=compact` | `RequestHistory{Candles}` 1d | 1 | none: the last 100 days |
| `series` | `GET /query?function=TIME_SERIES_INTRADAY&symbol&interval&outputsize=full` | `RequestHistory{Candles}` intraday | 1 | none: the recent month |
| `search` | `GET /query?function=SYMBOL_SEARCH&keywords` | `SearchInstruments` | 1 | none |

## Auth

`apikey` query parameter, added by the signer (`ApiKeyQuerySigner`); the mapper never
writes it. Secret: `api_key`. A missing key is refused before any call.

## Rate limits

Free key: 25 calls a day, 5 a minute (declared as an account bucket of 5 per minute; OTW's
worker paces at 12 s). Over the allowance the answer is HTTP 200 with a `Note` or
`Information` text, not a 429.

## Symbols

Native id = Alpha Vantage's own symbol, kept as typed (`IBM`, `TSCO.LON`, `SHOP.TRT`). Discovery
is search only.

## Enum tables

| Interval | Function / token |
| --- | --- |
| 1m, 5m, 15m, 1h | `TIME_SERIES_INTRADAY`, `1min 5min 15min 60min` |
| 1d | `TIME_SERIES_DAILY` |

| Search `3. type` | Market |
| --- | --- |
| `ETF`, `Mutual Fund`, `Fund` | Etf |
| `Crypto...`, `Currency` / `FX`, `Index`, `Option`, `Future` | Spot, Forex, Index, Option, Future |
| anything else (`Equity`) | Equity |

| Body | Canonical |
| --- | --- |
| `Note` / `Information` naming a rate limit, "requests per", "call frequency" | RateLimited |
| `Note` / `Information` naming a premium endpoint | Permission |
| other `Note` / `Information` | InvalidRequest |
| `Error Message` | NotFound ("bad symbol") |
| no `Time Series` object | NotFound |

## Units and time

- Prices and volumes as decimal strings; volume from `6. volume`, else `5. volume`.
- `5. adjusted close`, when present, goes to `Candle.adjusted.close` (the free endpoints
  carry none).
- No forming flag (`history.candles.final_flag = false`): the engine finalizes.

## Quirks

- The series ignores the window: one call answers the whole compact or recent series; the
  adapter keeps the window `[from, to)`.
- Intraday stamps (`2024-01-02 19:55:00`) carry no offset; the metadata names the zone
  (`6. Time Zone: US/Eastern`). They are read in that zone and stored as the UTC instant they
  name; a series without the zone is refused. Daily rows are dates, at UTC midnight. Rows
  stored before this read New York time as UTC; migration 0149 moved them.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers the window `[from, to)` with closed bars only (legacy handed back the whole
  series; the worker dropped the forming bar itself).
- A premium-only answer reads as a permission refusal rather than a generic error.

## Regions and terms

No regional restriction published. The free key's terms cap the call volume.
