# MEXC (spot and perpetual contracts)

Adapter version: 0.1.0. Provider API versions: spot v3, contract v1. Maturity: M2 (mock
catalog green; history, instrument lists, quotes, the clock and both sockets checked against
the live venue on 2026-10-19; account parity on mock only, no key lent).

One OTW provider, `mexc`, two adapters chosen by the connector's `market` setting (and the
broker's `books` setting): `Mexc::spot()` (venue `MEXC`) and `Mexc::futures()` (venue
`MEXC_FUTURES`, the perpetuals settled in their quote coin: USDT, USDC, USD1). A spot pair is
written `BTCUSDT`, a contract `BTC_USDT`. The coin-settled (inverse, `BTC_USD`) contracts are
not served.

Sources:
- Spot v3 reference: https://www.mexc.com/api-docs/spot-v3/ (the former
  https://mexcdevelop.github.io/apidocs/spot_v3_en/ now redirects there).
- Futures reference: https://www.mexc.com/api-docs/futures/ (the former
  https://mexcdevelop.github.io/apidocs/contract_v1_en/ now redirects there).
- Spot socket protobuf definitions: mexcdevelop/websocket-proto at commit
  `0c9c4f35dd0fadc3a46a350e909a93379d81e811` (`PushDataV3ApiWrapper.proto`,
  `PublicSpotKlineV3Api.proto`).
- ccxt `python/ccxt/mexc.py` (master, 2026-10-19): contract signing (`sign`), the
  open-orders call without a symbol, the contract fill sample.
- Measured answers of the public endpoints and sockets, 2026-10-19 (recorded in `tests.rs`).

## Endpoints

Both books below `https://api.mexc.com`: the futures change log moved the contract API there
("futures API base domain changed to `https://api.mexc.com`", 2026-01-19,
https://www.mexc.com/api-docs/futures/update-log; https://www.mexc.com/api-docs/futures/integration-guide
"Access URL"). Measured 2026-11-12: the public contract endpoints below answer there.

| Endpoint id | Spot | Futures | Canonical |
| --- | --- | --- | --- |
| `klines` / `kline` | `GET /api/v3/klines?symbol&interval&startTime&endTime&limit=500` | `GET /api/v1/contract/kline/{symbol}?interval&start&end` | `RequestHistory{Candles}` |
| `contract` | | `GET /api/v1/contract/detail/country?symbol` | contract size before a candle page |
| `exchangeInfo` / `contracts` | `GET /api/v3/exchangeInfo` | `GET /api/v1/contract/detail/country` ("Get Contract Info", https://www.mexc.com/api-docs/futures/market-endpoints/get-contract-info) | `LoadInstruments` |
| `bookTicker` / `ticker` | `GET /api/v3/ticker/bookTicker?symbol` | `GET /api/v1/contract/ticker?symbol` | `RequestSnapshot{Quote}` |
| `tickerPrice` | `GET /api/v3/ticker/price` | | `RequestBoard` |
| `time` / `ping` | `GET /api/v3/time` | `GET /api/v1/contract/ping` | `SyncTime` |
| `account` | `GET /api/v3/account` (signed) | | `Summary`, `Balances` |
| `openOrders` | `GET /api/v3/openOrders` (signed, no symbol) | `GET /api/v1/private/order/list/open_orders?page_num&page_size=100` (signed) | `OpenOrders` |
| `myTrades` / `orderDeals` | `GET /api/v3/myTrades?symbol&startTime&endTime&limit=100` (signed) | `GET /api/v1/private/order/list/order_deals/v3?symbol&start_time&end_time&page_num&page_size=100` (signed; https://www.mexc.com/api-docs/futures/account-and-trading-endpoints/get-historical-order-deal-details) | `Fills` |
| `assets` | | `GET /api/v1/private/account/assets` (signed) | `Summary` |
| `openPositions` | | `GET /api/v1/private/position/open_positions` (signed) | `Positions` |

Sockets: spot `wss://wbs-api.mexc.com/ws`, contracts `wss://contract.mexc.com/edge`.

- Spot: `{"method":"SUBSCRIPTION","params":["spot@public.kline.v3.api.pb@<symbol>@<interval>"],"id"}`,
  one pane per frame. The answer echoes `id`; its `msg` is the channel on success and
  `Not Subscribed successfully! [...]. Reason: Blocked!` on a refusal, an unknown pair among
  them (measured). Updates are protobuf binary frames (the JSON channel answers "Blocked!",
  measured): `PushDataV3ApiWrapper` field 1 the channel, 5 and 6 the make and send times in
  ms, 308 a `PublicSpotKlineV3Api` (`interval`, `windowStart` in s, the four prices,
  `volume` base, `amount` quote, all prices and amounts as strings). Ping `{"method":"PING"}`,
  answered `{"id":0,"code":0,"msg":"PONG"}`. 30 subscriptions per socket at most (docs): a
  31st pane is refused by the adapter.
- Contracts: `{"method":"sub.kline","param":{"symbol","interval"}}`, unsubscribed with the
  same `param` (`unsub.kline` with the interval drops that pane only, measured). Answers
  carry no id: `{"channel":"rs.sub.kline","data":"success"}` or
  `{"channel":"rs.error","data":"Contract [NOPE_USDT] not exists"}`, in the order asked
  (measured), matched to the panes in that order and by the name a refusal carries. Updates
  `push.kline`, `data` `{symbol, interval, t (s), o, c, h, l, q (contracts), a (quote)}`.
  Ping `{"method":"ping"}`: the venue drops a client that has not pinged for a minute (docs),
  so every 15 s.

The live runtime hands binary frames to the adapter as bytes (`Feed::on_binary`).

## Field mapping and rules

- Interval tokens: spot archive `1m 5m 15m 30m 60m 4h 8h 1d 1W 1M` (`1h`, `1w`, `2h`, `12h`
  refused, measured); contract archive and both sockets `Min1 Min5 Min15 Min30 Min60 Hour4
  Hour8 Day1 Week1 Month1` (`Hour1` refused, measured). Day at midnight UTC, week on Monday,
  month on the first of the calendar month, on both books (measured).
- Spot archive row: `[open ms, open, high, low, close, base volume, close ms, quote volume]`.
  Contract archive: columns (`time` s, `open`, `close`, `high`, `low`, `vol` contracts,
  `amount` quote, and `realOpen`... `realLow`), numbers. Both books write a bar's `open` as
  the previous bar's close (measured: 499 of 500 spot rows, 1999 of 2000 contract rows) and
  widen `high`/`low` to hold it; the contract `real*` columns are the trade-only values. The
  plain columns are read, as MEXC charts them.
- Paging (measured): spot answers 500 bars at most whatever `limit` says (the docs say 1000),
  over `[startTime, endTime)` in ms, the oldest first; `startTime` alone or `endTime` alone is
  ignored (the latest 500 come back). Contracts answer 2000 at most (2001 over a wide range,
  the newest), `start` and `end` in seconds, both inclusive. Pages walk forwards from the
  range's start, 500 or 2000 bars at a time.
- Lookback (measured, the window moves with the clock): 1m 30 days on both books; 5m and
  15m 360 days; 30m 360 days on spot and from the listing on futures; 1h and longer from the
  listing. A range that starts before the window answers nothing at all, even where it
  overlaps it (measured), so a request is cut at the window plus an hour; a range wholly
  older is refused as Unsupported, naming the date of the oldest bar served.
- Closed flags: neither archive nor socket carries one. Measured over ten minute boundaries
  on BTC, ETH, SOL and XRP: a closed minute was rewritten up to 2.5 s after its close on spot
  (21 rewrites) and up to 12.8 s on futures (26 rewrites), so an archive bar is final 10 s
  (spot) or 60 s (futures) after its close. Socket candles go out as forming
  (`candles_final_flag = false`): the engine closes one when the next starts.
- Futures volume counts contracts; `contractSize` (0.0001 BTC on `BTC_USDT`) turns it into
  coins. A candle page asks `contract/detail?symbol` first when the instrument list is not
  loaded; a futures pane reads the list before it subscribes (`live_requires_instruments`).
- Spot list: tick = 10^-`quotePrecision`, lot = 10^-`baseAssetPrecision` (the step the order
  path checks), min = `baseSizePrecision` ("min order quantity"), active when `status` is
  `1` and `isSpotTradingAllowed`. Contract list: `futureType` 1 and `settleCoin` equal to
  `quoteCoin` only, tick = `priceUnit`, lot = `volUnit` contracts, min = `minVol` contracts,
  active when `state` is 0.
- Quotes: spot `bookTicker` with sizes; the contract ticker carries `bid1`/`ask1` and no
  sizes.

## Account

- Spot: `/api/v3/account` gives the balances (free + locked), `canTrade`, `accountType` and
  `permissions`. Open orders are asked with no symbol (ccxt does the same; the docs mark the
  symbol as required, unverified without a key). `myTrades` answers one pair at a time, the
  last month only (docs), 100 rows at most and no cursor: a slice whose page comes back full
  is asked again at half the width before any of it is kept, down to a second (a full
  second is refused, naming the web export); a slice whose page is under half full lets the
  next widen again, up to a week. The broker declares `history_days = 30`.
- Futures: `account/assets` for the summary (USDT equity and unrealised), `open_positions`
  (`holdVol` contracts, `positionType` 1 long, 2 short), open orders by page, `order_deals`
  per contract over 30-day slices (the venue takes 90) paged by `page_num`. Side: 1 open long
  and 2 close short buy, 3 open short and 4 close long sell. Order types 1 limit, 2 post only,
  3 IOC, 4 FOK, 5 market, 6 market to limit.
- Both books answer fills one instrument at a time (`fills_need_symbols`). A broker account
  names pairs and contracts in one list; each book takes the names spelled its way
  (`BTC_USDT` futures, `BTCUSDT` spot) and sends nothing when none is.
- Futures orders and positions are read in the base coin, as the order path states them: an
  open-orders or positions read takes the contract list first and multiplies `vol`, `dealVol`
  and `holdVol` by the contract's `contractSize` (a contract missing from the list is an
  error, never a guess). Futures fills too (`vol` times `contractSize`, one unit one coin), as
  the order path reads them. The fills answer is read as a list (the page's example) or as a page holding
  `resultList` (the page's field table).

## Auth

- Spot: lowercase hex HMAC-SHA256 of the query string with `recvWindow=10000` and
  `timestamp` (ms) appended, sent as `signature`; key in `X-MEXC-APIKEY` (the SDK's
  `HmacQuerySigner`). Checked against example 2 of MEXC's docs; their example 1 prints a
  different signature for the same string, and Python's `hmac` agrees with example 2.
- Contracts: lowercase hex HMAC-SHA256 of `key + Request-Time + sorted query` on a GET, of
  `key + Request-Time + the JSON body as sent` on a POST ("For POST requests, the parameters
  to sign are the JSON string (no dictionary sorting required)", integration guide), sent as
  `ApiKey`, `Request-Time` (ms) and `Signature` headers with one `Content-Type:
  application/json`; the query travels in the order signed. Checked against references
  computed with Python's `hmac`.
- Clock: `SyncTime` reads the venue's clock (spot `/api/v3/time`, contracts
  `/api/v1/contract/ping`) before a signed read. Spot refuses a timestamp outside
  `recvWindow` (700003), contracts one more than 10 s off (513).
- Secrets: `api_key`, `api_secret`. A key bound to no IP address expires after 90 days
  (docs).

## Rate limits

Spot: 500 requests per 10 s per endpoint, per IP for public calls and per account for signed
ones; `/api/v3/account` 2 per second; new order and cancel order 12 per second, shared ("Shared
with the cancel order endpoint, 12 requests/second", S/spot-account-trade/new-order; the
cancel's endpoint id `spot_order:cancel` counts in the `spot_order` bucket). Contracts: 20 per
2 s per endpoint; "Get Contract Info" 10 per 2 s (F/market-endpoints/get-contract-info: the
read's `contracts` and `contract`, and the order path's `contracts:rules`, which counts in
`contracts`); place order 4 per 2 s (FT/place-order, `fut_create`).

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| HTTP 429, 429, 510 | rate limit | RateLimited |
| 401, 402, 602, 513, 1000, 10001, 10072, 10073, 700001, 700002, 700003 | key, signature, clock | Auth |
| 406, 700006 | IP not on the key's list | Auth |
| 403, 511, 701, 703, 700007 | permission | Permission |
| -1121, 10007, 30014, 730001, 1001 | unknown pair or contract (measured: -1121, 1001) | NotFound, naming the symbol and the book's spelling |
| 500, 501, 503, 504, 604, 801, 9999 | venue error | Transient |
| anything else (400, 600...) | | InvalidRequest |

Spot refuses with a 4xx status and `{"code","msg"}`; the contract API answers HTTP 200 with
`{"success":false,"code","message"}`. A body that is not a refusal falls back to the HTTP
status. Measured: the contract API answers an unknown key with code 500 ("Internal error"),
so a futures account call refused that way says to check the key.

## Not covered

- Coin-settled and dated futures, margin, sub-accounts, stop and trigger orders (placing; see
  "Trading").
- More than 100 spot fills of one pair within one second.

## Regions and terms

MEXC does not serve some jurisdictions (the United States among them); public market data
answered from the test machine.

## Live best bid and ask

Spot `spot@public.aggre.bookTicker.v3.api.pb@100ms@<symbol>`: protobuf, wrapper field 315
`PublicAggreBookTickerV3Api` (`bidPrice` 1, `bidQuantity` 2, `askPrice` 3, `askQuantity` 4).
Contracts `sub.ticker` / `push.ticker`: `bid1`, `ask1`, `timestamp`, about once a second,
answered `rs.sub.ticker` in order like a candle (measured 2026-10-19).

## Trading

Orders on both books, written from the documentation alone (no account, no key, no live order):
level E2D. No sandbox exists on either book ("No testnet or sandbox environment is mentioned",
https://www.mexc.com/api-docs/spot-v3/introduction; the futures integration guide names none):
every account is live. Mapper: `trade.rs`; error tables: `trade_errors.rs`; fixtures:
`fixtures/trade/` (sources in `trade_tests.rs`). Pages read 2026-11-12 through the live site
(`S` = https://www.mexc.com/api-docs/spot-v3/, `F` = https://www.mexc.com/api-docs/futures/,
`FT` = `F/account-and-trading-endpoints/`, `FW` = `F/websocket-api/`), and the docs' GitHub
mirror (`GH-S`: mexcdevelop/apidocs branch `spot_v3_en` 349c691c; `GH-C`: branch
`contract_v1_en` deae62dc) where the live page prints less.

Futures order placing reopened on 2026-03-31 ("Futures Trading API maintenance ended; trading
access reopened.", F/update-log). It needs KYC (F/integration-guide) and a contract whose
`apiAllowed` is true (F/market-endpoints/get-contract-info).

### Spot (`api.mexc.com`, v3)

| Call | Endpoint | Source |
| --- | --- | --- |
| place | `POST /api/v3/order`, the parameters in the body as `key=value&...` | S/spot-account-trade/new-order |
| dry run | `POST /api/v3/order/test`, the same parameters, no client id; answers `{}` | S/spot-account-trade/test-new-order |
| cancel | `DELETE /api/v3/order?symbol&orderId` | S/spot-account-trade/cancel-order |
| cancel all | `DELETE /api/v3/openOrders?symbol`, one call per pair with working orders ("Cancel all pending orders for a single symbol, including OCO pending orders") | S/spot-account-trade/cancel-all-open-orders-on-a-symbol |
| lookup | `GET /api/v3/order?symbol&origClientOrderId` ("up to 7 days"), by id `?orderId` | S/spot-account-trade/query-order |
| working orders | `GET /api/v3/openOrders` (every pair) or `?symbol` | S/spot-account-trade/current-open-orders |
| fills | `GET /api/v3/myTrades?symbol&startTime&endTime&limit=100`, an hour per call | S/spot-account-trade/account-trade-list |
| rules | `GET /api/v3/exchangeInfo?symbol` | S/market-data-endpoints/exchange-information |
| price for a market buy | `GET /api/v3/ticker/bookTicker?symbol`, the ask | S/market-data-endpoints |
| stream | `POST /api/v3/userDataStream` (listen key), `PUT` every 30 min, `wss://wbs-api.mexc.com/ws?listenKey=` | S/websocket-user-data-streams |

- Body: "Parameters in the Request Body use the raw `key=value&...` format", "Even when
  `application/json` is used, the body content must still be the raw parameter string",
  "Do not use `Content-Type: application/x-www-form-urlencoded`, or the request may return
  `700013 Invalid content Type`" (S/introduction). OTW sends `Content-Type: application/json`
  once; the signer puts `recvWindow`, `timestamp` and `signature` in the query.
- Signature: totalParams is "the query string concatenated with the request body", "with no
  extra `&` in between" (S/introduction). Byte tests: Example 2 (query only,
  `fd3e4e85...`) and Example 3 (mixed, `d1a67661...`), the doc's key and secret.
- Types (the time in force is the type): `LIMIT` (GTC), `LIMIT_MAKER` (post-only),
  `IMMEDIATE_OR_CANCEL`, `FILL_OR_KILL` (limit IOC and FOK), `MARKET` (asked GTC or IOC; FOK
  refused before I/O) (S/public-api-definitions, GH-S "ENUM definitions").
- A market sell is `quantity` (base); a market buy is `quoteOrderQty` ("On the BUY side, the
  order will buy as many BTC as quoteOrderQty USDT can.", S/spot-account-trade/new-order):
  the trader values the size at the best ask read just before sending, rounded down to
  10^-`quoteAssetPrecision`. The filled size may differ a little; the answer says so.
- Client id `newClientOrderId`: no rule stated; held to the futures rule (32, letters, digits,
  `-`, `_`). Not echoed by the place answer (its fields are `symbol`, `orderId`,
  `orderListId`, `price`, `origQty`, `type`, `side`, `stpMode`, `transactTime`).
- Status words (S/public-api-definitions): `NEW`, `PARTIALLY_FILLED`, `FILLED`, `CANCELED`,
  `PARTIALLY_CANCELED` (Cancelled after a partial fill). `NEW` with `executedQty` above 0 reads
  PartiallyFilled. Any other word stays unknown.
- Rules (S/market-data-endpoints/exchange-information): tick 10^-`quotePrecision`, lot
  10^-`baseAssetPrecision`, `baseSizePrecision` "min order quantity", `quoteAmountPrecision`
  "min order amount", `maxQuoteAmount` "max Quote Amount". `status` "1 - online, 2 - Pause, 3 -
  offline" (the example writes `ENABLED`: both read as trading); `isSpotTradingAllowed` false
  is closed; `tradeSideType` "2 - buy order only, 3 - Sell order only, 4 - Close" hold buy-only,
  sell-only, closed.
- Fills: `commission` read as a cost in `commissionAsset` (sign not stated). 100 rows per call
  and no cursor ("Default 100; max 100", GH-S): an hour per call, and a full page is said as
  an error rather than cut.
- Key: a key trades only the pairs set on it ("Trading pairs are set under My API Key",
  S/introduction); 70011 and 30020 are Permission naming that fix.
- Private stream: protobuf frames only (`spot@private.orders.v3.api.pb`,
  `spot@private.deals.v3.api.pb`, wrapper fields 304 and 306, mexcdevelop/websocket-proto
  0c9c4f35). Subscribe answers and the pong are JSON text (`{"id":0,"code":0,"msg":...}`). The
  frame's order id is written `C02__...` in the examples, the REST `orderId` otherwise: an
  order frame reads the order again by that id (`OrderChanged`); a REST call that does not
  know it reads the whole book. Ping `{"method":"PING"}` every 20 s ("If the subscription is
  successful but there is no data flow, the server will disconnect after one minute"). The
  listen key is "valid for 60 minutes", extended by `PUT` ("It is recommended to send a
  request every 30 minutes", GH-S).

### Futures (`api.mexc.com`, contract v1)

| Call | Endpoint | Source |
| --- | --- | --- |
| place | `POST /api/v1/private/order/create`, JSON body | FT/place-order |
| cancel | `POST /api/v1/private/order/cancel`, body `[orderId]` | FT/cancel-orders, GH-C "Cancel the order" (`None` / `List<Long>`) |
| cancel all | `POST /api/v1/private/order/cancel_all`, `{"symbol"}` per contract with working orders; `planorder/cancel_all` and `stoporder/cancel_all` the same way; trailing orders one by one (`trackorder/cancel`) | FT/cancel-all-orders-under-a-contract, FT/cancel-all-planned-orders, FT/cancel-all-tpsl-planned-orders, FT/cancel-trailing-order |
| lookup | `GET /api/v1/private/order/external/{symbol}/{external_oid}`, by id `GET .../order/get/{orderId}` | FT/get-order-by-external-id, FT/get-order-information-by-order-id |
| working orders | `GET /api/v1/private/order/list/open_orders?page_num&page_size=100` (no symbol parameter) | FT/get-current-orders |
| working conditional orders | plan `GET planorder/list/orders?states=1&page_num&page_size=100&start_time&end_time`; TP/SL `GET stoporder/open_orders`; trailing `GET trackorder/list/orders?states=0` and `?states=1` (`pageIndex=1&pageSize=100`) | FT/get-plan-order-list, FT/get-current-take-profitstop-loss-order-list, FT/query-trailing-orders |
| conditional lookup | plan `planorder/list/orders` with no `states`; TP/SL `stoporder/open_orders` then `stoporder/list/orders` with no `is_finished`; trailing `trackorder/list/orders` once per state 0 to 4 | FT/get-plan-order-list, FT/get-take-profitstop-loss-order-list, FT/query-trailing-orders |
| fills | `GET /api/v1/private/order/list/order_deals/v3?symbol&start_time&end_time&page_num&page_size=100`, 30 days per call | FT/get-historical-order-deal-details |
| positions | `GET /api/v1/private/position/open_positions?symbol` | FT/get-open-positions |
| position mode | `GET /api/v1/private/position/position_mode` ("1 Hedge Mode; 2 One-way Mode") | FT/get-user-position-mode |
| margin and leverage | `GET /api/v1/private/position/leverage?symbol`, one row per `positionType` | FT/get-position-leverage-multipliers |
| rules | `GET /api/v1/contract/detail/country?symbol` (public) | F/market-endpoints/get-contract-info |
| stream | `wss://contract.mexc.com/edge`, login, everything pushed after it | FW/native-ws-endpoint, FW/login-authentication, FW/subscription-filtering |

- Body: `symbol`, `price` (limit types), `vol` (whole contracts), `leverage` (when held),
  `side`, `type`, `openType`, `externalOid`, `positionMode` 1. Decimals travel as JSON numbers
  written from the decimal, never through a float.
- Market orders carry no `price`. FT/place-order marks `price` mandatory ("Price") but gives
  it no meaning for `type` 5 (market) and no example request; no page says which price a
  market order would carry, so none is invented (ccxt `create_swap_order` sends none for a
  market order either). Unverified against the venue: a refusal there is a 600 "Parameter
  error" or 2007 "Order price error", and nothing fills.
- Hedge mode only. `side` "1 open long, 2 close short, 3 open short, 4 close long": a buy on
  the long opens it (1), a sell on it closes it (4), a sell on the short opens it (3), a buy
  on it closes it (2). A reduce-only order is the close code of the side it reduces.
  `positionMode` "Default Hedge Mode; 1: Hedge Mode; 2: One-way Mode" and `reduceOnly`
  "only applicable in one-way mode" are stated, but no page says which `side` code a one-way
  order takes, nor how `reduceOnly` combines with it: every order of an account in one-way mode
  (read from `position_mode`, account wide), a close included, is refused before I/O, naming
  the fix (switch to hedge mode at MEXC). An order row read back with `positionMode` 2
  (FT/get-current-orders) acts on the net position, its direction the `side` code's.
- Position mode: read from `position_mode` (account wide), never from the rows (every row is
  sided by `positionType` "1 long 2 short" in both modes). Read again after 7002 "Position mode
  mismatch" or 2043 "The order does not match the position", and pushed by
  `push.personal.position.mode` (FW/position-mode-change).
- Margin: `openType` "1: isolated,2: cross", mandatory on every order, read per side from
  `position/leverage` (`margin_per_side`), never chosen by OTW. Read again after 2027, 7003,
  6007, 9070, 9071.
- Leverage: "Leverage, must be provided when opening a position" (FT/place-order). OTW sends
  the leverage `position/leverage` states when both sides hold the same one; when they differ,
  or none is held, none is sent and the venue answers (the trader keeps one leverage per
  instrument, MEXC one per side: one side's value is never sent for the other).
- Types: "1: limit,2: Post Only (maker only),3: IOC,4: FOK,5: market": limit GTC 1, post-only
  2, limit IOC 3, limit FOK 4, market 5 (asked GTC or IOC; FOK refused before I/O).
- Sizes: contracts of `contractSize` base coins (`counts_contracts`); a size that is not a
  whole number of contracts is refused with the nearest ones. Rules: tick `priceUnit`
  ("Minimum price tick"), lot `volUnit` contracts ("Minimum quantity step"), min `minVol`,
  max `limitMaxVol` ("Max contracts per limit order") for a limit and `maxVol` ("Maximum order
  size (contracts)") for a market order. `state` "0 = enabled, 1 = delivery, 2 = delivered, 3
  = offline, 4 = paused" and `apiAllowed` ("Whether API trading is allowed"): only state 0 with
  API trading allowed trades. `type` ("1 = normal, 2 = suspended") is not read: measured
  2026-11-12, 538 of 1188 listed contracts with state 0 and `apiAllowed` true carry type 2.
- Client id `externalOid`: "External order ID too long (max. 32 digit)" (2030, F/error-code);
  letters, digits, `-`, `_` (both in the docs' examples).
- Answers: success `{"success":true,"code":0,"data":...}`, refusal
  `{"success":false,"code","message"}` (F/integration-guide). The place answer's `data` is
  `{"orderId","ts"}` (the mirror's: the bare id; both read). 6008 "Order placed successfully"
  is an acceptance without the id: the order is found by `externalOid`, never sent again.
- Status `state`: "1: pending,2 unfilled,3 filled,4 canceled,5 invalid": New, New
  (PartiallyFilled with `dealVol` above 0), Filled, Cancelled, Rejected.
- Fills: `fee` "positive = paid by user, negative = received by user" (FW/fill-details), read
  as a cost with its sign. `vol` in contracts, read in the base coin on the order path and
  the account read alike. `category` 2 (forced liquidation custody) and 4 (ADL) mark a
  position moved with no order behind it.
- Private stream: the login `{"method":"login","param":{"apiKey","reqTime","signature"}}`,
  signed over `accessKey + timestamp` (FW/signature-string), answered `rs.login` "success" or
  `rs.error`. "After login, all personal data will be pushed by default" (FW/subscription-filtering):
  nothing is subscribed. `push.personal.order`, `push.personal.order.deal`,
  `push.personal.position` (sizes in contracts, read in the base coin) and
  `push.personal.position.mode` are read; the other channels are left. Ping
  `{"method":"ping"}` every 15 s ("Send a ping every 10-20 seconds", FW/command-details-for-data-exchange).

### Left out (why)

- Futures plan orders: no `externalOid` on `planorder/place/v2`, so a lost answer cannot be
  found; `executeCycle` caps them at 7 days (GH-C, survey).
- Attached TP/SL (`stopLossPrice`, `takeProfitPrice`) and position TP/SL (`stoporder/place`):
  the doc does not say whether a leg cancels its sibling, nor what becomes of the TP/SL row of
  an entry cancelled unfilled; the answer's `data` shape contradicts itself.
- Trailing and chase orders, flash close, BBO, reverse, `close_all` (scope and ids not
  stated), batch place ("Only supported for market maker accounts.", FT/batch-place-order).
- Amend (`change_limit_order`): whether the id is kept is not stated, and 2054 suggests a
  cancel and replace; refused in one-way mode (2053) and on partial fills (2052).
- Spot stops: the "Market TP/SL" type (change log 2025-11-25) has no documented placing
  parameter; `STOP_MARKET_ORDER` is "query only". Spot batch orders.
- Futures one-way orders, closes included (above). Coin-settled and delivery futures.
- Placing futures plan, TP/SL and trailing orders (above). Those placed elsewhere are listed
  with the account's orders under `cond:<kind>:<id>` and cancelled (one, or with cancel all),
  each kind on its own endpoints (table above): the plan list's `start_time` and `end_time`
  are required, so the signer writes the last 8 days to the signing time (a plan order lives 7
  days at most, `executeCycle` "2: 7 days", and a day covers the two clocks); the trailing
  list's `states` is a `List<int>` whose GET form is not stated, so each call names one state.
  One of those lists the venue refuses (InvalidRequest, Permission, Rejected) is read as empty
  and logged, the other orders still read; a rate limit, a key refusal or a lost answer still
  fails the read.

### Errors

Lost answers are Unknown: looked up by the client id, never sent again. A code neither table
lists is Unknown, unmapped, and alerts ("MEXC answered code X (msg), which OpenTraderWorld does
not map"); a refusal reads "MEXC refused it: msg (code X)", a Permission one names the fix.
HTTP 5xx and timeouts are Unknown ("the execution status is UNKNOWN and could have been a
success", GH-S "HTTP Return Codes"); HTTP 429 and 418 are RateLimited with `Retry-After` in
seconds. The futures page's campaign, grid-bot and Earn codes (8002 to 8202, 8900 to 8911, 8925
to 8929, 8933 to 8946, 9065 to 9067, 9501 to 9533, 9901 to 9918) concern no order call and are
not mapped.

#### Spot errors

| Code | Canonical | MEXC's words |
| --- | --- | --- |
| `-2011` | Rejected(not found) | Unknown order sent |
| `26` | Rejected(other) | operation not allowed |
| `400` | Auth | api key required |
| `401` | Auth | No authority |
| `403` | Permission | Access Denied |
| `429` | RateLimited | Too Many Requests |
| `500` | Unknown | Internal error |
| `503` | Unknown | service not available, please try again |
| `504` | Unknown | Gateway Time-out |
| `602` | Auth | Signature verification failed |
| `10001` | Auth | user does not exist |
| `10007` | InvalidRequest | bad symbol |
| `10072` | Auth | invalid access key |
| `10073` | Auth | invalid Request-Time |
| `10095` | InvalidRequest | amount cannot be null |
| `10096` | Rejected(venue_rule) | amount decimal places is too long |
| `10097` | Rejected(venue_rule) | amount is error |
| `10098` | Rejected(other) | risk control system detected abnormal |
| `10101` | Rejected(funds) | Insufficient balance |
| `10102` | InvalidRequest | amount cannot be zero or negative |
| `11444` | Unknown | This feature is temporarily unavailable. For details, please refer to our official announcement |
| `20002` | Unknown | Internal system error please contact support |
| `22222` | Rejected(not found) | record does not exist |
| `30000` | Rejected(market closed) | suspended transaction for the symbol |
| `30001` | Rejected(market closed) | The current transaction direction is not allowed to place an order |
| `30002` | Rejected(venue_rule) | The minimum transaction volume cannot be less than : |
| `30003` | Rejected(venue_rule) | The maximum transaction volume cannot be greater than : |
| `30004` | Rejected(funds) | Insufficient position |
| `30005` | Rejected(funds) | Oversold |
| `30010` | Rejected(price) | no valid trade price |
| `30014` | InvalidRequest | invalid symbol |
| `30016` | Rejected(market closed) | trading disabled |
| `30018` | Rejected(market closed) | market order is disabled |
| `30019` | Rejected(market closed) | api market order is disabled |
| `30020` | Permission | no permission for the symbol |
| `30021` | InvalidRequest | invalid symbol |
| `30025` | Rejected(price) | no exist opponent order |
| `30026` | Rejected(not found) | invalid order ids |
| `30027` | Rejected(market closed) | The currency has reached the maximum position limit, the buying is suspended |
| `30028` | Rejected(market closed) | The currency triggered the platform risk control, the selling is suspended |
| `30029` | Rejected(venue_rule) | Cannot exceed the maximum order limit |
| `30032` | Rejected(venue_rule) | Cannot exceed the maximum position |
| `30035` | Rejected(other) | You have triggered the order limit, please contact us by submitting a ticket. |
| `30041` | Rejected(venue_rule) | current order type can not place order |
| `33333` | InvalidRequest | param is error |
| `44444` | InvalidRequest | param cannot be null |
| `60005` | Permission | your account is abnormal |
| `70011` | Permission | Pair user ban trade apikey |
| `700001` | Auth | API-key format invalid |
| `700002` | Auth | Signature for this request is not valid |
| `700003` | Auth | Timestamp for this request is outside of the recvWindow |
| `700004` | InvalidRequest | Param 'origClientOrderId' or 'orderId' must be sent, but both were empty/null |
| `700005` | InvalidRequest | recvWindow must less than 60000 |
| `700006` | Auth | IP non white list |
| `700007` | Permission | No permission to access the endpoint |
| `700008` | InvalidRequest | Illegal characters found in parameter |
| `700013` | InvalidRequest | Invalid content Type |
| `730001` | InvalidRequest | Pair not found |
| `730002` | InvalidRequest | Your input param is invalid |

#### Futures errors

| Code | Canonical | MEXC's words |
| --- | --- | --- |
| `401` | Auth | Not logged in or login has expired |
| `402` | Auth | API Key expired, please apply again |
| `406` | Auth | Accessing IP is not in the whitelist |
| `500` | Unknown | Internal error. Please try again. |
| `501` | Unknown | System busy, try again later |
| `506` | Permission | Unknown source of request |
| `510` | RateLimited | Requests are too frequent, please try again later |
| `511` | Permission | You do not have access to the interface |
| `513` | Auth | Invalid request, please try again later |
| `600` | InvalidRequest | Parameter error |
| `601` | InvalidRequest | Data parsing error |
| `602` | Auth | Confirming signature failed |
| `603` | Unknown | Repeated request |
| `604` | Unknown | Sorry, this feature is under maintenance |
| `701` | Permission | Please enable API Key read access |
| `702` | Permission | Please enable API Key write access |
| `703` | Permission | Trading information read access is required |
| `704` | Permission | Please enable API Key trading information write access |
| `801` | Unknown | System under maintenance |
| `1000` | Auth | Account does not exist |
| `1001` | InvalidRequest | Contract does not exist |
| `1002` | Rejected(market closed) | Contract not activated |
| `1003` | Rejected(other) | Risk limit level error, please try again after adjustment |
| `1004` | InvalidRequest | Amount error |
| `2001` | InvalidRequest | Order direction error |
| `2002` | InvalidRequest | Position type error |
| `2003` | Rejected(price) | Exceeded the maximum buying price |
| `2004` | Rejected(price) | Selling price is lower than the minimum price |
| `2005` | Rejected(funds) | Balance insufficient |
| `2006` | Rejected(other) | Leverage multiplier must be within the upper limit |
| `2007` | Rejected(price) | Order price error |
| `2008` | Rejected(reduce only) | Insufficient number of positions can be closed |
| `2009` | Rejected(reduce only) | Position is nonexistent or closed |
| `2011` | Rejected(venue_rule) | Order quantity error |
| `2013` | InvalidRequest | Number of cancelled orders exceeds {count} |
| `2014` | InvalidRequest | Orders in batches shall not be greater than {count} |
| `2015` | Rejected(venue_rule) | Price or quantity precision error, please enter again |
| `2016` | Rejected(venue_rule) | Trigger order exceeds the max. number of {count} |
| `2018` | Rejected(funds) | Cannot exceed the maximum reducible margin {vol} |
| `2019` | Rejected(other) | Leverage adjustment unavailable while orders are open |
| `2021` | Rejected(other) | Order leverage is inconsistent with the existing position leverage |
| `2022` | InvalidRequest | Position type error |
| `2023` | Rejected(other) | Risk limit cannot be modified at the moment due to leverage |
| `2024` | Rejected(other) | There's order with leverage greater than the maximum leverage |
| `2025` | Rejected(other) | Your current position amount is greater than the allowable amount |
| `2026` | Rejected(other) | Leverage adjustment is unavailable for cross margin |
| `2027` | Rejected(other) | Cross and isolated position of the same direction are alternative. |
| `2028` | Rejected(venue_rule) | Maximum order quantity of {count} exceeded |
| `2029` | InvalidRequest | Order type error |
| `2030` | InvalidRequest | External order ID too long (max. 32 digit) |
| `2031` | Rejected(other) | Current risk limit and maximum allowable number of positions |
| `2032` | Rejected(price) | Order price is lower than the liquidation price of the position |
| `2033` | Rejected(price) | Order price is higher than the liquidation price of the position |
| `2034` | InvalidRequest | Batch query quantity exceeds the limit of {count} |
| `2035` | Rejected(venue_rule) | Market order level not supported |
| `2036` | Rejected(other) | You've reached today's order limit |
| `2037` | RateLimited | Trading too frequent, please try again later |
| `2038` | Rejected(other) | Position limit reached for your current leverage tier |
| `2039` | Rejected(other) | Counterparty's order amount shall not be greater than that of |
| `2040` | Rejected(not found) | Order does not exist |
| `2041` | Rejected(final) | Current order status does not allow cancelation |
| `2042` | Unknown | Duplicate order ID |
| `2043` | Rejected(other) | The order does not match the position |
| `2044` | Rejected(funds) | Insufficient margin to adjust leverage |
| `2045` | Rejected(other) | Minimum ratio allowed is {number} |
| `2046` | Rejected(other) | Maximum ratio allowed is {number} |
| `2047` | Rejected(other) | Minimum variance allowed is {number} |
| `2048` | Rejected(other) | Maximum variance allowed is {number} |
| `2049` | Rejected(price) | Liquidation may occur after the order is filled at the specified price |
| `2051` | Rejected(venue_rule) | Exceeds the maximum order amount allowed for a single order |
| `2052` | Rejected(other) | Partially filled orders do not support price or quantity editing |
| `2053` | Rejected(other) | One-way mode does not support limit order editing currently |
| `2054` | Rejected(other) | Unable to place a new order as the system has not found any successfully canceled orders |
| `2055` | Rejected(other) | System does not support limit order editing currently |
| `2056` | Rejected(other) | Market close order is in progress due to market volatility. |
| `2057` | Rejected(venue_rule) | Not meeting the minimum order amount required for a single order |
| `2058` | Rejected(other) | Chase Order Failed: The system does not currently support chase orders |
| `2059` | Rejected(other) | Chase Order Failed: Your order has been filled or canceled |
| `2060` | Rejected(other) | Chase Order Failed: TP price must be higher than the best bid price |
| `2061` | Rejected(other) | Chase Order Failed: SL price must be lower than the best bid price |
| `2062` | Rejected(other) | Chase Order Failed: TP price must be lower than the best ask price |
| `2063` | Rejected(other) | Chase Order Failed: SL price must be higher than the best ask price |
| `2064` | Rejected(other) | Unable to place chase orders in one-way position mode |
| `2065` | Rejected(funds) | Chase Order Failed: Insufficient assets |
| `2066` | Rejected(other) | Chase Order Failed: This action will result in liquidation |
| `2068` | Rejected(other) | MEXC has disabled certain SMS services |
| `2069` | Rejected(other) | Your current Cross Margin position is showing a loss. |
| `2070` | Rejected(price) | Price distance cannot exceed 5% of the current bid1/ask1 |
| `2071` | Rejected(other) | Max. chase distance cannot exceed 10% |
| `2072` | Rejected(other) | You can have up to {count} active chase limit orders per account |
| `2073` | Rejected(other) | A chase limit order already exists for this Futures pair |
| `2074` | Rejected(other) | Chase limit orders cannot be edited |
| `2075` | Rejected(other) | This is not a chase limit order |
| `2076` | Rejected(other) | This order is a chase limit order |
| `2077` | Rejected(price) | Long: Fill price is below the liquidation price. Use a limit order instead. |
| `2078` | Rejected(price) | Short: Fill price exceeds the liquidation price. Use a limit order instead. |
| `2079` | Rejected(other) | Loss Coverage does not exist |
| `2080` | Rejected(other) | Loss Coverage used |
| `2081` | Rejected(other) | Loss Coverage expired |
| `2082` | Rejected(other) | Loss Coverage unavailable |
| `3001` | InvalidRequest | Trigger order price type error |
| `3002` | InvalidRequest | Trigger order type error |
| `3003` | InvalidRequest | Execution cycle error |
| `3004` | Rejected(price) | Trigger price error |
| `3006` | Rejected(price) | Order prices can only be edited for orders that have not been triggered |
| `3007` | Rejected(price) | Take-profit price of a trigger-market long order should be higher |
| `3008` | Rejected(price) | Take-profit price of a trigger-limit long order should be higher |
| `3009` | Rejected(price) | Stop-loss price of a trigger-market long order should be lower |
| `3010` | Rejected(price) | Stop-loss price of a trigger-limit long order should be lower |
| `3011` | Rejected(price) | Take-profit price of a trigger-market short order should be lower |
| `3012` | Rejected(price) | Take-profit price of a trigger-limit short order should be lower |
| `3013` | Rejected(price) | Stop-loss price of a short position trigger-market order should be higher |
| `3014` | Rejected(price) | Stop-loss price of a short position trigger-limit order should be higher |
| `3015` | Rejected(price) | Pre-set TP/SL prices can only be edited for orders that have not been filled |
| `3016` | Unknown | {symbol} order timed out |
| `3017` | Unknown | {symbol} trigger order timed out |
| `3018` | Unknown | {symbol} trailing stop order timed out |
| `3019` | Unknown | {symbol} chase limit order timed out |
| `4001` | Rejected(other) | Unsupported cryptocurrency |
| `5001` | Rejected(other) | Stop profit price and stop loss price cannot both be empty |
| `5002` | Rejected(not found) | Stop-limit order is nonexistent or closed |
| `5003` | Rejected(price) | The price of stop-limit order error |
| `5004` | Rejected(other) | Total amount of the stop-limit order is greater than the closable amount |
| `5005` | Rejected(other) | Order failed as there is already a position TP/SL order. |
| `5006` | Rejected(other) | You currently have maker orders or positions in the opposite direction |
| `6001` | Rejected(market closed) | Trading is forbidden |
| `6002` | Rejected(market closed) | Position opening is forbidden. Please contact Customer Service |
| `6003` | InvalidRequest | Incorrect time range, you can select up to 90 days only |
| `6004` | InvalidRequest | The trading pair and status should be filled in |
| `6005` | Rejected(market closed) | The trading pair is not available |
| `6006` | Rejected(other) | The current version does not support Cross Margin function |
| `6007` | Rejected(other) | Switching from Cross Margin mode to Isolated Margin Mode is not supported |
| `6008` | Unknown | Order placed successfully (The estimated liquidation price may not show accurately on the current version |
| `6009` | Permission | Position opening is currently unavailable for your account. |
| `6010` | Rejected(other) | Multi-asset mode isn't available for your account |
| `6011` | Rejected(other) | Unable to switch to Multi-Asset mode as there are currently active orders |
| `6012` | Rejected(other) | Sub-accounts cannot use Multi-Asset mode |
| `6013` | Rejected(other) | Traders in Copy Trade cannot use Multi-Asset mode |
| `6014` | Rejected(other) | Your account has liabilities and cannot switch to Multi-Asset mode |
| `6015` | Rejected(other) | The account has liabilities and cannot switch to Single-Asset mode |
| `6016` | Rejected(other) | Unable to switch to Single-Asset mode as there are currently active orders |
| `6017` | Rejected(other) | The current price has expired. Please request the price again. |
| `6018` | Rejected(other) | The current exchange rate is unfavorable, and this action may |
| `6019` | Rejected(other) | Coin-margined trading is not supported in Multi-Asset mode |
| `6020` | Rejected(other) | Only cross margin is allowed in Multi-Asset mode |
| `6021` | Rejected(other) | There is an issue with the liability crypto. Please try again later. |
| `6022` | Rejected(other) | Trading error. Please try again later. |
| `6023` | Rejected(other) | Market maker accounts cannot use Multi-Asset mode |
| `6024` | Rejected(other) | Event Futures trading is not available in Multi-Asset mode |
| `6025` | Rejected(other) | Position airdrops are not available in Multi-Asset mode |
| `6026` | Permission | Position opening is unavailable until risk control verification |
| `6027` | Rejected(market closed) | Due to risk controls, opening new positions for this pair is temporarily |
| `6028` | Rejected(other) | After the platform's risk review, you are no longer allowed to |
| `6029` | Rejected(other) | Bonuses cannot be used to open Prediction Futures positions |
| `6030` | Permission | Position opening is unavailable until risk control verification |
| `7001` | Rejected(other) | Please cancel active orders or close positions before modifying position mode |
| `7002` | Rejected(other) | Position mode mismatch, please switch position mode |
| `7003` | Rejected(other) | The new order type must be consistent with the existing order |
| `7004` | Rejected(other) | The new order leverage must be consistent with the existing order |
| `7005` | Rejected(reduce only) | The position amount for closing is insufficient |
| `7008` | Rejected(venue_rule) | Cannot be less than the minimum order amount {value} USDT |
| `7009` | Rejected(other) | Your leverage ratio (position value/wallet balance) is too high |
| `8012` | Rejected(funds) | Insufficient available margin |
| `8815` | Rejected(other) | Current positions exceeded the maximum number of positions |
| `8816` | Rejected(other) | Exceeded the maximum number of positions {amount}{currency} that can be held |
| `8817` | Rejected(other) | Risk limiting mechanism has been upgraded. |
| `8818` | Rejected(other) | Current positions exceeded the maximum number of contracts |
| `8819` | Rejected(other) | Exceeded the maximum number of contracts {amount} that can be held |
| `8820` | Rejected(other) | Due to risk control measures, your account has reached the maximum |
| `8821` | Rejected(other) | The current position quantity exceeds the maximum position quantity |
| `8823` | Rejected(market closed) | This Futures pair will be delisted. New positions cannot be opened |
| `8912` | Rejected(other) | Automatically cancelled as position has been closed |
| `8913` | Rejected(other) | Manually cancelled by user |
| `8915` | Rejected(other) | Leverage mode cannot be changed as there is an active order |
| `8917` | Rejected(reduce only) | The reduce-only order would lead to insufficient position margin |
| `8918` | Rejected(other) | Queries for market makers or particular users are not currently supported |
| `8919` | Rejected(other) | Exceeds the maximum value limit for position and open order value |
| `8920` | Rejected(market closed) | Due to market risk controls, only closing positions is allowed for this pair |
| `8921` | Rejected(market closed) | Due to market risk controls, only opening positions is allowed for this pair |
| `8922` | Rejected(market closed) | Trading for this pair is suspended. Wait for trading to resume. |
| `8930` | Rejected(market closed) | Not yet listed, unable to place an order |
| `8931` | Rejected(other) | Your leverage exceeds the maximum limit of {max}x in your country |
| `8932` | Rejected(market closed) | Due to risk controls, opening new positions for this pair is temporarily |
| `8934` | Rejected(market closed) | Position opening is prohibited |
| `8948` | Rejected(market closed) | Trading for this pair is about to close for the selected time period |
| `8949` | Rejected(market closed) | This trading pair is outside trading hours. Placing orders is disabled. |
| `8950` | Rejected(market closed) | For risk management reasons, opening positions for this trading pair |
| `9068` | Rejected(market closed) | Trading unavailable due to market closure. Only order cancellation |
| `9069` | Rejected(other) | This trading pair only supports TP/SL triggered by fair price. |
| `9070` | Rejected(other) | This pair supports Isolated Margin mode only |
| `9071` | Rejected(other) | This pair supports Cross Margin mode only |
| `9073` | Rejected(market closed) | Stock Futures trading is suspended during market closure. |
| `9074` | Rejected(other) | Guaranteed Stop-Loss is currently unavailable |
| `9075` | Rejected(other) | Stop-Loss trigger price can only be set to the last price |
| `9998` | Rejected(market closed) | Only position closing is supported, as the platform does not support |
| `9999` | Unknown | Network error. Please try again. |
| `11444` | Unknown | This feature is temporarily unavailable. |
| `200005` | Permission | To further enhance the security of user accounts and assets |
| `200006` | Permission | Please complete KYC verification to access platform services |
| `300000` | Permission | Please complete KYC verification to access platform services |
| `300001` | Permission | To further enhance the security of user accounts and assets |
| `300002` | Permission | Due to the relevant regulations, this service is not supported in your region |

### Doc gaps

- Futures one-way orders: which `side` code a one-way order takes, and how `reduceOnly`
  combines with it (FT/place-order states neither; the current-orders example shows `side` 1
  on a `positionMode` 2 row, an example, not a rule). One-way accounts are refused.
- Futures market `price`: marked mandatory on every type, its meaning on a market order not
  stated. OTW sends none on a market order (type 5); the first live order says whether the
  venue takes that.
- Futures cancel body: `orderIds` `List<Long>` (live) vs a bare list (GH-C); the bare list is
  sent.
- Whether `contract.mexc.com` still answers REST: not stated; every call goes to
  `api.mexc.com`.
- `position/leverage` for a flat contract: whether it answers rows is not stated.
- `maxVol` vs `limitMaxVol` for a market order (read as above); the minimum order value
  (7008, 2057) is not in the contract list.
- `order/cancel_all`: whether it reaches plan, TP/SL and trailing orders.
- `externalOid` charset and uniqueness window; `newClientOrderId` length, charset,
  uniqueness and duplicate code (a duplicate lands unmapped: Unknown, which is safe).
- Retention of `order/get`, `order/external` (futures) lookups; spot `GET /api/v3/order` is
  "up to 7 days".
- REST fill fee sign (both books); the `order_deals/v3` span cap (90 days on the former path).
- Spot tick and lot fields: the precisions are "Int" with no further word; `baseSizePrecision`
  is called "min order quantity". OTW reads the lot as 10^-`baseAssetPrecision` and the minimum
  as `baseSizePrecision`.
- Spot `status` word (`1` vs `ENABLED`); the market-order bounds `quoteAmountPrecisionMarket`
  and `maxQuoteAmountMarket` are not held (one bound per rule set): the venue checks them.
- Spot market buy by `quantity`: not described (only a sell); OTW sends `quoteOrderQty`.
- Spot stream order id vs the REST `orderId` (`C02__...` in the examples): read again by id.
  A frame with no `id` (field 1) is read again by its `clientId` (field 2); a frame with
  neither is not read, and the whole book is read again.
- Futures market order `price`: "mandatory", meaning not stated (none sent, above).
- The trailing list's `states` (`List<int>`): the GET form of several values is not stated.
- Spot `myTrades` row order and paging past 100 rows.
- Error bodies: the pages list codes, not bodies; the envelopes above are the documented ones.

### What stays unproven

Real fills, partial fills and their event order; real error bodies; ack timing; whether the
listed endpoints answer a real key as documented. No sandbox: the first orders are live.
