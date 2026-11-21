# Massive (formerly Polygon.io)

Adapter version: 0.1.0        Provider API version: v2 aggregates, v3 quotes and reference, futures v1, socket        Maturity: M2

Sources:
- Aggregates: https://massive.com/docs/rest/stocks/aggregates/custom-bars
- Futures aggregates: https://massive.com/docs/rest/futures/aggregates
- Quotes: https://massive.com/docs/rest/stocks/trades-quotes/quotes
- Tickers: https://massive.com/docs/rest/stocks/tickers/all-tickers
- Socket: https://massive.com/docs/websocket/quickstart, https://massive.com/docs/websocket/stocks/aggregates-per-minute
- Plans and limits: https://massive.com/pricing

M2: mock catalog green; parity with the legacy connector on mock (bars for every market,
quotes, search, refusals, live handshake and aggregates). Real shadow ready with a key
(`OTW_MASSIVE_API_KEY`).

## Endpoints

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `aggs` | `GET /v2/aggs/ticker/{ticker}/range/{mult}/{span}/{from_ms}/{to_ms}?adjusted=false&sort=asc&limit=50000` | `RequestHistory{Candles}` every market but futures | 1 | none: up to 50000 bars, the planner sizes the window |
| `futuresAggs` | `GET /futures/v1/aggs/{ticker}?resolution&window_start.gte&window_start.lt&sort=window_start.asc&limit=50000` | `RequestHistory{Candles}` futures | 1 | none |
| `quote` | `GET /v3/quotes/{ticker}?timestamp.lt\|gte={ns}&order=desc\|asc&sort=timestamp&limit=1` | `RequestQuoteAt`, `RequestSnapshot{Quote}` (last before now) | 1 | none |
| `search` | `GET /v3/reference/tickers?search&market&active=true&limit=50` | `SearchInstruments` | 1 | none |
| socket | `wss://socket.massive.com/{cluster}` | `Subscribe{Candles 1m}` | | |

## Auth

REST: `apiKey` query parameter, added by the signer. Socket: `{"action":"auth","params":key}`
sent by the signer (`WsLogin`) once the server greets with `status: connected`. Secret:
`api_key`. The key never reaches an error message (the transport strips URLs).

## Rate limits

Free key: 5 requests a minute; paid plans are unlimited. No bucket declared (the limit is the
plan's); OTW's job worker paces at 12 s as before. Over the limit
answers 429, or an `ERROR` envelope naming the limit under a 200: both are rate-limited.

One socket per market cluster per key.

## Symbols

Native id = the REST ticker, the market riding the id's venue:

| Market | Venue | Native | Example |
| --- | --- | --- | --- |
| Stocks, ETFs | `MASSIVE` | bare | `AAPL` |
| Options | `MASSIVE-OPTIONS` | `O:` + OCC | `O:SPY251219C00650000` |
| Crypto | `MASSIVE-CRYPTO` | `X:` + glued pair | `X:BTCUSD` |
| Forex | `MASSIVE-FX` | `C:` + glued pair | `C:EURUSD` |
| Indices | `MASSIVE-INDICES` | `I:` | `I:SPX` |
| Futures | `MASSIVE-FUTURES` | contract with its month | `ESZ5` |

The prefix is added when missing; with no market named, the prefix says it. Discovery is
search only, narrowed to one market (`stocks` when none is named).

Socket channels differ from the tickers: `AM.<ticker>` on stocks, options, indices and
futures; `XA.BTC-USD` on crypto and `CA.EUR/USD` on forex.

## Enum tables

| Interval | v2 `mult/span` | futures `resolution` |
| --- | --- | --- |
| 1m, 5m, 15m | `1/minute`, `5/minute`, `15/minute` | `1min`, `5min`, `15min` |
| 1h | `1/hour` | `1hour` |
| 1d | `1/day` | `1session` |
| 1w | `1/week` | `1week` |

| REST status | Canonical |
| --- | --- |
| `OK`, `DELAYED` | served |
| `NOT_AUTHORIZED` | Permission |
| `NOT_FOUND` | NotFound ("bad symbol") |
| `ERROR` naming a limit, or HTTP 429 | RateLimited |
| other `ERROR` | InvalidRequest |
| 5xx | Transient |

| Socket status | Canonical |
| --- | --- |
| `auth_failed` (free key) | Permission, naming the paid plan |
| `auth_timeout` | Auth |
| `max_connections`, error naming the connection cap | RateLimited |
| error: not authorized, insufficient, upgrade | Permission |
| error: unknown or invalid ticker | NotFound |
| other error | Transient |

## Units and time

- Prices and volumes as JSON numbers, read from their text.
- v2 bar `t` in Unix milliseconds (a stock's day at New York midnight); futures
  `window_start` in nanoseconds (a session in Central Time); quote `sip_timestamp` in
  nanoseconds; socket aggregate `s` in milliseconds.
- `n` (trade count) goes to `Candle.trades`.
- No forming flag (`history.candles.final_flag = false`): the engine finalizes. A socket
  minute aggregate is published once its minute ends, so it is final.

## Quirks

- The v2 `to` bound is inclusive: the adapter asks one millisecond less and keeps `[from, to)`.
- The index endpoint takes no `adjusted` parameter and its bars carry no volume.
- Futures live on their own service with their own parameters and units.
- A crypto pair is split for the socket at the longest known quote currency
  (`USDT USDC TUSD BUSD USD EUR GBP JPY BTC ETH`); a pair already written with a separator is
  kept.
- Only past quotes of stocks, ETFs, options and forex are kept; the latest quote is read for
  every market.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` with closed bars only.
- An unknown ticker reads as a symbol fault; an over-limit `ERROR` envelope is retried.
- A refused socket login reads as an entitlement fault (legacy: plan), both terminal;
  `max_connections` ends the session (legacy let it pass).

## Regions and terms

US markets data; redistribution is restricted by the plan's terms. Live streaming and
intraday history are paid features; futures are their own entitlement.

## Live best bid and ask

`Q.<ticker>` on stocks, options and futures, `XQ.<pair>` on crypto, `C.<from>/<to>` on forex
(the aggregate channel's spelling); indices have none, refused per pane. Fields `bp`/`ap`
(forex `b`/`a`), sizes `bs`/`as` carried in shares and coins only, `t` in ms (docs, not
measured: no quotes-plan key). Quotes are a separate plan (stocks: Advanced and up). The key
gets one socket per market, so quote channels go out in their own frame and stay unconfirmed
until `success` (`subscribed to: Q.MSFT`) or a first quote names them; an `error` naming a
quote channel, or arriving while one is unconfirmed, fails those quote panes and the socket
carries on.
