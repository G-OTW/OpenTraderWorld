# Binance (spot and USDⓈ-M futures)

One module, two markets (`market.rs`): same wire shapes, different hosts, paths, page sizes
and account endpoints. Spot below; USDⓈ-M differences in their own section.

Adapter version: 0.2.0. Provider API version: v3. Maturity: M2 (mock catalog green; shadow
runs against the live venue, history and socket, identical to the legacy connector).

Sources:
- REST market data: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints
- General info, limits, error codes: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-api-information
- Error codes: https://developers.binance.com/docs/binance-spot-api-docs/errors

## Endpoints

| Endpoint id | Method + path | Canonical command | Canonical event | Weight | Paging |
| --- | --- | --- | --- | --- | --- |
| `klines` | `GET /api/v3/klines?symbol&interval&startTime&endTime&limit=1000` | `RequestHistory{Candles{tf}}` | `HistoryChunk{Candles}` | 2 | time: next `startTime` = last open + tf, only after a full page |
| `exchangeInfo` | `GET /api/v3/exchangeInfo?permissions=SPOT` | `LoadInstruments` | `Instrument` x N, `InstrumentsEnd` | 20 | none (whole list) |
| `bookTicker` | `GET /api/v3/ticker/bookTicker?symbol` | `RequestSnapshot{Quote}` | `Quote` | 2 | none |
| `tickerPrice` | `GET /api/v3/ticker/price` | `RequestBoard` | `Board` | 4 | none |
| `time` | `GET /api/v3/time` | `SyncTime` | `ServerTime` | 1 | none |
| `account` | `GET /api/v3/account` (signed) | `QueryAccount{Summary}` / `{Balances}` | `AccountSummary` / `AccountChunk{Balances}` | 20 | none |
| `openOrders` | `GET /api/v3/openOrders` (signed) | `QueryAccount{OpenOrders}` | `AccountChunk{Orders}` | 80 | none |
| `myTrades` | `GET /api/v3/myTrades?symbol&limit=1000&startTime` then `&fromId` (signed) | `QueryAccount{Fills}` | `AccountChunk{Fills}` | 20 | cursor `<symbol index>:<fromId>`, per symbol |
| socket | `wss://stream.binance.com:9443/ws/<first stream>` + `SUBSCRIBE` / `UNSUBSCRIBE` frames | `Subscribe{Candles{tf}}` / `Unsubscribe` | `Candle`, `SubscriptionAck`, `SubscriptionFail` | 5 frames/s | none |

## Field mapping

`klines` row (positional): `0 openTime (ms)` to `Candle.open_ts`; `1..4 open/high/low/close`
(strings) to `Price`; `5 volume` (base asset) to `Qty`; `6 closeTime` (last ms of the bar)
decides `is_final` (`closeTime < received_ts`); `7 quoteAssetVolume` to `quote_volume`;
`8 numberOfTrades` to `trades`. Rows past index 8 are ignored.

`exchangeInfo.symbols[]`: `symbol` to `InstrumentId.native`; `baseAsset/quoteAsset` to the
display `BASE/QUOTE`; `status == "TRADING"` to `active`; filter `PRICE_FILTER.tickSize` to
`tick_size`; `LOT_SIZE.stepSize/minQty` to `lot_size/min_qty`.

`bookTicker`: `bidPrice/askPrice` to `bid/ask`, `bidQty/askQty` to sizes. An answer for
another symbol than the one asked is a decode error.

## Auth

Market data: none. Account: HMAC-SHA256 over the query string (`HmacQuerySigner`, checked
against the documented example), key in `X-MBX-APIKEY`, `timestamp` from the exchange clock
(`SyncTime` before each account read), `recvWindow=20000`. Secrets `api_key`, `api_secret`
(a read-only key is enough).

## Socket

One socket per connector. The path names the first stream; every stream (the first
included) is subscribed by frame, several in one frame when panes join together. A kline
message names its symbol (`s`) and interval (`k.i`): that pair routes it. `k.x` is the closed
flag, `E` the venue's event time. `{"result":null,"id":N}` acks frame N; `{"code":..,"msg":..,"id":N}`
refuses it, reported on each instrument of that frame.

`<symbol>@bookTicker` streams the best bid and ask on every change (`Subscribe{Quote}`,
`live.quote = push`): `b`/`a` the prices, `B`/`A` the sizes. Spot sends no event time (stamped
on receipt); a zero side (halted book) emits nothing.

## Account

- `myTrades` is per symbol: the caller names the symbols. A period opens on `startTime`
  alone and pages by `fromId` (the 24 h ceiling applies to `startTime` + `endTime` together);
  the period end is inclusive and enforced on the first fill past it.
- The quote currency of a fill or an order comes from the instrument list.
- Balances: free + locked, zero balances left out, no cost basis.
- Holdings are valued on the first market in `quote_preference` (USDT, USDC, FDUSD, EUR,
  BTC), direct or reversed.

## Rate limits

`REQUEST_WEIGHT` 6000 per minute per IP. 429 carries `Retry-After`; ignoring it leads to a
418 IP ban (also mapped to `RateLimited`).

## Symbols

Native form is the concatenated pair in upper case (`BTCUSDT`). User input is upper-cased;
anything with a separator or a space is refused (resolve it through the instrument list).

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| -1000, -1001, -1007 | unknown, disconnected, timeout | Transient |
| -1003 | too many requests | RateLimited |
| -1002, -1021, -1022, -2014, -2015 | unauthorized, timestamp, signature, key | Auth |
| -1121 | invalid symbol | NotFound ("bad symbol") |
| -1100 to -1199 (others) | illegal or malformed parameters | InvalidRequest |
| anything else | | the HTTP status decides |

HTTP: 429/418 RateLimited, 401 Auth, 403 Permission, 404 NotFound, 5xx Transient.

## Units and time

Quantities in base asset. Timestamps in milliseconds UTC. A daily bar opens at 00:00 UTC
(epoch day), a weekly bar on Monday 00:00 UTC.

## Quirks

- `endTime` is inclusive on Binance's side; the half-open range is enforced on decode.
- The answer includes the bar still forming; it is emitted with `is_final = false` and OTW
  does not store it.
- A page repeating the previous page's last bar is not believed (rows before the page start
  are dropped).

## Deliberate differences from the legacy connector

- 418 (IP ban) is a rate limit: the download worker parks the job instead of failing it.
- A non-2xx is no longer reported as "bad symbol?": the chart's notice names the real cause
  (rate limit, venue down, bad parameter).
- A stream the socket refuses is reported on its instrument (the pane's banner); the legacy
  feed swallowed the refusal.
- The exchange clock is read once per account operation, not before every signed call.
- The quote currency of a fill or an order comes from the instrument list (loaded once,
  reloaded when a pair is missing) instead of a pair lookup per call.

## USDⓈ-M futures (`binance_futures`)

Sources: https://developers.binance.com/docs/derivatives/usds-margined-futures/general-info

| Endpoint id | Method + path | Canonical | Notes |
| --- | --- | --- | --- |
| `klines` | `GET /fapi/v1/klines` (limit 1500, weight 10) | `RequestHistory{Candles}` | same row shape as spot |
| `exchangeInfo` | `GET /fapi/v1/exchangeInfo` | `LoadInstruments` | `contractType` PERPETUAL / TRADIFI_PERPETUAL = perp, else dated future with `deliveryDate` as expiry; search line `Binance <contractType>` |
| `bookTicker`, `tickerPrice`, `time`, `openOrders` | `/fapi/v1/...` | as spot | |
| `account` | `GET /fapi/v3/account` (signed) | `QueryAccount{Summary}` | wallet balance verbatim, open positions counted |
| `balance` | `GET /fapi/v3/balance` (signed) | `QueryAccount{Balances}` | the margin wallet, no cost basis |
| `positionRisk` | `GET /fapi/v3/positionRisk` (signed) | `QueryAccount{Positions}` | hedge mode names the side, one-way puts it in the sign |
| `userTrades` | `GET /fapi/v1/userTrades?symbol&startTime&endTime&limit=1000` (signed) | `QueryAccount{Fills}` | windows of at most 7 days; inside a window, next page at last fill + 1 ms; cursor `<symbol>:<window start>:<at>:<pages>`, 500 pages max |
| socket | `wss://fstream.binance.com/market/ws/<first stream>` | `Subscribe{Candles}` | as spot, no 1s klines |
| socket, quotes | `wss://fstream.binance.com/public/ws/<symbol>@bookTicker` | `Subscribe{Quote}` | own socket class `public`: the `market` route acks a book ticker and never sends one (measured 2026-10-19); frames carry `E` |

- Instrument ids live on venue `BINANCE_USDM`, so `BTCUSDT` spot and `BTCUSDT` perpetual
  never collide. Dated contracts are spelled `ETHUSDT_250926`.
- Accounts may be set to `env = testnet`: same adapter on `https://testnet.binancefuture.com`
  (declared as `regions.sandbox_url`). A refused key reminds which environment it was sent to.
- Holdings are valued on the contract itself when the asset is one (`quote_self`), else on
  the first listed `<asset><USDT|USDC|BUSD|USD>`, never reversed.
- Deliberate difference: the quote currency of a dated contract (`ETHUSDT_250926`) comes from
  the instrument list (USDT); the legacy suffix rule left it empty.

## Regions and terms

Binance.com refuses US IPs (HTTP 451): no CI smoke test on US-hosted runners.
