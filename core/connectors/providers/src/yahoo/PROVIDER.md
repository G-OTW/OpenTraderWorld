# Yahoo Finance

Adapter version: 0.1.0        Provider API version: chart v8, search v1 (unofficial)        Maturity: M2

Sources:
- No published reference. The chart and search endpoints are the ones Yahoo's own pages
  call; every behaviour below was observed against the live endpoints (2026-10-17) and is
  pinned by a test.
- Chart: `https://query1.finance.yahoo.com/v8/finance/chart/AAPL`

M2: mock catalog green; shadow against the live endpoints (38 701 bars over 1m, 1h, 1d, 1w
on an equity, an index, a crypto pair and a currency pair, plus search) identical to the
legacy connector.

## Endpoints

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `chart` | `GET /v8/finance/chart/{symbol}?interval&period1&period2` | `RequestHistory{Candles}` | 1 | none: the whole window in one answer |
| `search` | `GET /v1/finance/search?q&quotesCount=25&newsCount=0` | `SearchInstruments` | 1 | none |

## Auth

None. A user agent is required (a request without one is answered 429); OTW's HTTP client
sends `OpenTraderWorld/histdata`.

## Rate limits

None published. Throttling answers a bare HTTP 429 (`Edge: Too Many Requests`) or, inside
the JSON, an error whose text says "Too Many Requests": both `RateLimited`. OTW's worker
paces downloads at 600 ms.

## Symbols

Native id = Yahoo's own symbol, kept as typed (`AAPL`, `^GSPC`, `BTC-USD`, `EURUSD=X`,
`BRK-B`). It lands in a URL path: only letters, digits and `^ . - = _` pass. Discovery is
search only (`instrument_discovery = "search_only"`): each query is answered in Yahoo's
order, filtered by the market asked for.

## Enum tables

| `quoteType` | Market |
| --- | --- |
| `ETF`, `ETP`, `FUND`, `MUTUALFUND` | Etf |
| `CRYPTOCURRENCY`, `CRYPTO`, `DIGITAL CURRENCY` | Spot (crypto) |
| `CURRENCY`, `FX`, `FOREX` | Forex |
| `INDEX`, `INDICES` | Index |
| `OPTION`, `OPTIONS` | Option |
| `FUTURE`, `FUTURES` | Future |
| anything else (`EQUITY`) | Equity |

| Interval | Token | Depth Yahoo keeps |
| --- | --- | --- |
| 1m | `1m` | 30 days |
| 5m, 15m | `5m`, `15m` | 60 days |
| 1h | `1h` | 730 days |
| 1d | `1d` | full |
| 1w | `1wk` | full |

Errors (in the body whatever the status):

| `chart.error` | Canonical |
| --- | --- |
| code `Not Found` | NotFound ("bad symbol") |
| text containing "Too Many Requests"; bare 429 | RateLimited |
| anything else (`Unprocessable Entity`: range past the depth kept) | InvalidRequest, Yahoo's sentence kept |
| no result and no error | NotFound |

## Units and time

- Columns `open, high, low, close, volume` and `adjclose`, JSON numbers read from their text;
  a null OHLC is a gap (skipped), a null volume is 0.
- `timestamp` in seconds: the period's start as Yahoo stamps it (an equity day at its
  session open, 14:30 UTC for New York in winter).
- The adjusted close goes to `Candle.adjusted.close`.
- No forming flag (`history.candles.final_flag = false`): every bar goes out forming and the
  engine finalizes a bar once its bucket is over and a full period has passed since its stamp.

## Quirks

- `period2` is inclusive, and the answer for a window ending now includes the forming bar.
- A window starting earlier than the depth kept for an intraday interval is refused, not
  clamped: the adapter pulls the start in to one day under the depth.
- The refusal body comes with a 404 or a 422, not a 200.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` with closed bars only (legacy also returned the bar at
  `period2` and the forming one, which the worker dropped).
- Prices are read from their text and rounded correctly; legacy's JSON float parsing could
  land one unit in the last place off on a long decimal.
- A bare 429 is a rate limit the worker waits out (legacy could not read its body and failed
  the download).

## Regions and terms

Unofficial endpoint; Yahoo's terms do not grant automated access. Best effort, as before.
