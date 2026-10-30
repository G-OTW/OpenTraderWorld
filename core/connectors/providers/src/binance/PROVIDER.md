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

Market data: none. Account and orders: HMAC-SHA256 over the query string (`HmacQuerySigner`, checked
against the documented example), key in `X-MBX-APIKEY`, `timestamp` from the exchange clock
(`SyncTime` before each account read; for orders, at most every 5 min), `recvWindow=20000`. Secrets `api_key`, `api_secret`
(a read-only key is enough to read; orders need Spot & Margin Trading or Futures enabled,
withdrawals stay off: the connection test says which the key has).

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
- Account figures (`Broker::account`), in USDT: cash = the USDT balance, free + locked
  (`GET /api/v3/account?omitZeroBalances=true`), available = its free part. Equity, on live
  only, is the spot wallet's value as Binance computes it (`GET /sapi/v1/asset/wallet/balance
  ?quoteAsset=USDT`, `walletName = "Spot"`, weight 60). Spot Demo Mode serves `/api` alone, so
  its equity stays unstated: OTW does not sum the holdings at its own prices. No margin
  figures (a spot account has none).

- Accounts may be set to `env = testnet`: Spot Demo Mode on `https://demo-api.binance.com`
  (declared as `regions.sandbox_url`), same API, filters and limits as live, prices close to
  live. The older `testnet.binance.vision` takes other keys (`-2015` with a demo key, measured
  2026-10-23).

## Orders (execution, v2)

`trade.rs` builds the requests, `otw-core/src/brokers/binance.rs` sends them. Testnet only
until the order path is rated E4 (`[orders] level`, spec 9): an account set to live is refused
before anything is sent, and needs live trading turned on for it in OTW once E4.

| Canonical | Spot | USDⓈ-M |
| --- | --- | --- |
| place market / limit | `POST /api/v3/order` `MARKET` / `LIMIT` | `POST /fapi/v1/order` `MARKET` / `LIMIT` |
| place stop / stop-limit | `POST /api/v3/order` `STOP_LOSS` / `STOP_LOSS_LIMIT` | `POST /fapi/v1/algoOrder` `algoType=CONDITIONAL`, `STOP_MARKET` / `STOP`, `triggerPrice` (the order route answers `-4120`) |
| place MIT / LIT | `TAKE_PROFIT` / `TAKE_PROFIT_LIMIT`, `stopPrice` | algo `TAKE_PROFIT_MARKET` / `TAKE_PROFIT`, `triggerPrice` |
| trailing stop | `STOP_LOSS` + `trailingDelta` (whole basis points), `stopPrice` = start | algo `TRAILING_STOP_MARKET`, `callbackRate` (%), `activatePrice` = start |
| post-only, iceberg | `LIMIT_MAKER`; `icebergQty` (GTC limits) | `timeInForce=GTX`; none |
| trigger, close on trigger | | `workingType` `CONTRACT_PRICE` / `MARK_PRICE`; `closePosition=true` (no size) |
| OCO, exits | `POST /api/v3/orderList/oco` (limit half as `LIMIT_MAKER`), `oto` / `otoco` for a limit entry; a market entry's exits after its fill | plain orders OTW links (below) |
| time in force | `GTC`, `IOC`, `FOK` | `GTC`, `IOC`, `FOK`, `GTD` (`goodTillDate`, at least 10 min ahead) |
| change | `POST /api/v3/order/cancelReplace` `STOP_ON_FAILURE`, same `newClientOrderId`, quantity = size left | limit: `PUT /fapi/v1/order` (price, size); stop: no algo amend exists, the changed stop goes out first under the next client id (`k1.r1`), then the old one is cancelled |
| cancel | `DELETE /api/v3/order?origClientOrderId` | `DELETE /fapi/v1/order` / `DELETE /fapi/v1/algoOrder?clientAlgoId` |
| look up | `GET /api/v3/order?origClientOrderId` | `GET /fapi/v1/order` / `GET /fapi/v1/algoOrder` |
| working orders | `GET /api/v3/openOrders` | `GET /fapi/v1/openOrders` + `GET /fapi/v1/openAlgoOrders` |
| cancel all | `DELETE /api/v3/openOrders?symbol` per symbol | `DELETE /fapi/v1/allOpenOrders` + `DELETE /fapi/v1/algoOpenOrders` per symbol |
| close position | | `MARKET reduceOnly=true`, size from `GET /fapi/v3/positionRisk?symbol` |
| filters | `GET /api/v3/exchangeInfo?symbol` (20) | `GET /fapi/v1/exchangeInfo` (1, every symbol) |
| reference price | `GET /api/v3/referencePrice` (2), else `GET /api/v3/avgPrice` (2) | mark price, `GET /fapi/v1/premiumIndex?symbol` (1) |
| open orders on a symbol | `GET /api/v3/openOrders?symbol` (6) | `GET /fapi/v1/openOrders?symbol` + `GET /fapi/v1/openAlgoOrders?symbol` |
| order buckets | `GET /api/v3/rateLimit/order` (40) | `GET /fapi/v1/rateLimit/order` (1) |
| dry run (live) | `POST /api/v3/order/test` | `POST /fapi/v1/order/test` (plain orders; the algo book has none) |
| key rights (live) | `GET /sapi/v1/account/apiRestrictions` (1) | same, on the spot host |
| dead man's switch | none | `POST /fapi/v1/countdownCancelAll?symbol&countdownTime` (10) |
| leverage, margin mode | | `GET /fapi/v1/symbolConfig` (5), read only |

- An order's OTW id is `SYMBOL:clientOrderId` (`SYMBOL:algo:clientAlgoId` for a futures
  stop): a spot change keeps the client id, not the order number.
- Client ids: 1 to 36 of `.A-Z:/a-z0-9_-`. Binance refuses a duplicate only while the first
  works: an id this process already sent for the key is refused here.
- Filters checked before sending, from `exchangeInfo` (kept a day, dropped after a refusal
  naming a filter: `-1013`, `-4013`, `-4014`, `-4016`, `-4023`, `-4024`, `-4164`, `-4005`):
  `PRICE_FILTER` (tick, min, max), `LOT_SIZE` (step, min, max), `MARKET_LOT_SIZE` (market
  orders), `NOTIONAL` / `MIN_NOTIONAL`, `PERCENT_PRICE` / `PERCENT_PRICE_BY_SIDE`,
  `MAX_NUM_ORDERS`, `MAX_NUM_ALGO_ORDERS`. A refusal names the filter and the nearest values
  that pass; nothing is adjusted. A bound set to 0 is no bound.
  - Notional is price x size: on spot every order with a limit price, and a market order
    when `applyToMarket` / `applyMinToMarket` / `applyMaxToMarket` says so, valued at the
    reference price; on USDⓈ-M a plain limit, and a market order at the mark price, a
    reduce-only order exempt (`-4164`: "unless you choose reduce only"). Stops without a limit
    price and USDⓈ-M conditional orders are left to the venue.
  - Price band: spot `price` within `multiplierDown` to `multiplierUp` (per side for
    `PERCENT_PRICE_BY_SIDE`) of the symbol's reference price (`referencePrice`), else of the
    average price over `avgPriceMins` (`avgPrice`, used only when its `mins` equals the
    filter's); USDⓈ-M a buy limit at most mark x `multiplierUp`, a sell at least mark x
    `multiplierDown`. A reference that cannot be read leaves the band to the venue.
  - Open orders: the symbol's working orders are counted (one call, two on USDⓈ-M) against
    what the ticket adds (an OCO and an OTO add 2, an OTOCO 3, as Binance counts them).
- Balance and margin are the venue's (`-2010`, `-2019`), answered synchronously.
- No day order on either market. USDⓈ-M hedge mode is read (`GET /fapi/v1/positionSide/dual`,
  kept 5 min): every order then names `positionSide` (a buy works the long, a sell the short,
  a reduce-only or close-position one the other side's) and sends no `reduceOnly`; a symbol
  held both ways lists, and closes, each side on its own (`SYMBOL:LONG`, `SYMBOL:SHORT`).
- Spot order lists link at the venue: cancelling any order of a list cancels the list. An
  OTO/OTOCO entry is a limit (`workingType` takes `LIMIT` and `LIMIT_MAKER` only); a stop
  entry takes no exits. A market entry's exits go out once it reads back filled, as one order
  or an OCO; refused then, the error says the entry filled without them.
- USDⓈ-M links nothing at the venue: a ticket's exits go out with the entry as reduce-only
  orders of its size, and an OCO pair as two plain orders, and OTW links them by reading them
  every 5 s: an entry gone unfilled cancels its exits, and once it has filled (a pair has
  none) the first to fill cancels the others. After a restart the link is taken back up from
  the stored legs still working (`Broker::resume`): an entry with the exits hung on it, legs
  of one group with no entry, and exits whose entry no longer works (filled: an OCO now).
- A futures stop has no change in place (no algo amend endpoint): the changed stop goes out
  first under the next client id (`k1` becomes `k1.r1`, then `k1.r2`; the base is cut to keep
  36 characters), so **its OTW id changes** (`SYMBOL:algo:k1.r1`), then the old one is
  cancelled. A stop works throughout; both work for that moment. Refused, the old one stands
  unchanged. If the old one fired meanwhile, the new one is cancelled. The replacement keeps
  reduce-only, close-position, the trigger price kind and, in hedge mode, the position it
  works; a trailing stop is not changed in place. The old id still names it here (cancel,
  change, link); after a restart only the new id does.
- A cancel answered `-2011` reads the order back: any final state (cancelled, expired, or
  filled before the cancel reached it) is done (spec 6.2); the cycle and the stream show how
  it ended. A cancel-replace that cancelled and failed to place (`-2021`) says the order is
  gone.
- Live gates, live accounts only (refused below E4 anyway, so not exercised yet): before the
  first live order of a session the key's rights are read (`apiRestrictions`): trading on for
  the market (`enableSpotAndMarginTrading` / `enableFutures`), `enableWithdrawals` off,
  `ipRestrict` on, else refused with the setting to change. Then the order is dry run on the
  test route (validated, not sent to the matching engine); a ticket whose orders have no test
  route (an order list's entry is tested alone; USDⓈ-M conditional orders) leaves the dry run
  to the next one. Skipped on testnet: Spot Demo Mode serves no `/sapi`.
- USDⓈ-M dead man's switch: once OTW placed on a symbol, `countdownCancelAll` is armed for
  120 s and refreshed every 30 s while anything works there; stopped (`countdownTime=0`)
  once nothing does. If OTW goes away, Binance cancels **every** order on the symbol, OTW's
  or not. Armed on testnet too. The account setting `cancel_on_disconnect = off` leaves it
  unarmed. Whether it also cancels the algo book is not stated in the docs: unverified.
- USDⓈ-M positions carry the symbol's leverage and margin mode (`cross` / `isolated`, from
  `marginType` `CROSSED` / `ISOLATED`), read from `symbolConfig`: `positionRisk` v3 no longer
  has them. OTW never changes either.
- The book follows the account live on its user data stream (below).

## Stream (user data)

`user_stream.rs` names the socket and reads its frames; `otw-core/src/brokers/binance_events.rs`
holds the socket (`Broker::events`).

Sources:
- Spot: https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream,
  https://developers.binance.com/docs/binance-spot-api-docs/web-socket-api (User Data Stream
  requests, signed request example), Demo Mode hosts: `demo-mode/general-info.md` of the spot docs.
- USDⓈ-M: https://developers.binance.com/en/docs/products/derivatives-trading-usds-futures/user-data-streams,
  its ws-streams schemas (`orderTradeUpdate`, `accountUpdate`, `algoUpdate`, `listenKeyExpired`)
  and the WebSocket change notice (`/public`, `/market`, `/private`).

| | Spot | USDⓈ-M |
| --- | --- | --- |
| socket | WebSocket API `wss://ws-api.binance.com:443/ws-api/v3`, demo `wss://demo-ws-api.binance.com/ws-api/v3` | `wss://fstream.binance.com/private/ws/<listenKey>`, testnet `wss://stream.binancefuture.com/private/ws/<listenKey>` |
| subscribe | `userDataStream.subscribe.signature`: `apiKey`, `recvWindow`, `timestamp`, `signature` (HMAC over the sorted parameters), weight 2; answer `{"status":200,"result":{"subscriptionId"}}` | `POST /fapi/v1/listenKey`, `X-MBX-APIKEY` only (no signature), weight 1; a POST on an account with a key answers that key |
| keepalive | none (the subscription lives with the socket) | `PUT /fapi/v1/listenKey` every 30 min (the key lives 60 min); `-1125`: gone, reconnect with a new one |
| events read | `executionReport` | `ORDER_TRADE_UPDATE`, `ALGO_UPDATE` (the algo book), `ACCOUNT_UPDATE` (positions) |
| events left | `outboundAccountPosition`, `balanceUpdate`, `listStatus`, `externalLockUpdate` (no book row) | balances, `ACCOUNT_CONFIG_UPDATE`, `MARGIN_CALL`, `TRADE_LITE`, `CONDITIONAL_ORDER_TRIGGER_REJECT`, `STRATEGY_UPDATE` |
| end of stream | `eventStreamTerminated`, `serverShutdown`, 24 h socket limit | `listenKeyExpired`, 24 h socket limit |

- Spot events come wrapped (`{"subscriptionId", "event"}`), USDⓈ-M ones bare.
- HMAC keys work: `subscribe.signature` takes any key type (spot changelog 2025-08-12); only
  `userDataStream.subscribe` after `session.logon` wants Ed25519.
- An order event is the order whole, under the book's id: `SYMBOL:clientOrderId`, and
  `SYMBOL:algo:clientAlgoId` for an `ALGO_UPDATE`. A spot cancel's report carries the cancel's
  own id in `c` and the order's in `C`: the order's is used.
- To the book: a working state is `order` (whole row), a fill (`x = TRADE`) on an order still
  working is `filled`, a final state is `done` with its reason (spot `r` reject reason or `eR`
  expiry reason, USDⓈ-M `er` code, algo `rm`). An algo order that fired (`TRIGGERED`,
  `FINISHED`) is done, as the REST read says it: what it placed is an order of its own.
- USDⓈ-M positions: a one-way position (`ps = BOTH`) is a `position` row (zero is flat). A
  hedge-mode one is a resync: the book names a symbol held both ways by its side and one held
  one way by the symbol, which one event cannot tell.
- Gaps: every reconnect, an expired or lost listen key, a frame that cannot be read and an
  unknown order state are `resync` (the book is read again). Silence: pinged after 60 s,
  dropped 20 s later. Reconnects back off 1 s to 60 s; after 10 failed in a row the stream
  ends and the page reads on a cycle again.
- One socket per follower (page); USDⓈ-M followers share the account's listen key. The key is
  not deleted when a follower leaves (another may use it); it expires on its own.

## Rate limits

`REQUEST_WEIGHT` 6000 per minute per IP. 429 carries `Retry-After`; ignoring it leads to a
418 IP ban (also mapped to `RateLimited`).

Orders (spec 8): every successful order answer carries `X-MBX-ORDER-COUNT-(n)(unit)` (spot
`10S` and `1D`, USDⓈ-M `10S` and `1M`). The counts are held against the account's buckets
(`rateLimit/order`, read once an hour): a ticket that would overflow one is refused, not
queued, with the seconds left in that bucket (a count older than its interval is not held
against it). A 429 holds new orders on that host, a 418 every call, until its
`Retry-After`; cancels go through a 429 hold. `-1015` (too many new orders) is `RateLimited`.

## Symbols

Native form is the concatenated pair in upper case (`BTCUSDT`). User input is upper-cased;
anything with a separator or a space is refused (resolve it through the instrument list).

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| -1000, -1001, -1007 | unknown, disconnected, timeout | Transient |
| -1003, -1015 | too many requests, too many new orders | RateLimited |
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
