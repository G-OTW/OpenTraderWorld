# Gate.io (spot and USDT perpetuals)

Adapter version: 0.1.0. Provider API version: v4. Maturity: M2 (mock catalog green; history,
instrument lists, quotes and both sockets checked against the live venue on 2026-10-19;
account parity on mock only, no key lent).

One OTW provider, `gateio`, two adapters chosen by the connector's `market` setting (and the
broker's `books` setting): `Gateio::spot()` (venue `GATEIO`) and `Gateio::futures()` (venue
`GATEIO_FUTURES`, the USDT-settled perpetuals). `BTC_USDT` names a spot pair and a perpetual
at once, so the book is a setting, never inferred from the ticker. The BTC-settled (inverse,
`BTC_USD`) and dated (delivery) contracts are not served.

Sources:
- API v4 reference: https://www.gate.com/docs/developers/apiv4/en/
- Official SDK, gateio/gateapi-python at commit `22fdf34ae4af855863ccb01b95e8c996de4cbea3`
  (2026-07-16): `docs/SpotApi.md`, `docs/FuturesApi.md`, `docs/AccountApi.md`,
  `docs/UnifiedApi.md` and the models (`Trade`, `Order`, `OpenOrders`, `Position`,
  `FuturesOrder`, `MyFuturesTradeTimeRange`, `UnifiedAccount`, `UnifiedBalance`,
  `AccountDetail`); signing in `gate_api/api_client.py` (`gen_sign`).
- WebSocket v4: https://www.gate.com/docs/developers/apiv4/ws/en/ and
  https://www.gate.com/docs/developers/futures/ws/en/
- Measured answers of the public endpoints and sockets, 2026-10-19 (recorded in `tests.rs`).

## Endpoints

All REST paths are below `https://api.gateio.ws/api/v4`.

| Endpoint id | Spot | Futures (`settle` = `usdt`) | Canonical |
| --- | --- | --- | --- |
| `candlesticks` | `GET /spot/candlesticks?currency_pair&interval&from&to` | `GET /futures/usdt/candlesticks?contract&interval&from&to` | `RequestHistory{Candles}` |
| `contract` | | `GET /futures/usdt/contracts/{contract}` | contract size before a candle or quote call |
| `currency_pairs` / `contracts` | `GET /spot/currency_pairs` | `GET /futures/usdt/contracts` | `LoadInstruments` |
| `tickers` | `GET /spot/tickers?currency_pair` | `GET /futures/usdt/tickers?contract` | `RequestSnapshot{Quote}` |
| `tickers` | `GET /spot/tickers` | | `RequestBoard` (`last`) |
| `accountDetail` | `GET /account/detail` (signed) | | `Summary` |
| `unifiedMode` | `GET /unified/unified_mode` (signed) | | `Balances`, step one |
| `spotAccounts` / `unifiedAccounts` | `GET /spot/accounts` or `GET /unified/accounts` (signed) | | `Balances`, step two |
| `openOrders` | `GET /spot/open_orders?page&limit=100` (signed) | | `OpenOrders` |
| `myTrades` | `GET /spot/my_trades?page&limit=1000&from&to` (signed) | | `Fills` |
| `futuresAccounts` | | `GET /futures/usdt/accounts` (signed) | `Summary` |
| `positions` | | `GET /futures/usdt/positions?holding=true&limit=100&offset` (signed) | `Positions` |
| `orders` | | `GET /futures/usdt/orders?status=open&limit=100&offset` (signed) | `OpenOrders` |
| `myTradesTimerange` | | `GET /futures/usdt/my_trades_timerange?offset&limit=1000&from&to` (signed) | `Fills` |

Sockets: spot `wss://api.gateio.ws/ws/v4/`, futures `wss://fx-ws.gateio.ws/v4/ws/usdt`.
Subscribe: `{"time","id","channel":"spot.candlesticks"|"futures.candlesticks","event":"subscribe","payload":["<interval>","<symbol>"]}`;
the answer echoes `id` with `result.status` `success` or `fail` (`error.message` names the
symbol, measured). Updates: `event` `update`, `result` one row on spot and a list on
futures, keyed by `n` = `<interval>_<symbol>`. Keepalive: `{"time","channel":"spot.ping"}`
(or `futures.ping`) after 20 quiet seconds.

## Field mapping and rules

- Interval tokens (measured on both archives and sockets): `1m 5m 15m 30m 1h 4h 8h 1d`. The
  week is `7d` on spot (Monday, measured `1791763200` = Mon 2026-10-12) and `1w` on futures
  (Monday); the futures `7d` opens on Thursday and the spot socket refuses `1w`. The month is
  spot's `30d`, a calendar month (measured: the first of each month). The futures `30d`
  rolls every 30 days with no volume, so futures declare no month.
- Daily bars open at midnight UTC on both books (measured).
- Spot archive row: `[t s, quote volume, close, high, low, open, base volume, window_closed]`,
  all strings; the last field is the closed flag. Futures archive row: an object
  `{t, o, h, l, c, v, sum}` with `v` an integer count of contracts and `sum` the turnover in
  USDT. Contracts become coins through the contract's `quanto_multiplier` (0.0001 BTC on
  `BTC_USDT`).
- Paging (measured): spot refuses a range of more than 1000 points
  (`Candlestick range too broad`), futures refuses `limit` above 2000 and answers the newest
  2000 of a wider range. Both take `from` and `to` in seconds, both ends inclusive. Pages
  walk backwards from the last bar opening before the range's end, 1000 (spot) or 2000
  (futures) bars at a time.
- Lookback (measured): both archives refuse anything older than 10000 bars of the asked
  timeframe (`Candlestick too long ago. Maximum 10000 points ago are allowed`). A range is cut
  at 9980 bars back from now; a range wholly older is refused as Unsupported, naming the
  date of the oldest bar served. 1m reaches back about a week, 1h about 14 months.
- Closed flags. Spot: the row's flag; measured over ten minute boundaries, no row changed
  once flagged. Futures archive: no flag; measured over ten minute boundaries on BTC, ETH,
  SOL and XRP, a closed minute was rewritten up to 7.3 s after its close (18 rewrites), so a
  bar is final 30 s after its close. Both sockets carry `w`, true on the frame pushed at the
  boundary (measured).
- Socket volumes: spot `v` is the quote volume and `a` the base amount; futures `v` is
  contracts and `a` the quote volume (measured: 66476 contracts × 0.0001 × 82735 ≈ `a`).
  A futures pane reads its contract's size from the instrument list before subscribing
  (`live_requires_instruments`); one the list does not hold is refused by name.
- Spot list: tick = 10^-`precision`, lot = 10^-`amount_precision`, min = `min_base_amount`,
  active only when `trade_status` is `tradable`. Futures list: `type` `direct` only (an
  `inverse` contract is left out), base and quote read from the name, lot = one contract,
  min = `order_size_min` contracts, active when `status` is `trading` and not
  `in_delisting`; `contract_type` (`stocks`, `indices`, `metals`, `forex`, `commodities`, empty
  for crypto) kept as the note.
- Futures quotes: `highest_size` / `lowest_size` are contracts, multiplied into coins.

## Account

- Balances: `unified_mode` first. `classic` reads `/spot/accounts` (available + locked);
  any unified mode reads `/unified/accounts`, where a coin's holding is available + freeze −
  borrowed (a net borrowing is no holding). A key refused the mode check (Permission,
  InvalidRequest, NotFound) falls back to the classic spot account; an Auth refusal is
  reported.
- Fills: spot `my_trades` covers the spot, unified and margin accounts by default (SDK
  description); its range may not exceed 30 days and its time filter applies to the
  order's end time. Futures `my_trades_timerange` by `offset`. Both have an undocumented
  or unclear ceiling on paging depth (`limit * (page - 1) <= 100,0` in the SDK text), so a
  slice whose first page comes back full is read again at half the width before anything
  of it is kept, down to an hour; a slice whose first page is under half full lets the next
  one widen again, up to seven days. 2000 calls per pull at most.
- Futures quantities are contracts (signed: the side is the sign); the instrument list's
  multiplier is the point value. Fees as written: positive a cost, negative a maker rebate.
  Spot fees: `fee` in `fee_currency`, else `gt_fee` in GT, else `point_fee` as `POINT`.
- Spot open orders page while any pair filled the page (100 orders per pair per page).
- Price-triggered orders follow the plain ones on both books: `GET /spot/price_orders` and
  `GET /futures/usdt/price_orders` with `status=open`, `limit` 100, paged by `offset`. Each
  is `cond:<id>`; its cancel goes to `DELETE /spot/price_orders/{order_id}` or
  `DELETE /futures/usdt/price_orders/{order_id}`. Rows read as the SDK's
  `SpotPriceTriggeredOrder` (`market`, `trigger.price`, `put.type`/`side`/`price`/`amount`,
  `ctime` in seconds) and `FuturesPriceTriggeredOrder` (`initial.contract`/`size`/`price`,
  `trigger.price`, `create_time`, `order_type`; `size` 0 closes the whole position, its side
  told by `order_type` (`close-long-*`, `close-short-*`, `plan-close-long-position`,
  `plan-close-short-position`) or `auto_size`, its type marked "(whole position)" and its size
  0: none is stated). A spot market buy's `put.amount` is the quote coin to spend, so its size
  shows 0. The spot list asks the account's mode first (`GET /unified/unified_mode`, as for
  balances): a unified account's list is asked with `account=unified` ("Trading account type.
  Unified account must be set to `unified`", SDK `list_spot_price_triggered_orders`), the
  next pages too (cursor `cond:unified:<offset>`); a key refused the mode check reads the
  list with no `account`. **(SDK only)**: every one of these facts is from the SDK
  (`SpotApi.md`, `FuturesApi.md` and the model pages): the doc site answered no page.
  Placing a price-triggered order from OpenTraderWorld is not declared.
- Summary: spot reads `/account/detail` (user id, key mode, IP list size); futures the USDT
  futures account total and unrealised PnL. Gate.io publishes no call that names a key's
  permissions, so `can_trade` is unknown.

## Auth

`SIGN` = lowercase hex HMAC-SHA512 of
`METHOD + "\n" + /api/v4/path + "\n" + query + "\n" + hex(SHA512(body)) + "\n" + timestamp`,
the query written unencoded (the SDK signs `unquote_plus(urlencode(query))`), the timestamp in
seconds; sent with `KEY` and `Timestamp`. Checked against the overview's worked example (both
SIGN values, byte for byte) and a reference computed with Python's `hmac` and `hashlib`. Secrets: `api_key`, `api_secret`. Host clock: the venue refuses a
timestamp more than 60 s off (`REQUEST_EXPIRED`, measured).

## Rate limits

Public market data: 200 requests per 10 s per endpoint per IP (`x-gate-ratelimit-limit: 200`
measured). Private endpoints: 200 per 10 s per account on most.

## Error labels

| Label | Meaning | Canonical |
| --- | --- | --- |
| none, 2xx | success | none |
| `TOO_MANY_REQUESTS`, HTTP 429 | rate limit | RateLimited |
| `INVALID_KEY`, `INVALID_SIGNATURE`, `INVALID_CREDENTIALS`, `REQUEST_EXPIRED`, `MISSING_REQUIRED_HEADER`, `IP_FORBIDDEN`, `ACCOUNT_LOCKED` | credentials | Auth |
| `FORBIDDEN`, `READ_ONLY` | permission | Permission |
| `INVALID_CURRENCY_PAIR`, `INVALID_CURRENCY`, `CONTRACT_NOT_FOUND` | unknown symbol (measured) | NotFound, naming the symbol |
| `SERVER_ERROR`, `INTERNAL`, `TOO_BUSY` | venue error | Transient |
| anything else (`INVALID_PARAM_VALUE`…) | | InvalidRequest |

A body that is not a label answer falls back to the HTTP status.

## Not covered

- BTC-settled and dated futures, options, margin and earn accounts, sub-accounts.
- Placing price-triggered (stop) orders: they are read, listed and cancelled only.

## Regions and terms

Gate.io does not serve some jurisdictions (the United States among them); public market
data answered from the test machine.

## Live best bid and ask

`spot.book_ticker` / `futures.book_ticker` on the candle socket, payload `[symbol]`: `{t, s, b,
B, a, A}` on every change; futures sizes are numbers in contracts, multiplied by the contract
size (measured 2026-10-19). Spot acks a pair it does not list and sends nothing.

## Trading

Execution maturity: **E2D** (spot and USDT perpetual books), built from the documentation
alone: no account, no testnet key, no canary. Sandbox: Gate.io testnet (`site = testnet`:
REST `https://api-testnet.gateapi.io/api/v4`, sockets on `ws-testnet.gate.com`), its own keys,
"request formats are identical" (overview, "Testnet"). The account read follows the same
`site` setting (a testnet account is read on the testnet host). Code: `trade.rs` (mapper),
`trade_errors.rs` (error table), the socket frame signature in `signer.rs`; the order path is
the shared trader's (`connectors/engine/src/trading.rs`).

How the doc was read (2026-11-11 and 2026-11-12). The live doc site answers automated clients
with a Cloudflare challenge (HTTP 403, not worked around). Facts come from:
- **(SDK)** the official SDK, gateio/gateapi-python at commit
  `22fdf34ae4af855863ccb01b95e8c996de4cbea3` (2026-07-16, spec v4.105.8), `docs/SpotApi.md`,
  `docs/FuturesApi.md`, `docs/AccountApi.md` and the model pages named below. It is generated
  from the same OpenAPI spec as the doc site, and older than it.
- **(WF)** the live pages read through a summarising reader: quotes are short and a long text
  is never copied whole. Each (WF) fact is worth a browser check before E3.

| Page | Used for |
| --- | --- |
| [API v4 overview](https://www.gate.com/docs/developers/apiv4/en/) (WF) | signature and its worked example (both SIGN values), testnet host, error shape `{label, message}`, the label list and meanings, HTTP 202 and 204, `X-Gate-RateLimit-*` headers, order rate limits |
| [Spot](https://www.gate.com/docs/developers/apiv4/en/spot/) (WF); SDK `SpotApi.md`, `Order.md`, `OrderCancel.md`, `OrderPatch.md`, `BatchAmendItem.md`, `BatchOrder.md`, `Trade.md`, `CurrencyPair.md`, `Ticker.md`, `OpenOrders.md` | create, cancel, cancel all, amend, get, list, open orders, personal trades, pairs, tickers; `market_order_max_stock` (WF only) |
| SDK only: `SpotApi.md` (`list_spot_price_triggered_orders`, `get_spot_price_triggered_order`, `cancel_spot_price_triggered_order`, `cancel_spot_price_triggered_order_list`), `FuturesApi.md` (`list_price_triggered_orders`, `get_price_triggered_order`, `cancel_price_triggered_order`, `cancel_price_triggered_order_list`, `get_dual_mode_position`), `SpotPriceTriggeredOrder.md`, `SpotPriceTrigger.md`, `SpotPricePutOrder.md`, `FuturesPriceTriggeredOrder.md`, `FuturesPriceTrigger.md`, `FuturesInitialOrder.md`, `Contract.md` (`in_delisting`, `position_size`) | price-triggered orders listed (`status=open`, `account=unified` on a unified account), read and cancelled by id, cancelled all; never placed |
| [Futures](https://www.gate.com/docs/developers/apiv4/en/futures/) (WF); SDK `FuturesApi.md`, `FuturesOrder.md`, `FuturesOrderAmendment.md`, `MyFuturesTradeTimeRange.md`, `Contract.md`, `Position.md`, `FuturesAccount.md` | create, cancel, cancel all, amend, get, list, trades by time range, contracts, position, account `position_mode`; `market_order_size_max`, `enable_decimal`, the dual-mode position note (WF only) |
| SDK `AccountApi.md`, `AccountDetail.md` | `GET /account/detail` `user_id` (the futures channels' user id) |
| [Spot WebSocket v4](https://www.gate.com/docs/developers/apiv4/ws/en/) (WF) | hosts, per-frame auth, `spot.orders`, `spot.usertrades`, `spot.ping`, subscribe answers |
| [Futures WebSocket v4](https://www.gate.com/docs/developers/futures/ws/en/) (WF) | hosts, per-frame auth, `futures.orders`, `futures.usertrades`, `futures.positions`, `futures.ping`, server pings, subscribe answers |

### Order types and flags

| Canonical | Spot (`POST /spot/orders`) | Futures (`POST /futures/usdt/orders`) |
| --- | --- | --- |
| `limit` | `type=limit`, `amount` (base), `price`, `time_in_force` | `size` in whole contracts (positive a buy, negative a sell), `price`, `tif` |
| `market` | `type=market`; a sell's `amount` is the base coin, a buy's the **quote coin to spend** (SDK `Order.md`); `time_in_force` `ioc` or `fok` ("Only `ioc` and `fok` are supported when `type`=`market`") | `price` "0" with `tif` `ioc` ("Price of 0 with `tif` set to `ioc` represents a market order", SDK `FuturesOrder.md`) |
| `reduce_only` | not declared (a spot sale of what is held is a plain sell) | `reduce_only=true` |
| close a position | none | the trader's reduce-only market order for the size read from the venue (whole or part); Gate.io's own `size=0, close=true` / `auto_size` is not used |
| post-only, iceberg, stop, stop-limit, trailing, attached TP/SL, OCO | not declared | not declared |

- **Spot market buy, sized in the quote coin** (user decision: accept and convert). Just
  before sending, the trader reads the pair's ticker (`GET /spot/tickers?currency_pair`,
  public) and values the size at `lowest_ask` ("Recent lowest ask"); the order goes out with
  `amount` = size x ask, rounded down to the pair's `precision` (`10^-precision`): the doc
  names no separate step for that amount (no field of `CurrencyPair` does, WF), so the
  pair's precision is used; a value Gate.io still refuses comes back as its
  `INVALID_PRECISION` refusal. The answer tells the user the filled size may differ a little.
  A failed or empty ticker read sends nothing.
- **Market GTC.** A market order works immediate-or-cancel on both books: one asked GTC or
  IOC goes out `ioc`; a spot market FOK goes `fok`; a futures market FOK is refused before
  I/O. The futures order page states only "Price of 0 with `tif` set to `ioc` represents a
  market order" (SDK `FuturesOrder.md`); "market orders currently only support ioc mode" is
  said of a price-triggered order's initial order (SDK `FuturesInitialOrder.md`), read here
  as the rule for a plain one too: FOK at the market is not stated either way.
- **Contracts.** Every futures size is a whole number of contracts of `quanto_multiplier`
  base coins ("the number of contracts is specified `size`, not the number of coins", SDK
  create_futures_order). OTW states sizes in the base coin and converts; a size that is not
  a whole number of contracts is refused naming the two nearest. Decimal contract sizes
  (`enable_decimal`, header `X-Gate-Size-Decimal`) are not used: the doc states no step.
- **Position mode** is the futures account's `position_mode` (`GET /futures/usdt/accounts`):
  `single` one-way, `dual` hedge; read once per account (with no position open), held,
  forgotten after a `POSITION_DUAL_MODE` refusal, never switched (`POST /futures/usdt/dual_mode`
  is never called). No position index is sent: in dual mode the sign of `size` and
  `reduce_only` pick the side ("Reduce position: reduce_only=true, size is a positive number
  that indicates short position, negative number that indicates long position", SDK
  create_futures_order). Split mode (`dual_plus`, needs `pid`) is refused before I/O, naming
  the fix. The deprecated `in_dual_mode` is read where `position_mode` is absent.
- **One contract's positions** (a close, the leverage read): in dual mode, as held, `GET
  /futures/usdt/dual_comp/positions/{contract}`, both sides in a list ("For dual mode position
  operations, refer to `/futures/{settle}/dual_comp/positions` API group", changelog v4.16.0;
  SDK `get_dual_mode_position`); in single mode, or before any mode is held, `GET
  /futures/usdt/positions/{contract}`.
- **Leverage and margin**, read only, from the position row: `leverage` ("0 means cross
  margin; positive number means isolated margin"), `cross_leverage_limit` when cross,
  `pos_margin_mode` where the row carries it (WF). Never set by OTW.
- **Change in place** (`amend`): futures `PUT /futures/usdt/orders/{id}` with the new `price`
  and/or `size`: "New order size, including filled part", sent in whole contracts of the
  contract size, signed like the order ("Order side must be identical to the original one";
  whether the sign is required is doc silent). Spot: the single
  amend is `PATCH /spot/orders/{id}`, a method OTW's requests cannot carry, so the change goes
  through `POST /spot/amend_batch_orders` with one row, and "Only one of `amount` or `price` can
  be specified": a change of both is refused before I/O; the row's `succeeded`/`label` is the
  answer. "Modifying the price or increasing the quantity will adjust the priority to the end
  of the new price level"; a quantity under the filled one cancels the order.

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| `gtc` | `gtc` | default; a market order asked GTC goes `ioc` |
| `ioc` | `ioc` | |
| `fok` | `fok` | limit on both books, market on spot only |

`poc` (post-only) exists on both books and is not declared. No day or good-till-date order.

### Client order id

`text`: "1. prefixed with `t-` 2. no longer than 28 bytes without `t-` prefix 3. can only
include 0-9, A-Z, a-z, underscore(_), hyphen(-) or dot(.)" (SDK `Order.md`, `FuturesOrder.md`).
OTW sends `t-` + its key; a key outside that rule is refused before I/O. Any other `text`
(`web`, `api`, `app`, `apiv4`, the liquidation words) is the order's source, read as no client
id. Uniqueness: doc silent (labels `ORDER_EXISTS`, `DUPLICATE_REQUEST` exist, mapped Unknown).

Look up by client id, in turn:
- spot: `GET /spot/orders/{text}?currency_pair` ("Operations based on custom IDs can only be
  checked in pending orders. Only order ID can be used after the order is finished"), then
  the first page of `GET /spot/orders?currency_pair&status=finished` matched by `text`;
- futures: `GET /futures/usdt/orders/{text}` (by text "within 60 seconds after the end of the
  order"), then the first page of `GET /futures/usdt/orders?contract&status=finished`
  matched by `text`. "Zero-fill orders cannot be retrieved 10 minutes after order
  cancellation": such an order answers "never seen" after that.
OTW follows an order as `BOOK:SYMBOL:id`.

### Order status

| Native | Canonical |
| --- | --- |
| spot `status` `open` (REST), stream `event` `put` / `update` | New; PartiallyFilled once `filled_amount` is above 0 |
| futures `status` `open` | New; PartiallyFilled once `left` is under the size |
| spot `closed`, `finish_as` `filled` | Filled |
| spot `cancelled` ("regardless of partial fills") | Cancelled (with what filled) |
| `finish_as` `ioc`, `fok`, `poc`, `stp`, `small`, `depth_not_enough`, `trader_not_enough`, `liquidate_cancelled`, `price_protect_cancelled`, `unified_check_failed` (spot); `cancelled`, `liquidated`, `auto_deleveraged`, `reduce_only`, `position_closed`, `reduce_out`, `stp`, `ioc` (futures) | Cancelled (with what filled) |
| `unknown`, `-`, `finished` without `finish_as`, any other word | Unknown, alerts |

A futures `finished` order reads its `finish_as`; a spot stream frame (no `status`) reads
`event` and, on `finish`, `finish_as`. Rejected is never a plain order's status: a refusal is
an HTTP error with a `label`. Sources: SDK `Order.md`, `FuturesOrder.md`; spot and futures
WebSocket pages (WF).

A price-triggered order (`cond:<id>`) has words of its own (SDK `SpotPriceTriggeredOrder.md`,
`FuturesPriceTriggeredOrder.md`):

| Native | Canonical |
| --- | --- |
| spot `status` `open`; futures `open`, `inactive` (an order take profit or stop loss whose order has not filled yet) | New, waiting for its trigger |
| spot `cancelled`; futures `finished` + `finish_as` `cancelled` | Cancelled |
| spot `finish` ("Successfully completed"); futures `succeeded` | Filled: it placed its order; what traded is on that order (`fired_order_id` / `trade_id`), not on this one |
| spot or futures `failed` | Rejected |
| spot or futures `expired` | Expired |
| futures `invalid`, any other word | Unknown, alerts |

### Price-triggered orders on the order path

The working list the order path reads (`open_orders`, which cancel all and the kill switch
walk) holds the price-triggered orders after the plain ones: futures `GET
/futures/usdt/price_orders?status=open&limit=100&offset[&contract]`; spot the account's mode
first (`GET /unified/unified_mode`), then `GET /spot/price_orders?status=open&limit=100&offset`
with `account=unified` for a unified account (a key refused the mode check reads it with no
`account`). Spot on one pair lists the plain orders only: nothing asks for one pair's price
orders. Each row is `cond:<id>`, sized in the base coin (futures: contracts times
`quanto_multiplier`; a whole-position close, `size` 0, shows 0); its side is the `initial`
size's sign, or the side that closes the position `order_type` / `auto_size` names.
- Cancel all: `DELETE /spot/orders` then `DELETE /spot/price_orders` (with `market` per pair
  when pairs are named); futures `DELETE /futures/usdt/orders?contract` then `DELETE
  /futures/usdt/price_orders?contract` per contract ("Cancel all auto orders", SDK). A price
  order the call leaves (a unified account's: the call sends no `account`) is listed again and
  cancelled on its own (`DELETE /spot/price_orders/{order_id}`, which takes no `account`).
- Look up by id: `GET /spot/price_orders/{order_id}`, `GET /futures/usdt/price_orders/{order_id}`
  ("Query single auto order details", SDK).
**(SDK only)**: the doc site answered no page for these calls.

### Instrument trading rules

Read from the whole public list once a day and again after a rule refusal (a single pair or
contract call answers an unknown name with an error rather than an empty list):

| Book | List | Tick | Size step | Min / max size | Market max | Value | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| spot | `GET /spot/currency_pairs` | 10^-`precision` | 10^-`amount_precision` | `min_base_amount` / `max_base_amount` (`null` no limit) | `market_order_max_stock` (WF) | `min_quote_amount` / `max_quote_amount` | `tradable` trading; `buyable` / `sellable` trading (the venue refuses the closed side itself, OTW has no side-specific state); `untradable` closed |
| futures | `GET /futures/usdt/contracts?limit=100&offset` | `order_price_round` | one contract (`quanto_multiplier`) | `order_size_min` / `order_size_max` contracts | `market_order_size_max` contracts (WF, 0 none) | none stated | `trading` trading; `delisting` or `in_delisting` close-only (`CONTRACT_IN_DELISTING`: "only reduce-only order or close order is allowed"); `in_delisting` with `position_size` 0 closed ("indicates the contract is delisted", SDK `Contract.md`); `prelaunch`, `delisted`, `circuit_breaker` closed |

A refused order names the rule and the nearest valid values; nothing is adjusted. Sizes are
stated in the base coin (contracts times `quanto_multiplier`). `order_price_deviate` (the
band around the mark price) is left to the venue (`PRICE_TOO_DEVIATED`).

### Errors

Gate.io answers a refusal with an HTTP error status and `{label, message}`; "Programs can use
`label` to identify and catch a specific error" (overview, WF). The labels of the overview's
list that an order call can answer, as mapped in `trade_errors.rs` (a test asserts each row maps
as listed and that an unlisted label is Unknown and alerts). The user sees "Gate.io refused it:
<message> (code <label>)". Loan, repayment, risk limit update and margin transfer labels are
not listed: no order call answers them. The futures position and margin labels
(`ORDER_PENDING`, `POSITION_HOLDING`, `POSITION_CROSS_MARGIN`), `NO_MERGEABLE_ORDERS` and the
Flash Convert `REQUEST_FORBIDDEN` are listed as clean refusals (WF, label list), so one
answered to an order call does not alert.

| Label | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| timeout, closed socket, HTTP 5xx with no label, HTTP 202 | no answer, or "accepted ... processing is not done yet" | Unknown | looked up by `text`, never resent |
| HTTP 429 with no label | rate limit | RateLimited | refused, new orders held until the reset |
| HTTP 404 with no label, on a call by order id (get, cancel, price order get or cancel) | the order is not known there | Rejected(not found) | a lookup moves on to its next call; a cancel reads the order back |
| `INVALID_PARAM_VALUE` | invalid parameter value | InvalidRequest | refused |
| `INVALID_PROTOCOL` | invalid parameter value | InvalidRequest | refused |
| `INVALID_ARGUMENT` | invalid argument | InvalidRequest | refused |
| `INVALID_REQUEST_BODY` | invalid request body | InvalidRequest | refused |
| `MISSING_REQUIRED_PARAM` | missing required parameter | InvalidRequest | refused |
| `BAD_REQUEST` | invalid request | InvalidRequest | refused |
| `INVALID_CONTENT_TYPE` | invalid Content-Type header | InvalidRequest | refused |
| `NOT_ACCEPTABLE` | invalid Accept header | InvalidRequest | refused |
| `METHOD_NOT_ALLOWED` | request method is not allowed | InvalidRequest | refused |
| `NOT_FOUND` | request URL not exists | InvalidRequest | refused |
| `INVALID_CREDENTIALS` | invalid credentials provided | Auth | refused |
| `INVALID_KEY` | invalid API key | Auth | refused |
| `IP_FORBIDDEN` | request IP not in whitelist | Auth | refused |
| `READ_ONLY` | API key is read-only | Permission | refused |
| `INVALID_SIGNATURE` | invalid signature | Auth | refused |
| `MISSING_REQUIRED_HEADER` | missing required authentication header | Auth | refused |
| `REQUEST_EXPIRED` | request Timestamp is far from the server time | Auth | refused |
| `ACCOUNT_LOCKED` | account is locked | Auth | refused |
| `FORBIDDEN` | account has no permission to request operation | Permission | refused |
| `DUPLICATE_REQUEST` | duplicate request | Unknown | looked up by `text`, never resent |
| `ORDER_EXISTS` | order already exists, do not resubmit | Unknown | looked up by `text`, never resent |
| `INVALID_CLIENT_ORDER_ID` | the client_order_id is invalid | InvalidRequest | refused |
| `INVALID_PRECISION` | invalid precision | Rejected(venue_rule) | refused, the rules read again |
| `INVALID_CURRENCY` | invalid currency | InvalidRequest | refused |
| `INVALID_CURRENCY_PAIR` | invalid currency pair | InvalidRequest | refused |
| `POC_FILL_IMMEDIATELY` | order would match and take immediately so it's cancelled | Rejected(post-only) | refused |
| `ORDER_NOT_FOUND` | order not found | Rejected(not found) | reconciled: the order is read |
| `ORDER_CLOSED` | order already closed | Rejected(final) | reconciled: the order is read |
| `ORDER_CANCELLED` | order already cancelled | Rejected(final) | reconciled: the order is read |
| `QUANTITY_NOT_ENOUGH` | amount is not enough | Rejected(venue_rule) | refused, the rules read again |
| `BALANCE_NOT_ENOUGH` | balance is not enough | Rejected(funds) | refused |
| `MARGIN_NOT_SUPPORTED` | currency pair doesn't provide margin trading | InvalidRequest | refused |
| `MARGIN_BALANCE_NOT_ENOUGH` | margin balance is not enough | Rejected(funds) | refused |
| `AMOUNT_TOO_LITTLE` | amount does not reach minimum required | Rejected(venue_rule) | refused, the rules read again |
| `AMOUNT_TOO_MUCH` | amount exceeds maximum allowed | Rejected(venue_rule) | refused, the rules read again |
| `REPEATED_CREATION` | repeated creation | Unknown | looked up by `text`, never resent |
| `NO_CHANGE` | no change is made | InvalidRequest | refused |
| `TOO_MANY_CURRENCY_PAIRS` | too many currency pairs in batch orders creation | InvalidRequest | refused |
| `TOO_MANY_ORDERS` | too many orders | Rejected(other) | refused |
| `MIXED_ACCOUNT_TYPE` | more than one account type in a batch | InvalidRequest | refused |
| `AUTO_BORROW_TOO_MUCH` | auto borrow exceeds maximum allowed | Rejected(funds) | refused |
| `TRADE_RESTRICTED` | trading is restricted due to high debt ratio | Rejected(other) | refused |
| `FOK_NOT_FILL` | FOK order cannot be filled completely | Rejected(other) | refused |
| `INITIAL_MARGIN_TOO_LOW` | total initial margin rate is too low | Rejected(funds) | refused |
| `ORDER_BOOK_NOT_FOUND` | insufficient liquidity | Rejected(other) | refused |
| `NO_MERGEABLE_ORDERS` | orders can be merged not found | Rejected(other) | refused |
| `FAILED_RETRIEVE_ASSETS` | failed to retrieve account assets | Unknown | looked up by `text`, never resent |
| `CANCEL_FAIL` | order cancel failed | Unknown | looked up by `text`, never resent |
| `USER_NOT_FOUND` | user has no futures account | Permission | refused |
| `CONTRACT_NO_COUNTER` | no counter order found | Rejected(other) | refused |
| `CONTRACT_NOT_FOUND` | contract not found | InvalidRequest | refused |
| `RISK_LIMIT_EXCEEDED` | risk limit exceeded | Rejected(other) | refused |
| `INSUFFICIENT_AVAILABLE` | balance is not enough | Rejected(funds) | refused |
| `LIQUIDATE_IMMEDIATELY` | operation may cause liquidation | Rejected(funds) | refused |
| `LEVERAGE_TOO_HIGH` | leverage too high | Rejected(other) | refused |
| `LEVERAGE_TOO_LOW` | leverage too low | Rejected(other) | refused |
| `ORDER_NOT_OWNED` | order not owned | Rejected(not found) | reconciled: the order is read |
| `ORDER_FINISHED` | order already finished | Rejected(final) | reconciled: the order is read |
| `POSITION_IN_LIQUIDATION` | position is being liquidated | Rejected(other) | refused |
| `POSITION_IN_CLOSE` | position is closing | Rejected(other) | refused |
| `POSITION_EMPTY` | position is empty | Rejected(reduce-only) | refused |
| `PRICE_TOO_DEVIATED` | order price deviates too much from mark price | Rejected(price) | refused |
| `SIZE_TOO_LARGE` | order size exceeds maximum | Rejected(venue_rule) | refused, the rules read again |
| `SIZE_TOO_SMALL` | order size does not reach minimum | Rejected(venue_rule) | refused, the rules read again |
| `PRICE_OVER_LIQUIDATION` | price to increase position can not exceed liquidation price | Rejected(price) | refused |
| `PRICE_OVER_BANKRUPT` | price to decrease position cannot exceed bankrupting price | Rejected(price) | refused |
| `ORDER_POC_IMMEDIATE` | POC order will be finished immediately | Rejected(post-only) | refused |
| `INCREASE_POSITION` | POC order will increase position | Rejected(reduce-only) | refused |
| `CONTRACT_IN_DELISTING` | contract is delisting, only reduce-only order or close order is allowed | Rejected(market closed) | refused |
| `POSITION_NOT_FOUND` | position not found | Rejected(reduce-only) | refused |
| `POSITION_DUAL_MODE` | operation forbidden in dual-mode | InvalidRequest | refused |
| `REDUCE_EXCEEDED` | reduce order would exceed position in dual-mode | Rejected(reduce-only) | refused |
| `AMEND_WITH_STOP` | amend forbidden with stop order | InvalidRequest | refused |
| `ORDER_FOK` | killed for FOK | Rejected(other) | refused |
| `ORDER_PENDING` | operation forbidden with pending orders | Rejected(other) | refused |
| `POSITION_HOLDING` | operation forbidden with holding position | Rejected(other) | refused |
| `POSITION_CROSS_MARGIN` | margin updating is not allowed in cross margin | Rejected(other) | refused |
| `INTERNAL` | internal server error | Unknown | looked up by `text`, never resent |
| `SERVER_ERROR` | internal server error | Unknown | looked up by `text`, never resent |
| `INTERNAL_SERVER_ERROR` | operation failed, please try again later | Unknown | looked up by `text`, never resent |
| `TOO_BUSY` | server is too busy at the moment | Unknown | looked up by `text`, never resent |
| `TOO_MANY_REQUESTS` | request rate exceeds limits | RateLimited | refused, new orders held until the reset |
| `SERVER_TIMEOUT` | service timeout | Unknown | looked up by `text`, never resent |
| `REQUEST_FORBIDDEN` | asset management product is under liquidation; only USDT purchases are allowed | Rejected(other) | refused |
| any other label | not in the doc's list | Unknown, alerts | looked up, never resent, notified |

A batch answer (the spot amend) carries a row per order (`succeeded`, `label`, `message`):
its failed row is the call's refusal. Spot cancel-all answers a row per order the same way:
a failed row is cancelled on its own after.

### Rate limits for orders

Spot: place and amend 10 requests/s per UID and market, cancel 200/s. Futures: place and amend
100/s, cancel 200/s. Others 200 per 10 s per endpoint. Fill-ratio throttles apply to accounts
that place without filling (overview, "Frequency limit rule", WF). HTTP 429 refuses the order
and holds new ones until `X-Gate-RateLimit-Reset-Timestamp` (unit not stated: a value under
10^12 is read as seconds). Orders are refused, never queued.

### Stream

One socket per book, followed on its own:
- spot `wss://api.gateio.ws/ws/v4/` (testnet `wss://ws-testnet.gate.com/v4/ws/spot`):
  `spot.orders` and `spot.usertrades`, payload `["!all"]`;
- futures `wss://fx-ws.gateio.ws/v4/ws/usdt` (testnet
  `wss://ws-testnet.gate.com/v4/ws/futures/usdt`): `futures.orders`, `futures.usertrades`,
  `futures.positions`, payload `[user_id, "!all"]`, the user id read from
  `GET /account/detail` (`user_id`, SDK `AccountDetail.md`) before each connection.

Each subscribe frame is its own login: `{"time", "channel", "event": "subscribe", "payload",
"auth": {"method": "api_key", "KEY", "SIGN"}}`, SIGN the hex HMAC-SHA512 of
`channel=<channel>&event=<event>&time=<time>` (WF; no worked value, tested against an
independent implementation). Each is answered (`result.status` `success`, or `error` with
`code`/`message`, code 4 "authentication fail") before the next. Heartbeat `spot.ping` /
`futures.ping` every 10 s (the pages state no interval, their demos ping every 10 s); the
futures server also sends protocol pings, answered. Orders, fills and positions are read as
over REST; a position frame updates the held position mode and leverage; a fill of an order
Gate.io placed itself (`text` `auto_deleveraging`, `liquidation`, `liq-...`, ...) asks for a
`Resync`. Stream fills carry no fee currency on futures (the settlement currency is read, as
over REST). Polling (the reconciliation cycle) stays the fallback.

### Disconnect behaviour

Nothing declared. Gate.io documents countdown cancel-all (`POST /spot/countdown_cancel_all`,
`POST /futures/usdt/countdown_cancel_all`) that OTW does not arm; working orders stay working
if OTW goes away.

### Dry-run

None on either book (overview: none documented). The nearest, `action_mode` and the countdown
cancel-all, are not checks.

### Quirks

- Live, 2026-11-12 (public conformance, no key): spot lists 2156 pairs, futures 1086
  contracts (eleven pages of 100), all with sound rules; an order signed with a key Gate.io
  never issued answers `INVALID_KEY` (Auth) on both books, an unsigned one
  `MISSING_REQUIRED_HEADER` "Missing required header: Timestamp" (Auth).
- Spot cancel-all without `currency_pair` cancels every pending order of the spot, unified and
  isolated margin accounts ("When the `account` parameter is not specified, all pending orders
  including spot, unified account, and isolated margin will be cancelled"): the kill switch is
  that call and the price-triggered one. Futures cancel-all goes per contract, both calls.
- Spot fills: "The range not allowed to exceed 30 days", and "The maximum number of pages
  ... is 100,0, that is, limit * (page - 1) <= 100,0" (SDK `list_my_trades`, read as 1000):
  pages of 1000 rows (the `limit` maximum), so two pages per window at most. A period is
  walked in windows of one day (`from` and `to` rounded out to whole seconds, far inside the
  30 days), 2000 fills per pair and day; a busier day asks a third page, which Gate.io
  refuses: the read fails, it never answers short.
- Futures fills (`my_trades_timerange`) page by `offset` (100 rows); no window bound is stated.
- Futures fees: `fee` in the settlement currency (the fill names none); `point_fee` is a fee
  line of its own. Spot: `fee` in `fee_currency`; `gt_fee` (GT) and `point_fee` (POINT) are
  fee lines of their own beside it.
- The spot market buy row's `amount` is the quote coin spent: its size is `filled_amount`
  (the doc does not state that unit, see "Doc gaps"). While it works (PartiallyFilled), its
  size is what filled so far.

### Doc gaps

What the documentation does not state (or states only on pages OTW could read through the
summarising reader), so OTW does not rely on it; each closed only by a testnet run (E3) or
the live canary (E4):
- Verbatim response examples: none could be copied, every fixture is built on the schema.
- The step of a spot market buy's quote amount (rounded down to the pair's `precision`).
- Client id uniqueness and its window; the lookup-by-text window: the futures single-order
  page says 60 s, the list's `last_id` says one hour, and the changelog says "Spot and Futures
  operations based on order ID also accept user custom ID(but only for 30 minutes since
  creation)" (v4.19.5, WF).
- The unit of a spot market buy's `filled_amount` (read as the base coin bought: `Order.md`
  says "Amount filled" and names the quote coin only for `amount`), and so its size while
  it works.
- `market_order_max_money`, the quote-coin cap of a spot market buy (WF; "no limit when
  their value is null or 0", changelog v4.106.122), is not read: a market buy over it is
  sent and Gate.io refuses it.
- Spot retention of finished orders, and the order of the finished list (only its first page
  is read in a lookup).
- Whether an order refused FOK or post-only leaves a queryable record.
- What a timeout, a 5xx or a 202 means for an order (treated as unknown, looked up).
- Off-grid prices and sizes: rounded or refused (OTW refuses first).
- The decimal contract size step; the futures fills window and fee currency.
- Futures market FOK.
- What `GET /futures/usdt/positions/{contract}` answers in dual mode.

- The unit of `X-Gate-RateLimit-Reset-Timestamp`.
- The ping interval, and whether the ping's `time` is checked (OTW sends it unstamped).
- Whether the label list is exhaustive for order endpoints.
- Testnet and live differences beyond the hosts.

### What stays unproven

Real fills, partial fills and their event order; real error bodies and statuses where they
differ from the doc; ack timing; undocumented filters, account-tier limits and regional
restrictions; testnet against live differences.

### Fixtures

All `synthetic`, in `fixtures/trade/` (sources in `trade_tests.rs`): built on the SDK model
pages and the WebSocket field tables, since no response example could be copied verbatim.
Every error body is synthetic too (`{label, message}`). The scenarios run against a mock venue
through the shared trader in `trade_tests.rs`, not through the testkit's catalog: its request
probes carry no contract size and no quote amount, so it cannot drive this venue.
