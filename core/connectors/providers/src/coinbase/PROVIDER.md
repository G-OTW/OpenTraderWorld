# Coinbase

Adapter version: 0.1.0        Provider API version: Exchange REST + feed, Advanced Trade v3        Maturity: M2

Sources:
- Exchange candles: https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproductcandles
- Exchange products and ticker: https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproducts
- Exchange feed, `matches`: https://docs.cdp.coinbase.com/exchange/docs/websocket-channels
- Advanced Trade REST: https://docs.cdp.coinbase.com/advanced-trade/reference
- Advanced Trade JWT auth: https://docs.cdp.coinbase.com/advanced-trade/docs/rest-api-auth
- Rate limits: https://docs.cdp.coinbase.com/advanced-trade/docs/rest-api-rate-limits

M2: mock catalog green; shadow against the live venue (history, product list, feed)
identical to the legacy connector; account parity on mock, real shadow ready with a key.

## Endpoints

| Endpoint id | Method + path / WS channel | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `candles` | `GET /products/{id}/candles?granularity&start&end` (Exchange) | `RequestHistory{Candles}` | 1 | time: 300 steps a call, both ends counted, the end bar opens the next page |
| `products` | `GET /products` (Exchange) | `LoadInstruments` | 1 | none |
| `ticker` | `GET /products/{id}/ticker` (Exchange) | `RequestSnapshot{Quote}` | 1 | none |
| `board`, `productQuotes` | `GET /api/v3/brokerage/market/products?product_type=SPOT` (public) | `RequestBoard`; product quote currencies kept one hour | 1 | none |
| `accounts` | `GET /api/v3/brokerage/accounts?limit=250` (JWT) | `QueryAccount{Summary}`, `{Balances}` | 1 | none |
| `ordersOpen` | `GET /api/v3/brokerage/orders/historical/batch?order_status=OPEN&limit=100` (JWT) | `QueryAccount{OpenOrders}` | 1 | none |
| `fills` | `GET /api/v3/brokerage/orders/historical/fills?limit=100&start_sequence_timestamp&end_sequence_timestamp[&product_ids][&cursor]` (JWT) | `QueryAccount{Fills}` | 1 | cursor `<pages>:<venue cursor>`, 500 pages max |
| feed `matches` | `wss://ws-feed.exchange.coinbase.com`, `{"type":"subscribe","product_ids":[..],"channels":["matches"]}` | `Subscribe{Trades}` | | |

## Auth

A JWT per request, valid 120 s: header `{alg: EdDSA, typ: JWT, kid: <key name>, nonce}`,
payload `{sub: <key name>, iss: cdp, nbf, exp, uri: "GET api.coinbase.com<path>"}` (query
string excluded), sent as `Authorization: Bearer`. Ed25519 keys only (64 base64 bytes, or a
32-byte seed); an ECDSA PEM is refused by name. Secret `api_private_key`; setting
`key_name` (`organizations/<org>/apiKeys/<key>`, checked when saved). Host clock, no clock
call. Signature checked against RFC 8032 vector 1 and verified with the public key.

## Rate limits

- Exchange public REST: about 10 requests per second per IP; OTW's worker paces history at
  200 ms.
- Advanced Trade private REST: 30 requests per second.
- Over the limit: HTTP 429 (`RateLimited`, `Retry-After` honoured when sent).

## Symbols

Native id = the product id (`BTC-USD`), used in URL paths, so only letters, digits and `-`
are accepted. Display symbol = `display_name` (`NEON/USD` for some products, else the id).
Base and quote from `base_currency` / `quote_currency`. Account rows use the product id;
their quote currency comes from the public product list, never from cutting on the dash.
A holding is valued on `<asset>-<quote>` for quote in USD, USDC, USDT, EUR.

## Enum tables

| Coinbase | Canonical |
| --- | --- |
| side `BUY` / `SELL` (account) | `Side::Buy` / `Side::Sell`; anything else `Unknown(raw)` |
| feed `side` `buy` / `sell` (the maker's) | aggressor `Sell` / `Buy`; anything else `Unknown(raw)` |
| granularity 60, 300, 900, 3600, 86400 | `1m 5m 15m 1h 1d` (21600 exists, not declared) |
| wallet `type` containing `FIAT` | fiat wallet (reported as forex) |
| `trading_disabled: true` | inactive |
| order type, order configuration | kept verbatim; prices read from whatever configuration holds them |

Errors: the HTTP status decides (Exchange `{"message":..}`, Advanced Trade
`{"error":..,"message":..}`): 429 RateLimited, 401 Auth, 403 Permission, 404 NotFound ("bad
symbol"), other 4xx InvalidRequest, 5xx Transient. A feed `error` naming a carried product
fails that pane; any other is reported as an error event, not fatal.

## Units and time

- Spot quantities in base units. A fill with `size_in_quote` is sized in money: quantity =
  size / price (exact decimals); at price 0 it is refused with a sentence.
- Candle row `[time, low, high, open, close, volume]`, newest first, JSON numbers (read
  from their text), `time` in seconds, candle open time, UTC epoch aligned.
- No forming flag on candles (`history.candles.final_flag = false`): every bar goes out
  forming and the engine finalizes at the period's end.
- Fill `trade_time`, order `created_time`, feed `time`: RFC 3339, kept to the millisecond.
- Fees positive (cost, `commission` taken as its absolute value), in the quote currency.
- Wallet total = available + hold.

## Quirks

- A candle window counts both ends: 300 steps answer 301 bars, 301 steps are refused
  ("Count of aggregations requested exceeds 300"). An unaligned start begins at the next
  bar (checked against the live venue).
- Fills and orders carry no quote currency; the public product list does.
- The public product list carries a price per product; an unpriced product is left off the
  board.
- The feed streams trades only: every live timeframe is folded by OTW from them.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` with committed bars only.
- A venue outage reads as the provider's in the chart's notice (legacy blamed the symbol).
- Fill and order times keep the millisecond (legacy kept microseconds).
- Wallet totals and quote-sized quantities are exact decimals.
- Suggestions count an asset held entirely by an open order, and add products with a
  working order.
- The price board is the public product list: valuing holdings needs no signed call.
- The feed subscribes a product once whatever the number of panes on it.
- A fill pull longer than 500 pages is an error asking for a shorter period.

## Regions and terms

Advanced Trade accounts follow Coinbase's jurisdictions; the Exchange market data answered
from the test machine. Coinbase offers a sandbox for the Exchange API, not used here.

## Live best bid and ask

Channel `level2_batch` (answered as `level2_50`) on the trade socket: a `snapshot` of the whole
book (about 1 MB on BTC-USD) then `l2update` changes every 50 ms, size 0 removing a level; the
top of the book is the quote (measured 2026-10-19). `ticker` carries a best bid/ask only when a
trade prints and is not used.
