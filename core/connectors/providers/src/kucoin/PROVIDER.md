# KuCoin (spot and futures)

Adapter version: 0.1.0. Provider API version: v1 (v2 for the spot symbol list). Maturity: M2
(mock catalog green; history, instrument lists, quotes and both sockets checked against the
live venue on 2026-10-19; account parity on mock only, no key lent).

Two adapters, two OTW providers: `Kucoin::spot()` (`kucoin`, venue `KUCOIN`) and
`Kucoin::futures()` (`kucoin_futures`, venue `KUCOIN_FUTURES`). Spot and futures are separate
platforms on separate hosts with their own symbols (`BTC-USDT`, `XBTUSDTM`), so nothing is
inferred from a ticker. Futures are the USDT- and USDC-margined contracts; coin-margined
(inverse, `XBTUSDM`) contracts are left out of the list and refused by name, since their
volume is counted in USD and their profit in the coin.

Sources:
- KuCoin universal SDK specification, `spec/original/meta.json`:
  https://github.com/Kucoin/kucoin-universal-sdk (endpoint parameters, limits, examples)
- Signing: https://www.kucoin.com/docs-new/authentication and the SDK's
  `sdk/python/kucoin_universal_sdk/internal/infra/default_signer.py` / `default_transport.py`
- Spot candles: https://www.kucoin.com/docs-new/rest/spot-trading/market-data/get-klines
- Futures candles: https://www.kucoin.com/docs-new/rest/futures-trading/market-data/get-klines
- Measured answers of the public endpoints, 2026-10-19 (recorded in `tests.rs`).

## Endpoints

| Endpoint id | Spot (`api.kucoin.com`) | Futures (`api-futures.kucoin.com`) | Canonical |
| --- | --- | --- | --- |
| `candles` / `kline` | `GET /api/v1/market/candles?type&symbol&startAt&endAt` (s) | `GET /api/v1/kline/query?symbol&granularity&from&to` (ms) | `RequestHistory{Candles}` |
| `contract` | | `GET /api/v1/contracts/{symbol}` | contract size before a candle or quote call |
| `symbols` / `contracts` | `GET /api/v2/symbols` | `GET /api/v1/contracts/active` | `LoadInstruments`; futures `OpenOrders` and `Positions` start with it (lots read in the coin) |
| `level1` / `ticker` | `GET /api/v1/market/orderbook/level1?symbol` | `GET /api/v1/ticker?symbol` | `RequestSnapshot{Quote}` |
| `allTickers` | `GET /api/v1/market/allTickers` | | `RequestBoard` (`last`) |
| `bullet` | `POST /api/v1/bullet-public` | `POST /api/v1/bullet-public` | socket address and token (`socket_bootstrap`) |
| `apiKey` | `GET /api/v1/user/api-key` (signed) | | `Summary`: permissions, can trade |
| `accounts` | `GET /api/v1/accounts` (signed) | | `Balances` (`main` + `trade` summed per coin) |
| `activeSymbols` / `activeOrders` | `GET /api/v1/hf/orders/active/symbols`, then `GET /api/v1/hf/orders/active?symbol` per symbol | | `OpenOrders` |
| `hfFills` | `GET /api/v1/hf/fills?symbol&startAt&endAt&limit=100[&lastId]` (signed) | | `Fills`, per named pair |
| `accountOverview` | | `GET /api/v1/account-overview?currency=USDT` (signed) | `Summary` |
| `positions` | | `GET /api/v1/positions?currency=USDT`, then `USDC` | `Positions`, in the coin |
| `orders` / `stopOrders` | | `GET /api/v1/orders?status=active&currentPage&pageSize=1000`, then `GET /api/v1/stopOrders` | `OpenOrders`, in the coin |
| `fills` | | `GET /api/v1/fills?startAt&endAt&currentPage&pageSize=1000` | `Fills` |

Socket: the bullet answer names `instanceServers[0].endpoint` and a `token`; the socket opens
at `endpoint?token=...`, says `welcome`, and takes
`{"id","type":"subscribe","topic","privateChannel":false,"response":true}`. Topics: spot
`/market/candles:<symbol>_<interval>`, futures `/contractMarket/limitCandle:<symbol>_<interval>`.

## Field mapping and rules

- Interval tokens: `1min 3min 5min 15min 30min 1hour 2hour 4hour 6hour 8hour 12hour 1day 1week`
  (spot history and both sockets); futures history takes minutes
  (`1 5 15 30 60 120 240 480 720 1440 10080`).
- Candle columns (measured): spot `[time s, open, close, high, low, volume, turnover]` on the
  archive and the socket; futures archive `[time ms, open, high, low, close, volume,
  turnover]` as JSON numbers; futures socket `[time s, open, close, high, low, turnover,
  volume]`. Futures volume is in contracts and is multiplied by the contract size
  (`multiplier`, e.g. 0.001 XBT) into the coin.
- Paging (measured): spot answers the newest 1500 bars of `[startAt, endAt)`, walked
  backwards with `endAt` = the oldest bar's open. Futures answers the oldest 200 of
  `[from, to]`, walked forwards from the newest bar plus one period.
- Daily bars open at midnight UTC on both. Spot weeks open on **Thursday** (measured
  `1792022400` = Thu 2026-10-15), so spot does not declare `1w`: the engine folds days into
  Monday weeks. Futures weeks open on Monday (measured) and are native.
- No archive row carries a closed flag. Measured 2026-10-19 over four minute boundaries: no
  spot bar changed after its close; futures bars were rewritten up to 12.3 s after. A bar is
  final once its period plus 10 s (spot) or 60 s (futures) has passed.
- The socket carries no closed flag either: every candle goes out forming and the engine
  closes it when the next period starts (`candles_final_flag = false`). Spot event time is in
  nanoseconds, futures in milliseconds.
- The socket acknowledges a symbol it does not list and then stays silent (measured), so a
  live session loads the instrument list and refuses an unlisted pane by name. A ping
  (`{"id","type":"ping"}`) goes out 5 s before the bullet's `pingInterval` (18 s).
- Futures quotes: `bestBidSize` / `bestAskSize` are contracts, multiplied into coins.
- Account rows: spot quantities are in the coin. Futures orders (working and stop) and
  positions are read in the base coin, the unit the order path and the stream use: each such
  read first takes `GET /api/v1/contracts/active` and multiplies the lots by the contract's
  `multiplier` (a position then counts one coin per unit); a row whose contract is not in that
  list is an error naming it, never a guess. Futures fills stay in contracts, the instrument
  list's `multiplier` being their point value. Fees as written, positive for a cost.
  Futures rows settled outside USDT/USDC (inverse) are not read. Spot balances sum the
  funding (`main`) and trading (`trade`) accounts; a margin account is not read.
- Spot fills: the endpoint requires `symbol`, so pairs are named (`fills_need_symbols`) and
  checked against the list before any signed call. Both fill endpoints answer at most 7 × 24
  hours per call: slices `[to − 7 d + 1 ms, to]` walked backwards without overlap. Spot pages
  by `lastId` (100 per page), futures by `currentPage` (1000 per page). 500 pages per pull.
- How far back KuCoin keeps fills is not documented for these endpoints: no limit is
  claimed.

## Auth

`KC-API-SIGN` = base64 HMAC-SHA256 of `timestamp(ms) + METHOD + path?query + body`, the query
written unencoded; `KC-API-PASSPHRASE` = base64 HMAC-SHA256 of the passphrase under the
secret; `KC-API-KEY`, `KC-API-TIMESTAMP`, `KC-API-KEY-VERSION: 3` (what KuCoin's SDKs send).
Signature and passphrase checked against references computed with Python's `hmac`. Secrets:
`api_key`, `api_secret`, `api_passphrase`. Host clock. The key test reads the key's
permissions: `General` reads, `Spot`, `Margin` or `Futures` could trade, which the test says.

## Rate limits

Public pool: 2000 weight per 30 s per IP (`gw-ratelimit-limit: 2000` measured); a candle
page weighs 3. Spot private pool 4000 per 30 s (fill page weight 2), futures pool 2000 per
30 s (fill page weight 5, account overview 5).

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `200000` | success | none |
| `429000`, HTTP 429 | rate limit | RateLimited |
| `400001` to `400006`, `411100` | header missing, timestamp, key, passphrase, signature, IP, frozen | Auth |
| `400007` | permission | Permission |
| `900001`, `404000`, `200003` | pair or contract does not exist, invalid symbol | NotFound (naming the symbol) |
| `400100` on a market-data call | parameter error (`Unsupported trading pair.` for a bad pair, measured) | NotFound naming the symbol |
| `500000`, `503000` | internal, busy | Transient |
| anything else | | InvalidRequest |

The spot best bid/ask answers `data: null` for a pair it does not list (measured): NotFound.

## Not covered

- Unified Trading Account (`/api/ua/...`) endpoints: the classic account endpoints above.
- Coin-margined futures, margin accounts, options.
- Orders: see [Trading](#trading), "Left out".

## Regions and terms

KuCoin is not licensed in some jurisdictions (the United States among them); public market
data answered from the test machine.

## Live best bid and ask

Spot `/market/ticker:<symbol>` (`bestBid`, `bestAsk`, sizes, `time` ms), futures
`/contractMarket/tickerV2:<symbol>` (`bestBidPrice`, `bestAskPrice`, sizes in contracts, `ts`
ns), on the candle socket (measured 2026-10-19).

## Trading

Execution maturity: **E2D** on both platforms, built from the documentation alone: no
account, no key, no live test. Sandbox: **none** (no sandbox, testnet or demo host appears in
the doc index, [llms.txt](https://www.kucoin.com/docs-new/llms.txt)), so every account is live
and takes orders only once its owner acknowledged that the broker was never tested against the
venue; KuCoin's order test endpoint then runs before the first order of each session. Code:
`trade.rs` (mapper, both books), `trade_errors.rs` (one error table per platform); the order
path is the shared trader's (`connectors/engine/src/trading.rs`); otw-core wiring in
`otw-core/src/brokers/native_trade_kucoin.rs` (brokers `kucoin`, book `spot`, and
`kucoin_futures`, book `futures`; orders followed as `BOOK:SYMBOL:orderId`).

Doc snapshot (read 2026-11-11, `D` = `https://www.kucoin.com/docs-new/`, each page also served
as Markdown at its URL plus `.md`; the newest changelog entry then was 2026.11.06):

| Page | Used for |
| --- | --- |
| [Spot Add Order](https://www.kucoin.com/docs-new/rest/spot-trading/orders/add-order) | HF place body, `clientOid` rule, limit and market parameters, `timeInForce`, `postOnly`, `size` or `funds`, price protection |
| [Spot Add Order Test](https://www.kucoin.com/docs-new/rest/spot-trading/orders/add-order-test) | dry run |
| [Spot Cancel Order By OrderId](https://www.kucoin.com/docs-new/rest/spot-trading/orders/cancel-order-by-orderld) | cancel, "only sends cancellation requests" |
| [Spot Cancel All Orders By Symbol](https://www.kucoin.com/docs-new/rest/spot-trading/orders/cancel-all-orders-by-symbol) | cancel all per symbol, `data: "success"` |
| [Spot Get Order By ClientOid](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-order-by-clientoid), [By OrderId](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-order-by-orderld) | lookup, `active`, `cancelExist`, `dealSize`, `cancelledSize`, `cancelledFunds`, "Canceled or fully filled orders are marked as completed Done status", retention (cancelled 2 days, filled 7 days) |
| [Spot Get Symbols With Open Order](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-symbols-with-open-order), [Get Open Orders By Page](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-open-orders-by-page) | working orders (symbol required), `pageSize` "maximum: 50" (50 sent) |
| [Spot Get Stop Orders List](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-stop-orders-list), [Cancel Stop Order By OrderId](https://www.kucoin.com/docs-new/rest/spot-trading/orders/cancel-stop-order-by-orderld), [Batch Cancel Stop Orders](https://www.kucoin.com/docs-new/rest/spot-trading/orders/batch-cancel-stop-orders) | spot stop orders placed elsewhere: listed (`status` NEW or TRIGGERED, `stopPrice`, `currentPage`/`totalPage`), cancelled by id, and per symbol with cancel all (`cancelledOrderIds`) |
| [Spot Get Trade History](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-trade-history) | fills, `tradeId`, `lastId` paging, `limit` max 100, 7 days |
| [Spot Get All Symbols](https://www.kucoin.com/docs-new/rest/spot-trading/market-data/get-all-symbols) | rules |
| [Futures Add Order](https://www.kucoin.com/docs-new/rest/futures-trading/orders/add-order) | place body, `clientOid` (required), `qty`, `stop`, `stopPrice`, `stopPriceType`, `reduceOnly`, `closeOrder`, `marginMode`, `positionSide`, open order caps |
| [Futures Add Order Test](https://www.kucoin.com/docs-new/rest/futures-trading/orders/add-order-test) | dry run |
| [Futures Cancel Order By OrderId](https://www.kucoin.com/docs-new/rest/futures-trading/orders/cancel-order-by-orderld) | cancel, "including a stop order" |
| [Futures Cancel All Orders](https://www.kucoin.com/docs-new/rest/futures-trading/orders/cancel-all-orders), [Cancel All Stop orders](https://www.kucoin.com/docs-new/rest/futures-trading/orders/cancel-all-stop-orders) | cancel all per symbol, "excluding stop orders" and the stops apart |
| [Futures Get Order By ClientOid](https://www.kucoin.com/docs-new/rest/futures-trading/get-stop-order-by-clientoid), [By OrderId](https://www.kucoin.com/docs-new/rest/futures-trading/orders/get-order-by-orderld) | lookup "including a stop order", retention (by client id 7 days, by id 3 months) |
| [Futures Get Order List](https://www.kucoin.com/docs-new/rest/futures-trading/orders/get-order-list), [Get Stop Order List](https://www.kucoin.com/docs-new/rest/futures-trading/orders/get-stop-order-list) | working orders and untriggered stops, `currentPage`/`pageSize` max 1000 |
| [Futures Get Trade History](https://www.kucoin.com/docs-new/rest/futures-trading/orders/get-trade-history) | fills, `tradeType`, `tradeTime` ns, 7 × 24 h per call |
| [Futures Get All Symbols](https://www.kucoin.com/docs-new/rest/futures-trading/market-data/get-all-symbols), [Get Symbol](https://www.kucoin.com/docs-new/rest/futures-trading/market-data/get-symbol) | rules, `multiplier`, `status` |
| [Get Position Mode](https://www.kucoin.com/docs-new/rest/futures-trading/positions/get-position-mode), [Switch Position Mode](https://www.kucoin.com/docs-new/rest/futures-trading/positions/switch-position-mode) | account-wide mode (OTW never switches it) |
| [Get Margin Mode](https://www.kucoin.com/docs-new/rest/futures-trading/positions/get-margin-mode) | per symbol margin mode |
| [Get Position Details](https://www.kucoin.com/docs-new/rest/futures-trading/positions/get-position-details), [Get Position List](https://www.kucoin.com/docs-new/rest/futures-trading/positions/get-position-list) | positions, `positionSide`, `marginMode`, `leverage` |
| [Get Private Token (Spot)](https://www.kucoin.com/docs-new/websocket-api/base-info/get-private-token-spot-margin), [(Futures)](https://www.kucoin.com/docs-new/websocket-api/base-info/get-private-token-futures), [WebSocket introduction](https://www.kucoin.com/docs-new/websocket-api/base-info/introduction) | private socket token, URL, ping, subscribe and ack |
| [Order V2](https://www.kucoin.com/docs-new/3470073w0), [Futures Orders](https://www.kucoin.com/docs-new/3470090w0), [Futures Stop Orders](https://www.kucoin.com/docs-new/3470091w0), [Futures Positions](https://www.kucoin.com/docs-new/3470093w0) | private stream frames |
| [Enums](https://www.kucoin.com/docs-new/enums-definitions) | time in force, "Market orders are not supported by the Time in Force strategy" |
| [Spot errors](https://www.kucoin.com/docs-new/error-code/spot), [Futures errors](https://www.kucoin.com/docs-new/error-code/futures), [HTTP errors](https://www.kucoin.com/docs-new/error-code/http), [WebSocket and gateway errors](https://www.kucoin.com/docs-new/error-code/websocket) | error tables |
| [Rate Limit Rule (Classic)](https://www.kucoin.com/docs-new/rate-limit-rule-classic) | pools, `gw-ratelimit-reset`, load-balancing disconnects |
| [Authentication](https://www.kucoin.com/docs-new/authentication) | signature (see [Auth](#auth)) |
| [Introduction (UTA)](https://www.kucoin.com/docs-new/introduction-uta), [Changelog](https://www.kucoin.com/docs-new/change-log) | hosts, classic API in "maintenance mode" for classic accounts |

### Order types and flags

| Book | Canonical | Native | Notes |
| --- | --- | --- | --- |
| spot | `market` | `type=market`, `size` (base coin) | "select one out of two: size or funds": OTW always sends `size`, so no quote-sized conversion; no `timeInForce` sent |
| spot | `limit` | `type=limit`, `price`, `size`, `timeInForce` | |
| spot | post-only | `postOnly=true` | "disabled when the order timing strategy is IOC or FOK": a post-only IOC or FOK is refused before I/O |
| futures | `market` | `type=market` | no `timeInForce` ("Optional for type is 'limit' order") |
| futures | `limit` | `type=limit`, `price`, `timeInForce` | |
| futures | `stop` | `type=market`, `stop`, `stopPrice`, `stopPriceType=TP` | `stop=up` for a buy ("reaches or goes above"), `down` for a sell ("reaches or goes below"); the trade price (`TP`) as trigger |
| futures | `stop_limit` | `type=limit`, `price`, `stop`, `stopPrice`, `stopPriceType=TP` | |
| futures | reduce-only | `reduceOnly=true` | "If the order would result in an increased position, it will be rejected" |
| futures | post-only | `postOnly=true` | "invalid when timeInForce is IOC": refused with IOC or FOK before I/O |
| futures | size | `qty` (base coin) | "must be an integer multiple of the multiplier": refused before I/O when not a whole number of contracts, naming the two nearest |
| futures | position | `positionSide` `BOTH`, `LONG`, `SHORT` | "Required in hedge mode"; from the account's position mode, read with Get Position Mode (account wide, answered with no position open), held, never switched; read again after `330011` or a parameter error (`300000`, `100001`) on an order |
| futures | margin | `marginMode` `CROSS` / `ISOLATED` | the default is `ISOLATED`, so the symbol's own mode is always sent: read with Get Margin Mode once per symbol (or from a position row), held, never switched; read again after `330005` or a parameter error; an order whose margin was not read is refused |
| futures | leverage | not sent | "Optional for type is 'isolated margin' order": the symbol's leverage stands; read only, from the position rows, shown on the chart |

Close position (futures): the shared trader's reduce-only market order for the exact size read
from Get Position Details now, on the other side, whole or partial. `closeOrder` (the venue
picks side and size, whole position only) is not used, and the `close_position` capability
(close on a stop's trigger) is not declared: the doc does not say a stop takes `closeOrder`.
Spot has no position: a sale of what is held is a plain sell.

Futures stops share the order id space: cancel and lookup by id "including a stop order", so
no `cond:` tag; listings read Get Order List (`status=active`) and Get Stop Order List
("un-triggered stop orders"; "Stop orders that have been triggered can be queried through the
general order interface"). The account read (`account.rs`) lists both too, so a stop placed from
OTW shows in the orders panel and cancels from it.

Spot stop orders keep an endpoint of their own: the spot account read and the whole-book
listing of the order path both list them after the working orders of every symbol
([Get Stop Orders List](https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-stop-orders-list),
`GET /api/v1/stop-order`, "your current untriggered stop order list", `currentPage`,
`pageSize` maximum 500, weight 8; the order path sends `tradeType=TRADE`, "Default is TRADE"),
each as `cond:<id>`, working, waiting for its trigger while `NEW`. A cancel from the orders
panel goes to [Cancel Stop Order By OrderId](https://www.kucoin.com/docs-new/rest/spot-trading/orders/cancel-stop-order-by-orderld)
(`DELETE /api/v1/stop-order/{orderId}`, weight 3); cancel all sends, per symbol, Cancel All
Orders By Symbol and [Batch Cancel Stop Orders](https://www.kucoin.com/docs-new/rest/spot-trading/orders/batch-cancel-stop-orders)
(`DELETE /api/v1/stop-order/cancel?symbol=&tradeType=TRADE`, weight 3, answering
`cancelledOrderIds`), then cancels what is still listed one by one. A lookup by id of a spot
stop order is not made. Its `stop` filter ("stop: stop loss order, oco: oco order") is not
sent: which kinds the list answers without it is not stated. Placing a spot stop from
OpenTraderWorld is not declared.

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| `gtc` | `GTC` | default |
| `ioc` | `IOC` | |
| `fok` | `FOK` | limit only: a market or stop order FOK is refused before I/O ("Market orders are not supported by the Time in Force strategy") |

Spot `GTT` (`cancelAfter` seconds) is not offered: the mapper has no clock to turn an expiry
moment into seconds from now. Futures `RPI` is "only specific market maker partners". No day
order on either book.

### Client order id

`clientOid`: spot "can only contain numbers, letters, underscores (_), and hyphens (-)", "The
maximum length for the ClientOid is 40 characters" ([Spot Add Order](https://www.kucoin.com/docs-new/rest/spot-trading/orders/add-order));
futures "The maximum length cannot exceed 40, e.g. UUID only allows numbers, characters,
underline(_), and separator (-)" ([Futures Add Order](https://www.kucoin.com/docs-new/rest/futures-trading/orders/add-order));
optional on spot, required on futures. The OTW key is sent unchanged; one outside that rule is
refused before I/O. Looked up by `GET /api/v1/hf/orders/client-order/{clientOid}?symbol=`
(spot) and `GET /api/v1/orders/byClientOid?clientOid=` (futures). A futures duplicate answers
`300018` "clientOid parameter repeated" (looked up, never resent). The dry run never carries
the live order's id: spot sends none, futures (where it is required) `otw-check-<n>`.

### Order status

KuCoin's REST answers carry no status word, so the mapper writes its own, and only these map:

| Native | Mapper word | Canonical |
| --- | --- | --- |
| spot `active=true`, `dealSize=0`; futures `status=open`, nothing filled; stream `new`/`open`/`match` | `open` | New |
| the same with part filled | `partial` | PartiallyFilled |
| spot `active=false`, futures `status=done`, all filled (`dealSize >= size`); stream `filled` (unless its own `filledSize` is short of the order), or any status all filled | `filled` | Filled |
| done and not all filled ("partially filled orders go to done"; a cancel, an IOC rest, price protection): spot `active=false`, futures `status=done`; done with `cancelExist=true`, `cancelledSize` or `cancelledFunds`; stream `canceled`, `done` + `update` ("the unfilled part got canceled"), or `filled` with `filledSize` below the order | `canceled` | Cancelled, with what filled |
| spot stop list row `NEW` / `TRIGGERED` | `open` (`NEW`: `trigger_pending`) | New |
| futures stop frame `open` / `triggered` / `cancel` | `open` (untriggered: `trigger_pending`) / `open` / `canceled` | New / New / Cancelled |
| a spot done order sized in funds with no cancellation of any kind, no `active` flag, any other word | (verbatim) | Unknown, alerts |

An untriggered futures stop (`stop` set, `stopTriggered=false`) is New with `trigger_pending`.
The futures `open` frame's example carries no `orderType`: "open" is "the order is in the
order book", where only a limit rests, so such a frame with a price is read as a limit at that
price. The futures stop frame's example names neither `reduceOnly` nor `positionSide`: read
when a frame carries them, else not reduce-only and net, as the account read shows the row.

### Instrument trading rules

Read once a day and after a rule refusal, the whole list in one call ([Spot Get All Symbols](https://www.kucoin.com/docs-new/rest/spot-trading/market-data/get-all-symbols)
`GET /api/v2/symbols`, [Futures Get All Symbols](https://www.kucoin.com/docs-new/rest/futures-trading/market-data/get-all-symbols)
`GET /api/v1/contracts/active`, from which every contract's size is also read); a contract
missing from the active list is asked by name (`GET /api/v1/contracts/{symbol}`), so a listed
one not trading is refused as such.

| Book | Tick | Size step | Min size | Max size (limit / market) | Min value | Status |
| --- | --- | --- | --- | --- | --- | --- |
| spot | `priceIncrement` | `baseIncrement` | `baseMinSize` | `baseMaxSize` | `minFunds` | `enableTrading=false` closed; `callauctionIsEnabled=true` closed (the phases are not timed here) |
| futures | `tickSize` (max `maxPrice`) | one contract (`multiplier`) | `lotSize` contracts | `maxOrderQty` / `marketMaxOrderQty` contracts | none stated | `Open` trading; `Init`, `BeingSettled`, `Settled`, `Paused`, `Closed`, `CancelOnly` closed |

Futures sizes are stated in contracts ("The basic unit of the contract API is lots"); OTW
states them in the base coin through `multiplier`, and reads every answer's lots back the same
way (a contract whose size is not known is an error, never a guess). Coin-margined contracts
(`isInverse`, or a settle coin other than USDT and USDC) are left out of every list. Off-grid
values are refused before I/O, never rounded ("must be a multiple of priceIncrement.
Otherwise, the order will return an invalid priceIncrement error").

### Errors

`trade_errors.rs`, one table per platform (the same code means different things on spot and
futures: `200001`, `200002`, `200003`). A test asserts each row maps as listed, that an
unlisted code is Unknown and alerts, and that these tables match the code. Every refusal
reads "KuCoin refused it: <msg> (code <code>)"; an unlisted code "KuCoin answered code X (msg),
which OpenTraderWorld does not map".

| Answer | Canonical | Action |
| --- | --- | --- |
| timeout, closed socket, HTTP 5xx without a code | Unknown | looked up by `clientOid`, never resent |
| HTTP 403 without a code ("Forbidden or Too Many Requests") | RateLimited | refused, held by `gw-ratelimit-reset` when given |
| `400100` whose message names `account.available.amount` / insufficient balance | Rejected(funds) | refused |
| `400100` on a cancel by id | Rejected(not found) | the order looked up, its real state answered (the doc names no code for an unknown or ended order) |
| `200002` (futures) whose message names Level 3 | InvalidRequest | refused |
| rate limits | RateLimited | wait from `gw-ratelimit-reset` (ms), else `Retry-After`, else `1015` 30 s, `200002` 10 s |

#### Spot and gateway codes

| Code | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| `400001`, `400002`, `400003`, `400004`, `400005`, `400006`, `400008`, `400009`, `400011`, `400012` | header, timestamp, key, passphrase, signature, IP, key version, session (gateway) | Auth | refused, the fix named |
| `400201`, `400202`, `400400`, `400401` | partner sign and IP, authorization token (gateway) | Auth | refused |
| `400007`, `400010` | permission (gateway) | Permission | refused |
| `400301`, `400302`, `400303`, `400304` | region, identity verification (gateway) | Permission | refused |
| `411100`, `411200` | user frozen, URL blacklisted (gateway) | Permission | refused |
| `400101`, `400102`, `404000`, `415000` | invalid data or parameters, URL not found, content type (gateway) | InvalidRequest | refused |
| `429000`, `429001`, `429002` | UID, system, per-connection limits (gateway) | RateLimited | refused, new orders held |
| `500000`, `501000`, `503000`, `504000`, `505000` | internal, stopped, busy, timeout, unknown (gateway) | Unknown | looked up, never resent |
| `400100`, `102411`, `900001` | parameter error, symbol does not exist | InvalidRequest | refused |
| `102421`, `200004` | insufficient balance | Rejected(funds) | refused |
| `102426`, `102428`, `230005` | duplicate client order id, order being processed, system busy | Unknown | looked up, never resent |
| `102427`, `200003`, `400800` | pending orders over the threshold, orders limit, leverage order failed | Rejected(other) | refused |
| `102429` | price below the protection price | Rejected(price) | refused |
| `102434`, `102435` | above the maximum, below the minimum size | Rejected(venue_rule) | rules read again |
| `200001`, `200002`, `400600`, `600203` | order creation or cancel suspended, not started, not enabled | Rejected(market closed) | refused |
| `200009`, `400200`, `400500`, `400700` | KYC, forbidden to place, region, risk restriction | Permission | refused |
| `116241` | positions in associated accounts at the system risk limit, close or reduce only ([Changelog](https://www.kucoin.com/docs-new/change-log) 2026.09.21, platform not named: mapped on both) | Rejected(other) | refused |
| `110188` | too many requests, the system is busy (Changelog 2026.09.04, platform not named: mapped on both) | RateLimited | refused, new orders held |

#### Futures codes

The gateway codes above apply as listed; the futures page's own:

| Code | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| `400100`, `100001`, `100003`, `200001`, `200003`, `300000` | parameter errors, Level 2 scope, symbol invalid | InvalidRequest | refused; `300000` and `100001` read the position and margin modes again |
| `330005`, `330011` | switch to cross margin, switch and confirm the position mode | InvalidRequest | refused; the margin (`330005`) or position (`330011`) mode read again |
| `1015`, `200002` | IP limit (30 s), business frequency limit (10 s) | RateLimited | refused, new orders held |
| `40010`, `400200` | region restricted, unknown partner | Permission | refused |
| `100002`, `300018` | systemConfigError, clientOid repeated | Unknown | looked up, never resent |
| `100004` | order is in not cancelable state | Rejected(final) | the order looked up, its final state answered |
| `200005`, `300003` | insufficient balance | Rejected(funds) | refused |
| `300006`, `300007`, `300011`, `300012` | price against bankruptcy or liquidation, above or below the allowed range | Rejected(price) | refused |
| `300009` | position size 0, unable to close | Rejected(reduce only) | refused |
| `300002`, `300014`, `300015` | placement suspended, liquidation, settlement in progress | Rejected(market closed) | refused |
| `100005`, `300001`, `300004`, `300005`, `300008`, `300010`, `300013`, `300016`, `300017` | risk limit, order counts, no contra order, close failed, leverage, Futures Brawl | Rejected(other) | refused |
| `116241` | positions in associated accounts at the system risk limit, close or reduce only ([Changelog](https://www.kucoin.com/docs-new/change-log) 2026.09.21, platform not named: mapped on both) | Rejected(other) | refused |
| `110188` | too many requests, the system is busy (Changelog 2026.09.04, platform not named: mapped on both) | RateLimited | refused, new orders held |

### Rate limits for orders

Private pools per UID, refreshed each second against a 30 s quota: spot 4000/30 s, futures
2000/30 s at VIP0 ([Rate Limit Rule](https://www.kucoin.com/docs-new/rate-limit-rule-classic));
weights: spot place 1, cancel 1, cancel all by symbol 2; futures place 2, cancel 1, cancel all
10, cancel all stops 15, lookup 5. `429000` with `gw-ratelimit-reset` (ms to the reset) refuses
the order and holds new ones for that long; a server-overload `429000` comes without it.
Orders are refused, never queued. Open order caps: futures 100 limit and 200 stop orders per
contract; spot 2000 per account, 200 per pair.

### Stream

Before each connection a signed `POST /api/v1/bullet-private` on the book's own host ("cannot
be mixed") hands out `token`, `instanceServers[0].endpoint` and `pingInterval`; the socket
opens at `endpoint?token=...&connectId=...` (no login frame), its `welcome` read, then each
subscribe frame (`privateChannel: true`, `response: true`) answered by its `ack`. Topics: spot
`/spotMarket/tradeOrdersV2`; futures `/contractMarket/tradeOrders`,
`/contractMarket/advancedOrders` (stops), `/contract/positionAll` (`position.change` only). A
`{"type":"ping"}` every half `pingInterval` (9 s for the 18 s now): "send a `ping` at least once
every `pingInterval` seconds", and "Network delay should also be considered; some users
implement a heartbeat cycle of 9 seconds". A `match` frame is an execution with **no
fee amount**: kept `fee_pending`, its fee read from Get Trade History by `tradeId`. A futures
`match` with `tradeType` `liquid` or `adl` moved the position with no order of the account
behind it and asks for a `Resync`. The token lasts 24 h and the connection is dropped after
24 h; load-balancing disconnects "at most once or twice per week": each reconnect is followed
by a `Resync`. Polling stays the fallback.

### Disconnect behaviour

Nothing declared. Spot documents a Disconnection Protect (`/api/v1/hf/orders/dead-cancel-all`,
[Set DCP](https://www.kucoin.com/docs-new/rest/spot-trading/orders/set-dcp)) that OTW does not
arm; working orders stay working if OTW goes away.

### Dry-run

`POST /api/v1/hf/orders/test` and `POST /api/v1/orders/test`: "the request parameters and
return parameters ... exactly the same as the order endpoint, and can be used to verify whether
the signature is correct"; "the order will not enter the matching system, and the order cannot
be queried". Declared `validate`: it runs before the first live order of each session (E2D).
What business checks it runs (balance, rules) is not stated.

### Quirks

- Hosts: spot `https://api.kucoin.com`, futures `https://api-futures.kucoin.com` ([Authentication](https://www.kucoin.com/docs-new/authentication),
  futures cancel pages "support Futures URL"), although every exported OpenAPI block names
  `api.kucoin.com`.
- Spot lists working orders per symbol only (`symbol` required on Get Open Orders By Page): the
  whole book names the symbols with working orders first, then walks them, page by page. A
  walk's remaining symbols are kept 10 minutes from its last step, dropped by age only; a walk
  whose rest is gone fails the listing ("list again"), never ends it as an empty page, so a
  cancel all never reads "nothing left" from a lost walk.
- Futures Cancel All Orders needs a symbol and leaves the stops: per symbol, it goes with
  Cancel All Stop orders.
- The same code means different things per platform; `400100` and futures `200002` carry two
  meanings each, told apart by the message.
- Spot price protection silently cuts a market order short or cancels a limit past
  `priceLimitRate`; the order's state is read back as the venue reports it.

### Left out

- Futures attached take profit / stop loss (`/api/v1/st-orders`): the response holds only
  `orderId`; sibling cancel, child ids and how the exits show are not documented.
- Spot stop and OCO orders: both pages are `x-abandon: abandon` with no replacement named.
- Spot change (`/api/v1/hf/orders/alter`): a cancel and replace under a new venue id, not
  declared at this stage.
- Futures `closeOrder`, `forceHold`, `stp`, `RPI`, `leverage`; spot `GTT`, `funds`, `stp`.
- Coin-margined futures, margin trading, the Unified Trading Account API.
- Any sandbox: none exists.

### Doc gaps

- Whether accounts upgraded to the Unified Trading Account still trade through these classic
  endpoints (the changelog puts classic compatibility in "maintenance mode").
- The fee sign in fills (OTW reads positive as a cost, as the account read does); whether
  spot `tax` and futures `openFeeTaxPay` / `closeFeeTaxPay` are inside `fee`, and in which
  currency (not read).
- The answer to a lookup of a client id the venue never saw (`data: null` is read as not
  found; an error code would refuse the lookup and leave the order unknown).
- The code for an unknown or already final order on a spot cancel, and for an unknown order
  on a futures cancel (a `400100` on a cancel is looked up).
- The code for an off-grid price or size, for a reduce-only
  futures order that would add (the scripted T16 answers `300009`), for a `positionSide` that
  does not match the mode, and for a `marginMode` that differs from the symbol's.
- How long rejected orders stay queryable; whether `/contracts/active` lists contracts that
  are not `Open`; the futures minimum value.
- Whether a market order sent with a time in force is refused or ignored (none is sent);
  whether spot `postOnly` with IOC or FOK is refused or ignored (refused before I/O).
- Isolated leverage when flat; the leverage an isolated order takes without `leverage`.
- Whether HF spot orders answer a duplicate `clientOid` with `102426` "Duplicate user-defined
  unique order ID": the spot error page lists it among the Convert codes (mapped Unknown,
  looked up, either way).
- The order type of a futures `open` frame (its example carries no `orderType`) and the
  reduce-only flag and position side of a futures stop frame (its example names neither).
- Partial fill event order on the stream, fill corrections or busts (T25), ack timing.

### What stays unproven

Real fills, partial fills and their event order; real error bodies where they differ from the
doc; ack timing (the unknown window is the reconciliation cycle's); undocumented filters,
account-tier limits and regional restrictions; the private socket against the live venue.

### Fixtures

Doc examples in `fixtures/trade/doc_*.json` (sources in `trade_tests.rs`), converted from the
pages' YAML or JSON examples with their values unchanged; `doc_ws_fut_stop_order.json`
transcribed from a JavaScript object literal. Synthetic (built on a documented shape): the
catalog script's answers (doc examples with ids, sizes, prices, symbol and state fields
changed), every error body (documented code in the documented `code` / `msg` envelope), the
socket error frame, the closed-instrument variants, and the `symbols` answer of the spot walk
test (the shape of Get Symbols With Open Order).
