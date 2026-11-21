# Bybit (v5: spot, linear, inverse, options; Bybit EU spot)

Adapter version: 0.2.0. Provider API version: v5. Maturity: M2 (mock catalog green; history,
instrument lists, quotes and the socket of every book checked against the live public
endpoints; account reads on mock and on a lent key when one is set, see Verification).

One adapter per book:

| Constructor | Venue | Provider id | Book |
| --- | --- | --- | --- |
| `Bybit::spot()` | `BYBIT` | `bybit` | bybit.com spot |
| `Bybit::linear()` | `BYBIT_LINEAR` | `bybit` | USDT perpetuals, USDC perpetuals (`BTCPERP`), USDT dated futures (`BTCUSDT-26DEC26`) |
| `Bybit::inverse()` | `BYBIT_INVERSE` | `bybit` | coin-settled contracts, one USD each (`BTCUSD`, `BTCUSDZ26`) |
| `Bybit::option()` | `BYBIT_OPTION` | `bybit` | options (`BTC-26DEC26-100000-C-USDT`); no candles exist |
| `Bybit::eu_spot()` | `BYBIT_EU` | `bybit_eu` | Bybit EU (bybit.eu) spot |

`BTCUSDT` is a spot pair and a perpetual at once and they are different series. A data
connector picks its book with its `market` setting (`spot`, `linear`, `inverse`); a broker
account reads the books its `books` setting names (`spot`, `linear`, `inverse`, `option`, all
by default). Bybit EU is its own provider and broker: a separate platform with its own order
book (thinner, its own pairs: `BTCEUR`, `BTCUSDC`, no `BTCUSDT`), its own hosts and its own
keys, checked on the live venue (2026-10-18).

Sources:
- Kline: https://bybit-exchange.github.io/docs/v5/market/kline
- Instruments: https://bybit-exchange.github.io/docs/v5/market/instrument
- Tickers: https://bybit-exchange.github.io/docs/v5/market/tickers
- Socket: https://bybit-exchange.github.io/docs/v5/websocket/public/kline
- Authentication, hosts: https://bybit-exchange.github.io/docs/v5/guide
- API key info: https://bybit-exchange.github.io/docs/v5/user/apikey-info
- Wallet: https://bybit-exchange.github.io/docs/v5/account/wallet-balance
- Positions: https://bybit-exchange.github.io/docs/v5/position
- Open orders: https://bybit-exchange.github.io/docs/v5/order/open-order
- Executions: https://bybit-exchange.github.io/docs/v5/order/execution
- Account modes (classic retired): https://bybit-exchange.github.io/docs/v5/acct-mode
- Error codes: https://bybit-exchange.github.io/docs/v5/error
- Rate limits: https://bybit-exchange.github.io/docs/v5/rate-limit
- Official SDK used as signing reference: https://github.com/bybit-exchange/pybit (`_http_manager.generate_signature`)

## Endpoints

| Endpoint id | Method + path | Canonical |
| --- | --- | --- |
| `kline` | `GET /v5/market/kline?category&symbol&interval&start&end&limit=1000` | `RequestHistory{Candles}`, newest first, backwards from `end` (not options) |
| `instruments` | `GET /v5/market/instruments-info?category[&baseCoin=All][&limit=1000&cursor]` | `LoadInstruments`; spot whole, other books paged on `nextPageCursor` (follow-up) |
| `tickers` | `GET /v5/market/tickers?category&symbol` | `RequestSnapshot{Quote}` (`bid1Price`, `ask1Price`) |
| `board` | `GET /v5/market/tickers?category=spot` | `RequestBoard` (`lastPrice`) |
| `queryApi` | `GET /v5/user/query-api` (signed) | `Summary`, step 1: read-only or not, unified or classic, permissions |
| `walletBalance` | `GET /v5/account/wallet-balance?accountType=UNIFIED` (signed) | `Summary` step 2 (`totalEquity`), `Balances` (spot) |
| `positionList` | `GET /v5/position/list?category[&settleCoin]&limit=200[&cursor]` (signed) | `Positions`; linear walked USDT then USDC; inverse and options answer all |
| `orderRealtime` | `GET /v5/order/realtime?category[&settleCoin]&limit=50[&cursor]` (signed) | `OpenOrders`, linear walked USDT then USDC |
| `executionList` | `GET /v5/execution/list?category&startTime&endTime&limit=100[&baseCoin][&cursor]` (signed) | `Fills`, 7-day slices backwards from the period's end, each paged on `nextPageCursor`; options per base coin; 2000 pages per book |
| socket | `wss://stream.bybit.com/v5/public/{spot,linear,inverse}`, `wss://stream.bybit.eu/v5/public/spot`, `{"op":"subscribe","req_id":"kline.1.BTCUSDT","args":["kline.1.BTCUSDT"]}` | `Subscribe{Candles}` |

Cursors OTW hands back: fills `f|coins|coin_index|slice_end|pages|venue_cursor`, the option
coin listing `c|coins|venue_cursor`, settle-coin walks `coin_index|venue_cursor`. The venue's
own cursor (already percent-encoded, accepted either way) is carried verbatim.

## Hosts (`site` setting)

A key works only on the site that issued it. Public market data is read on the book's own
host (bybit.com or bybit.eu) whatever the site. Every regional bybit.com host was checked to
serve the same book as the global one (identical candles, 2026-10-18); bybit.eu does not.

| `site` | Host | Book |
| --- | --- | --- |
| `global` (default) | `api.bybit.com` | bybit.com |
| `bytick` | `api.bytick.com` | bybit.com |
| `tr` / `kz` / `ge` / `ae` / `id` | `api.bybit.tr` / `api.bybit.kz` / `api.bybitgeorgia.ge` / `api.bybit.ae` / `api.bybit.id` | bybit.com |
| `jp` / `hk` | `api.manepa.jp` / `api.spark-fintech.com` | bybit.com |
| `testnet` | `api-testnet.bybit.com` | testnet (own prices) |
| `eu` (Bybit EU default) / `eu-testnet` | `api.bybit.eu` / `api-testnet.bybit.eu` | Bybit EU |

A bybit.com account naming an EU site, or the other way round, is refused when saved, with
the fix. A refused key names the host it was sent to.

## Field mapping and rules

- Interval: `1 3 5 15 30 60 120 240 360 720 D W M`, the same on every book and the socket.
  The daily bar opens at midnight UTC, the weekly on Monday, the monthly on the 1st (checked
  on the live venue), so no variant is needed.
- History rows `[start, open, high, low, close, volume, turnover]`, strings, newest first.
  No closed flag: a bar is final 10 s after its close. Measured on 2026-10-18 by polling the
  archive through minute boundaries: on bybit.com a closed minute was last rewritten up to
  2.2 s after its close (20 bars over spot, linear and inverse), on Bybit EU never after it.
  A month closes on the calendar.
- Volume: in the base coin on spot and linear (linear contracts are sized in the coin),
  turnover in the quote. **Inverse is the other way round** (recorded: `volume` 3000332 USD
  contracts, `turnover` 37.11 BTC), so the two are swapped to keep volume in the coin.
- Socket rows carry `confirm`, mapped to `is_final`, and `timestamp`, the envelope's venue
  time. One subscribe frame per topic with the topic as `req_id`: the answer names its pane,
  a refusal fails that pane only (an inverse socket may send data before its ack). A refusal
  without `req_id` is read from the topic in brackets in `ret_msg`. `{"op":"ping"}` after 20
  quiet seconds; the pong is ignored.
- Instruments: `status` `Trading` is active (options: `Trading` and `Delivering`).
  `contractType` `LinearPerpetual`, `LinearFutures`, `InversePerpetual`, `InverseFutures`;
  `optionsType` `Call`, `Put` with `deliveryTime` as expiry. Any other value is logged and
  left out. An inverse contract carries `inverse = true` and its settle coin.
- Fees: Bybit signs a charge positive and a maker rebate negative, which is the canonical
  sign: kept as is, never `abs`. Fee asset from `feeCurrency`.
- Executions: `Trade`, `AdlTrade`, `BustTrade`, `Delivery`, `BlockTrade`, `MovePosition`,
  `FutureSpread` are fills; `Funding` and `Settle` are cash movements and left out; anything
  else is logged and left out. Option fills carry the option class.
- Option fills: the venue answers BTC alone without `baseCoin`, so the coins are read first
  from the option list (public, `baseCoin=All`, paged), then each is walked in turn.
- Inverse: positions and orders are converted from USD contracts to coins (size / entry
  price, order quantity / its price or trigger), quoted in USD; the unrealised profit is in
  the coin and is not carried. **Inverse fills are refused by name**: their profit is
  quantity × (1/entry − 1/exit) in the coin, which the journal does not compute.
- Positions: `side` `Buy` long, `Sell` short (negative quantity); a zero size is skipped; an
  open position with another side is a decode error. Linear currency = the settle coin.
- Balances: `walletBalance` is the total, `walletBalance - locked` the available amount.
- Summary: a classic account (`uta = 0`) is refused with the fix (Bybit retired classic
  accounts); a key that can trade is said so (`readOnly = 0`).

## Enum tables

| Native | Canonical |
| --- | --- |
| side `Buy` / `Sell` | `Side::Buy` / `Side::Sell`, else `Unknown(raw)` |
| contractType `LinearPerpetual` / `InversePerpetual` | `Perp` |
| contractType `LinearFutures` / `InverseFutures` | `Future` |
| optionsType `Call` / `Put` | `OptionRight::Call` / `Put` |
| other contractType / optionsType | left out (warned) |
| execType `Trade` `AdlTrade` `BustTrade` `Delivery` `BlockTrade` `MovePosition` `FutureSpread` | fill |
| execType `Funding` `Settle` | not a fill |

## Auth

`X-BAPI-SIGN` over `timestamp(ms) + api_key + recv_window + query string` (a POST signs its
JSON body instead), plus `X-BAPI-API-KEY`, `X-BAPI-TIMESTAMP`, `X-BAPI-RECV-WINDOW: 5000`.

- System-generated key: HMAC-SHA256, lowercase hex.
- Self-generated key: RSA-SHA256 PKCS#1 v1.5, base64; the private key is pasted as the secret
  in PEM, PKCS#8 (`BEGIN PRIVATE KEY`) or PKCS#1 (`BEGIN RSA PRIVATE KEY`), 2048 to 4096
  bits.

Both checked against Bybit's official SDK (pybit `generate_signature`) on the plain text of
Bybit's documented example: HMAC with a test secret, RSA with a throwaway key generated for
the test (`fixtures/`, allowlisted for gitleaks by path). Bybit publishes the plain text but
not the secret behind its example signatures, so no published vector exists. Secrets:
`api_key`, `api_secret`. Host clock.

## Verification

- Public half, live: candles, day/week anchors, forming bar, stability, symbol search, unknown
  symbol, quotes and the socket on spot, linear, inverse and Bybit EU.
- A made-up key on bybit.com and bybit.eu: HTTP 401 with an empty body, before the
  signature is looked at. It proves the request reaches the venue, not the signature.
- `brokers::net_tests::a_lent_bybit_key_reads_the_whole_account` runs every account read
  against a real key set in the environment (`OTW_BYBIT_KEY`, `OTW_BYBIT_SECRET`, optional
  `OTW_BYBIT_SITE`, `OTW_BYBIT_BROKER`, `OTW_BYBIT_BOOKS`). Until it has run green, account
  reads are verified on mock only.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `0` | success | none |
| HTTP 403, CloudFront page ("block access from your country") | country refused | Permission |
| HTTP 403 otherwise | IP over 600 requests / 5 s, banned 10 minutes (documented) | RateLimited, retry after 600 s |
| 10006, 10018, 10009 | too many visits, IP limit, IP banned | RateLimited |
| 10002, 10003, 10004, 10007, 10010, 33004 | timestamp, key, signature, auth, IP, key expired | Auth |
| 10005, 10024 | permission, compliance (region) | Permission |
| 10001 naming a symbol, 170121 | the venue does not list it (recorded: `Not supported symbols`) | NotFound ("bad symbol") |
| 10000, 10016 | server timeout, server error | Transient |
| anything else | | InvalidRequest |

## Rate limits

Public market data: 600 requests per 5 seconds per IP (declared as one bucket). Private
endpoints: 10 to 50 requests per second per account and endpoint (`retCode` 10006). Past the
IP limit Bybit answers HTTP 403 for 10 minutes.

## Quirks

- The archive answers HTTP 200 with `retCode` 10001 for an unknown symbol.
- Bybit EU answers candles for some pairs it does not list (`BTCUSDT`); its instrument list
  is the source of truth, and the symbol picker offers only what it lists.
- The fill history covers two years and at most seven days per call.
- Options on a coin that no longer has any listed option are not found by the coin walk.

## Regions and terms

Bybit refuses some jurisdictions (the United States among them) and answers them HTTP 403
from its CDN. Residents of the EEA are served by Bybit EU. Public market data of both
platforms answered from the test machine.

## Live best bid and ask

Topic `orderbook.1.<symbol>` on the candle socket of each market (spot, linear, inverse, EU): a
level-1 snapshot on every change, `b`/`a` as `[price, size]`, stamped `ts` (measured
2026-10-19). A delta naming one side keeps the other; a zero size empties it.

## Trading

Execution maturity: **E2D** (spot and linear books, and the Bybit EU spot book), built from
the documentation alone: no account, no sandbox key, no canary. Sandbox: Bybit testnet
(`site = testnet`, `api-testnet.bybit.com`), Bybit EU testnet (`site = eu-testnet`,
`api-testnet.bybit.eu`). Inverse and options take no orders. Code: `trade.rs` (mapper),
`trade_errors.rs` (error table); the order path is the shared trader's
(`connectors/engine/src/trading.rs`). Bybit EU: see [Bybit EU](#bybit-eu) below.

Doc snapshot (read 2026-11-11, github.com/bybit-exchange/docs tree `6233a5a0`):

| Page | Used for |
| --- | --- |
| [Place Order](https://bybit-exchange.github.io/docs/v5/order/create-order) | create body, order types, TIF, `orderLinkId`, `positionIdx`, `reduceOnly`, `triggerPrice`, `triggerDirection`, `triggerBy`, `marketUnit`, open order caps |
| [Cancel Order](https://bybit-exchange.github.io/docs/v5/order/cancel-order) | cancel by `orderId`, only unfilled or partially filled orders |
| [Cancel All Orders](https://bybit-exchange.github.io/docs/v5/order/cancel-all) | per symbol; spot `orderFilter` default `Order`; futures at most 500 at random |
| [Get Open & Closed Orders](https://bybit-exchange.github.io/docs/v5/order/open-order) | open orders, lookup by `orderLinkId` / `orderId`, last 500 closed, linear needs `symbol` or `settleCoin` |
| [Get Order History](https://bybit-exchange.github.io/docs/v5/order/order-list) | lookup after a venue restart, 7 days per call |
| [Get Trade History](https://bybit-exchange.github.io/docs/v5/order/execution) | fills, `execId`, `execType`, 7 days per call |
| [Get Position Info](https://bybit-exchange.github.io/docs/v5/position) | position rows by symbol "regardless of having position or not", `positionIdx`, `side`, `size` |
| [Switch Position Mode](https://bybit-exchange.github.io/docs/v5/position/position-mode) | one-way default, hedge per symbol or coin (OTW never calls it) |
| [Pre Check Order](https://bybit-exchange.github.io/docs/v5/order/pre-check-order) | dry run (margin preview), linear cross and portfolio margin only, no conditional order |
| [Instruments Info](https://bybit-exchange.github.io/docs/v5/market/instrument) | trading rules |
| [Enums](https://bybit-exchange.github.io/docs/v5/enum) | `orderStatus`, `timeInForce`, `positionIdx`, `triggerBy`, `status`, `cancelType`, `execType` |
| [Error Codes](https://bybit-exchange.github.io/docs/v5/error) | error table |
| [Rate Limit Rules](https://bybit-exchange.github.io/docs/v5/rate-limit) | order buckets, `X-Bapi-Limit-*` headers, IP ban |
| [Order (websocket)](https://bybit-exchange.github.io/docs/v5/websocket/private/order), [Execution (websocket)](https://bybit-exchange.github.io/docs/v5/websocket/private/execution), [Position (websocket)](https://bybit-exchange.github.io/docs/v5/websocket/private/position) | private stream frames |
| [Connect](https://bybit-exchange.github.io/docs/v5/ws/connect) | private stream hosts, login frame and signature, heartbeat, subscription answers |
| [Integration Guidance](https://bybit-exchange.github.io/docs/v5/guide) | signature over `timestamp + key + recv_window + body` for a POST |
| [Demo Trading](https://bybit-exchange.github.io/docs/v5/demo) | demo host and its API list (not used: testnet is the sandbox) |

### Bybit EU

Spot orders on Bybit EU (provider `bybit_eu`, `capabilities_eu.toml`, level E2D) go through
the spot mapper above unchanged, on the EU host the account's `site` names (`eu`:
`https://api.bybit.eu`, `eu-testnet`: `https://api-testnet.bybit.eu`). The EU docs say almost
nothing of their own on orders: on every point but the host, the v5 pages above apply.

| Page | Used for |
| --- | --- |
| [Integration Guidance](https://bybit-exchange.github.io/docs/v5/guide) | "EEA users: use `https://api.bybit.eu` for mainnet (EU site API only support "Connect to Third-Party Applications" feature for API broker user)" |
| [EU v3 intro](https://bybit-exchange.github.io/eu-docs/v3/intro) (github.com/bybit-exchange/eu-docs, commit `104e8b49`) | hosts `api.bybit.eu`, testnet `api-testnet.bybit.eu` with keys from testnet.bybit.eu; the same signature; `X-Referer` for broker users only (not sent) |
| [Get Trade History](https://bybit-exchange.github.io/docs/v5/order/execution), [Execution (websocket)](https://bybit-exchange.github.io/docs/v5/websocket/private/execution) | `extraFees`, "returned only for ... spot fiat currency orders placed on the EU site": a string over REST, a list on the stream |
| [Enums](https://bybit-exchange.github.io/docs/v5/enum#extrafeesfeetype) | `extraFeesFeeType`: `WHT` "EU withholding tax. Only for EU site", `AUT_WITHHOLDING_TAX` |
| [Connect](https://bybit-exchange.github.io/docs/v5/ws/connect), [System Status](https://bybit-exchange.github.io/docs/v5/websocket/system/system-status) | no EU private stream host (only `wss://stream.bybit.eu/v5/public/misc/status` is named) |
| [Error Codes](https://bybit-exchange.github.io/docs/v5/error) | the v5 table; the only EU code, `81007` "Bybit Europe is not supported create API Key", is about key creation, not orders |

- **Spot only**, as on Bybit spot: market and limit, GTC, IOC and FOK (a market order FOK
  refused), the market size in the base coin (`marketUnit = baseCoin`), no stop, no
  reduce-only, no position, no pre-check (it serves linear only). Spot margin
  (`isLeverage=1`) and every other category: doc silent, not offered.
- **Polled only.** The doc names no EU private stream host, so `private_socket` answers none
  for `eu` and `eu-testnet`: the account is read on the reconciliation cycle.
- **`extraFees`** are read as fee lines of their own beside `execFee` (each line's `feeCoin`,
  `fee`, and `feeType` with its `subFeeType`). The doc does not say whether `execFee` already
  includes them, so nothing is netted (a fee counted twice is possible if Bybit EU folds them
  into `execFee`: doc gap). The REST string's non-empty form is never shown: it is read when
  it is the same list written as JSON. Any other string, or a line with no amount, is left out
  and logged (never a guess at a fee); the fill and the rest of the page are kept.
- **A personal EU key may be refused.** The guide's sentence above can mean that the EU site's
  API serves only third-party applications of API broker users. The doc does not settle it and
  the bybit.eu help pages could not be read. OTW sends the order as for any key: if Bybit EU
  refuses it, the user sees Bybit's refusal with its code (an unlisted code is unknown and
  alerts, as everywhere), and nothing is sent again.
- **EU doc gaps** (beyond the v5 ones below): which categories bybit.eu accepts, EU rate limits
  and IP-ban behaviour, EU-only refusal codes (KYC, MiCA, appropriateness test, retail limits),
  whether `extraFees` is inside `execFee`, the EU testnet's scope and its stream host, whether
  `api.bytick.eu` still serves v5, and how long ended orders stay queryable.

### Order types and flags

| Canonical | Native | Notes |
| --- | --- | --- |
| `market` | `orderType=Market` | Bybit works it as an IOC limit within its slippage band, "Market order will always use IOC" ([Place Order](https://bybit-exchange.github.io/docs/v5/order/create-order)): no `timeInForce` sent, and a market or stop order asked FOK is refused before I/O (`check_order`). Spot: `marketUnit=baseCoin`, so `qty` is in the base coin (Bybit reads a spot market buy in the quote coin by default). |
| `limit` | `orderType=Limit`, `price` | |
| `stop` (linear) | `orderType=Market`, `triggerPrice`, `triggerDirection` 1 (buy, rises to it) or 2 (sell, falls to it), `triggerBy=LastPrice` | "if `triggerPrice` is set, the order will be automatically converted into a conditional order" ([Place Order](https://bybit-exchange.github.io/docs/v5/order/create-order)) |
| `stop_limit` (linear) | `orderType=Limit`, `price`, `triggerPrice`, `triggerDirection`, `triggerBy=LastPrice` | same |
| `reduce_only` (linear) | `reduceOnly=true` | "Valid for `linear`, `inverse` & `option`"; not declared on spot (a sale of what is held is a plain sell) |
| reduce-only stop (linear) | `reduceOnly=true` and `closeOnTrigger=true` | a triggered conditional order is cancelled when margin is short; `closeOnTrigger` makes a protective stop reduce the position regardless. **Side effect** (create-order, closeOnTrigger): "other active orders of similar contracts will be cancelled or reduced" when the balance is short at the trigger. |
| position | `positionIdx` 0 (one-way), 1 (hedge buy side), 2 (hedge sell side) | never switched; held per account and symbol, not read before an order: read once at connect (every held position, by settle coin) and when the chart opens the instrument, updated from the private `position` topic, read again after a `10001` refusal of an order (the doc names no code for a wrong `positionIdx`; the refused order is not resent) |

The [error page](https://bybit-exchange.github.io/docs/v5/error) lists no code for a wrong
`positionIdx` (only `110024`, `110025`, `110028`, about switching the mode), so `10001` is the
trigger that reads the mode again.

Each order goes to its instrument's book: the one the chart's Bybit data connector is set to
(`market`), else the one book that lists the symbol; `BTCUSDT` (spot pair and perpetual) with
no Bybit connector to name the book is refused with that fix. Ids carry the book
(`BOOK:SYMBOL:orderId`, positions `BOOK:SYMBOL:side`). Leverage is read only, from the
position list's `leverage` ([Get Position Info](https://bybit-exchange.github.io/docs/v5/position))
and the account's `marginMode` ([Get Account Info](https://bybit-exchange.github.io/docs/v5/account/account-info):
`ISOLATED_MARGIN`, `REGULAR_MARGIN` shown as cross, `PORTFOLIO_MARGIN`; the position's
`tradeMode` is "Deprecated, always 0"), held from the reads and the `position` stream; spot
has none. The grid a ticket snaps to is the instrument's own (tick, size step, smallest size
from Instruments Info).

A dry run (pre-check) is sent without `orderLinkId`, so the venue never sees the live order's
client id before the order (a `110072` duplicate on the real order would leave it unknown).

### Attached take profit and stop loss (linear, `brackets`)

A ticket's entry (market or limit, not reduce-only) with a take profit (a limit or
if-touched exit) and/or a stop loss (a stop exit), each on the other side and of the entry's
size, is one `POST /v5/order/create` with `tpslMode=Full`, `takeProfit`, `stopLoss`,
`tpOrderType`/`slOrderType` `Market` ([Place Order](https://bybit-exchange.github.io/docs/v5/order/create-order):
"`Full`: entire position for TP/SL. Then, tpOrderType or slOrderType must be `Market`"; "When
reduceOnly is true, take profit/stop loss cannot be set"). A limit take profit therefore fires
at the market. Changing or removing one is [Set Trading Stop](https://bybit-exchange.github.io/docs/v5/position/trading-stop)
(`tpslMode=Full`, the new price, or `"0"`: "0 means cancel"), on the position's `positionIdx`.

How they appear: the position carries `takeProfit`/`stopLoss` ([Get Position Info](https://bybit-exchange.github.io/docs/v5/position));
the venue's conditional orders show in [Get Open & Closed Orders](https://bybit-exchange.github.io/docs/v5/order/open-order)
with `stopOrderType` `TakeProfit`/`StopLoss` ([Enums](https://bybit-exchange.github.io/docs/v5/enum)) and
`parentOrderLinkId`, "the linked parent order for attached take-profit and stop-loss orders".
The pane shows them as `TakeProfit`/`StopLoss` orders, reduce-only, linked to the entry while
it still works. OTW follows each exit under `BOOK:SYMBOL:tpsl:ENTRY:tp|sl:buy|sell`: working
with its entry until the entry fills, then the venue's order found by `parentOrderLinkId` and
`stopOrderType` among the open and the last 500 closed orders.

A position with no take profit or stop loss gets one the same way (`position_exits` in the
linear caps): Set Trading Stop with the new price, from the positions panel.

Spot orders placed elsewhere: Get Open & Closed Orders answers "all kinds of orders by
default" (`orderFilter`), so the account read lists the conditional ones too. A spot order
whose `stopOrderType` is set is followed as `cond:<orderFilter>:<orderId>` (`tpslOrder`,
`OcoOrder`, `BidirectionalTpslOrder` as named; `StopOrder` for any other conditional type),
because [Cancel Order](https://bybit-exchange.github.io/docs/v5/order/cancel-order) takes
`orderFilter` on spot only and "If not passed, `Order` by default": the cancel names it. That
page lists `Order`, `tpslOrder` and `StopOrder` only; an OCO or bidirectional TP/SL cancel is
sent with its own filter and a refusal is the venue's. Linear conditional orders keep the
venue's id (no filter there).

Not offered: `Partial` mode and limit TP/SL, spot TP/SL (`tpslOrder`).

Not declared: spot stops (`orderFilter=StopOrder`/`tpslOrder`: the doc states no trigger
direction for them), post-only (`PostOnly` TIF), amend, OCO, trailing stop, iceberg, trigger
choice.

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| `gtc` | `GTC` | default when not passed |
| `ioc` | `IOC` | |
| `fok` | `FOK` | |

No day or good-till-date order ([Enums, timeInForce](https://bybit-exchange.github.io/docs/v5/enum)).

### Client order id

`orderLinkId`: "A max of 36 characters. Combinations of numbers, letters (upper and lower
cases), dashes, and underscores are supported", unique, optional on spot and linear
([Place Order](https://bybit-exchange.github.io/docs/v5/order/create-order)). The OTW key is
sent unchanged; a key outside that charset or length is refused before I/O. Looked up with
`GET /v5/order/realtime?orderLinkId=` then `GET /v5/order/history?orderLinkId=`
([Get Open & Closed Orders](https://bybit-exchange.github.io/docs/v5/order/open-order): "After a
server release or restart, filled, cancelled, and rejected orders ... should only be queried
through order history"). [Get Order History](https://bybit-exchange.github.io/docs/v5/order/order-list)
keeps "Cancelled", "Rejected" and "Deactivated" orders for the last 24 hours only, other
closed ones 7 days, and beyond that only orders with fills: an order with no fill that ended
more than 24 hours ago (and left the last 500 of `realtime`) answers "never seen", which the
reconciliation cycle reads as `not received` for an unknown leg and `missing_at_broker` for a
working one. OTW follows an order as `SYMBOL:orderId`.

### Order status

| Native | Canonical |
| --- | --- |
| `New` | New |
| `PartiallyFilled` | PartiallyFilled |
| `Untriggered` (conditional created) | New, `trigger_pending` set on the order |
| `Triggered` ("instantaneous state ... from Untriggered to New") | New, `trigger_pending` cleared |
| `Filled` | Filled |
| `Cancelled` (may have an executed qty) | Cancelled |
| `PartiallyFilledCanceled` (spot) | Cancelled |
| `Deactivated` (spot conditional cancelled before trigger) | Cancelled |
| `Rejected` | Rejected |
| any other word | Unknown, alerts (logged, notified, reconciled) |

Source: [Enums, orderStatus](https://bybit-exchange.github.io/docs/v5/enum).

### Instrument trading rules

`GET /v5/market/instruments-info?category=&symbol=` on the account's host
([Instruments Info](https://bybit-exchange.github.io/docs/v5/market/instrument)), read once a day
and again after a rule refusal:

| Book | Tick | Size step | Min size | Max size (limit / market) | Min value | Status |
| --- | --- | --- | --- | --- | --- | --- |
| spot | `priceFilter.tickSize` | `lotSizeFilter.basePrecision` | none (`minOrderQty` "deprecated, no longer check") | `maxLimitOrderQty` / `maxMarketOrderQty` | `minOrderAmt` | `Trading`, else closed |
| linear | `priceFilter.tickSize`, `minPrice`, `maxPrice` | `qtyStep` | `minOrderQty` | `maxOrderQty` / `maxMktOrderQty` | `minNotionalValue` | `Trading`; every other word halted (no new order, cancels go out) |

`Delivering` is halted, not close-only: the [status enum](https://bybit-exchange.github.io/docs/v5/enum)
gives it no meaning and no page says a reduce-only order is accepted in it (error `110042`
names a "pre-delivery status" without tying it to this word). The default list holds
`Trading` and `PendingOpen` only ("By default returns `Trading` & `PendingOpen` symbols"): a
linear symbol missing from it is asked again by `status` (`PreLaunch`, `Delivering`, `Closed`)
so a listed instrument that is not trading is refused as such, never as unknown; spot "has
`Trading` only". Rules of an instrument that is not trading are not kept: it is read again on
the next order, so it takes orders the moment it opens.

A refused order names the rule and the nearest valid values; nothing is adjusted. A market
order's value is checked only when the ticket carries a reference price.

### Errors

The order codes of the [error page](https://bybit-exchange.github.io/docs/v5/error) (UTA and Spot
Trade sections) as mapped (codes of other endpoints, such as leverage, margin mode and TP/SL
settings, are not listed and would be Unknown with an alert) in `trade_errors.rs` (a test asserts each
row maps as listed and that an unlisted code is Unknown and alerts):

| Code | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| `0` | OK | none | |
| timeout, closed socket, HTTP 5xx | no answer | Unknown | looked up by `orderLinkId`, never resent |
| HTTP 403 (CDN country page) | country refused | Permission | refused |
| HTTP 403 otherwise | IP banned 10 minutes | RateLimited | new orders held 600 s |
| `-1`, `10002`, `10003`, `10004`, `10007`, `10010`, `-2015`, `33004` | expired request, time window, key, signature, auth, IP, key expired | Auth | refused, nothing resent |
| `10005`, `10008`, `10009`, `10024`, `10027`, `10028`, `100028`, `110067`, `110068`, `110069`, `110071`, `110088`, `110096`, `110097`, `110102`, `110123`, `110124`, `110125`, `110132`, `110136`, `170036`, `170209`, `170219`, `170224`, `170241`, `170348`, `170358`, `170359`, `3100197` | key permission, account mode, region, compliance, terms, account type | Permission | refused |
| `429`, `10006`, `10018`, `170005`, `170222` | rate limits | RateLimited | refused, new orders held until `X-Bapi-Limit-Reset-Timestamp` or `Retry-After` |
| `10000`, `10014`, `10016`, `40004`, `110035`, `110072`, `110079`, `170001`, `170007`, `170032`, `170035`, `170141`, `170146`, `170147`, `170195`, `170196`, `170212`, `170234` | timeout, duplicate request or client id, server error, processing, "your order may not be filled" | Unknown | looked up, never resent |
| `10001`, `10017`, `10029`, `110018`, `110019`, `110029`, `110032`, `110057`, `110108`, `110109`, `110119`, `170105`, `170115`, `170116`, `170117`, `170121`, `170130`, `170145`, `170201`, `170221`, `170202`, `170227`, `170355` | malformed request | InvalidRequest | refused (a bug signal) |
| `110017`, `110094`, `170124`, `170134`, `170136`, `170137`, `170140`, `170148`, `170197` to `170200`, `170341`, `170381`, `170382` | size or price off the rules, a size too large for the market | Rejected(venue_rule) | refused, rules read again |
| `30256`, `110004`, `110006`, `110007`, `110011`, `110012`, `110039`, `110040`, `110044`, `110045`, `110051`, `110052`, `110053`, `110101`, `110131`, `170033`, `170034`, `170131`, `170207`, `170228`, `170229`, `170346`, `3200306` | balance or margin | Rejected(funds) | refused |
| `30208`, `30209`, `110003`, `110092`, `110093`, `110120`, `110121`, `170132`, `170133`, `170192` to `170194`, `170203`, `170204`, `170218`, `170371`, `170372` | price band, trigger already crossed | Rejected(price) | refused |
| `30228`, `110023`, `110042`, `110063`, `110066`, `110070`, `110074`, `110095`, `110098`, `110100`, `110103`, `110134`, `110137`, `170031`, `170151`, `170157`, `170159`, `170206` | not trading now, close-only, auction | Rejected(market closed) | refused |
| `110001`, `170143`, `170213` | order does not exist | Rejected(not found) | a cancel is reconciled (looked up by `orderId`) |
| `110008`, `110010`, `170139`, `170142`, `170190` | order already final | Rejected(final) | a cancel is reconciled: no error when it ended |
| `30133` to `30136`, `110005`, `110009`, `110020`, `110021`, `110022`, `110080`, `110085` to `110087`, `110089`, `110090`, `110104` to `110107`, `110118`, `110135`, `170010`, `170011`, `170037`, `170144`, `170149`, `170150`, `170191`, `170210`, `170215`, `170216`, `170217`, `170220`, `170223`, `170226`, `170230`, `170344`, `170709`, `170810`, `3200403`, `3400139` | other refusals (order caps, risk limits, institutional loan, margin) | Rejected(other) | refused, venue's words shown |
| any other code | not in the doc's order codes | Unknown, alerts | looked up, never resent, notified |

### Rate limits for orders

`POST /v5/order/create` 10/s on linear, 20/s on spot; cancel 10/s and 20/s; cancel-all 10/s
and 20/s; pre-check 10/s and 20/s; realtime, history, executions 50/s, all per UID
([Rate Limit Rules](https://bybit-exchange.github.io/docs/v5/rate-limit)). `10006` refuses the
order and holds new ones until `X-Bapi-Limit-Reset-Timestamp`; HTTP 403 is the 10-minute IP
ban. Orders are refused, never queued.

### Stream

`wss://<site stream host>/v5/private` ([Connect](https://bybit-exchange.github.io/docs/v5/ws/connect):
`stream.bybit.com`, `stream-testnet.bybit.com`, the regional hosts of `tr`, `id`, `kz`, `ge`,
`jp`, `hk`; `bytick` and `ae` have none listed, and are polled). Login
`{"op":"auth","args":[api_key, expires, signature]}`, the signature a hex HMAC-SHA256 of
`GET/realtime{expires}`; the page documents the HMAC key only, so an RSA key is followed by
polling. Categorised topics `order.<book>`, `execution.<book>`, `position.linear`
([Order](https://bybit-exchange.github.io/docs/v5/websocket/private/order),
[Execution](https://bybit-exchange.github.io/docs/v5/websocket/private/execution),
[Position](https://bybit-exchange.github.io/docs/v5/websocket/private/position)); a
`{"op":"ping"}` every 20 s ("we recommend ... every 20 seconds"). The login and subscription
are answered before the stream is handed back. Reconnects back off 1 s to 60 s, each followed
by a `Resync`; the stream carries no sequence across messages, so a gap shows only as a
reconnect. A position frame updates the held position mode and, when its size changed, asks
for a `Resync` (the topic fires on every order action "regardless if there's any actual
change"); an execution other than `Trade` and `Funding` (`AdlTrade`, `BustTrade`, `Delivery`,
...) moved a position with no order behind it and asks for a `Resync`. Polling (the
reconciliation cycle) stays the fallback.

### Disconnect behaviour

Nothing declared. Bybit documents a Disconnect Cancel Protection (`/v5/order/disconnected-cancel-all`,
[DCP](https://bybit-exchange.github.io/docs/v5/order/dcp)) that OTW does not arm; working
orders stay working if OTW goes away.

### Dry-run

Linear: [Pre Check Order](https://bybit-exchange.github.io/docs/v5/order/pre-check-order),
the create body sent to `POST /v5/order/pre-check`; answers the initial and maintenance
margin rates before and after (`preImrE4`, `postImrE4`, ...; "30 means 0.30%"). Cross and
portfolio margin only, no conditional order. At E2D it runs before the first live order of the
session; a conditional first order is refused rather than sent unchecked. Spot: none.

### Quirks

- Live, 2026-11-11 (public conformance, no key): spot lists 521 instruments, linear 930, all
  with sound rules; an order signed with a key Bybit never issued answers `10003` (Auth); an
  unsigned one answers `10001` "apiKey is missing" (InvalidRequest), not an auth code.
- A market order on spot is sized in the quote coin unless `marketUnit` says otherwise.
- Cancel All on futures cancels "up to 500 orders ... randomly": the trader reads the open
  orders again after it and cancels what is left one by one.
- Spot Cancel All cancels plain orders only by default: OTW sends it once per `orderFilter`.
- Cancel All answers `success` "0" (with `retCode` 0) for a call that failed: what it left is
  cancelled one by one.
- Get Trade History answers seven days per call ("endTime - startTime <= 7 days"): a longer
  period is walked in seven-day windows. `execType` `Funding` moves cash only and is left out;
  every other type than `Trade` is kept as an execution that moved a position (`position_only`).

### Doc gaps

What the documentation does not state, so OTW does not rely on it (each closed only by a
sandbox run, E3, or the live canary, E4):

- Attached TP/SL (`Full`): the client id the venue's TP/SL orders carry (only
  `parentOrderLinkId` is documented), whether Set Trading Stop takes a change before the entry
  has filled (no position yet), and whether the TP and SL of one position cancel each other
  (the doc says "The system will cancel these orders if the position is closed").
- The error code for a reduce-only order that would increase a position (only `cancelType`
  `CancelByReduceOnly` is listed); the scripted T16 answers `110017`.
- The default `triggerBy` of a conditional order: OTW sends `LastPrice` explicitly.
- The spot conditional order's trigger direction (`orderFilter=StopOrder`).
- Rounding: whether a price or size off the grid is refused or rounded (OTW refuses first).
- Ack timing: "the request is asynchronous", with no bound on when an order becomes visible
  to `/v5/order/realtime`; the unknown window is the reconciliation cycle's (10 s ack window,
  5 minute deadline), not measured.
- Partial fill event order on the private stream, and fill corrections or busts (T25).
- The fee currency of a linear fill over REST: the Get Trade History example carries
  `"feeCurrency": ""` (the private `execution` example carries `"USDT"`).
- Whether a market order sent GTC is taken (the default when none is passed) or refused:
  OTW sends no time in force on a market order.
- The pre-check answer for an isolated-margin account (the doc says it is not supported, not
  what it returns): a pre-check refusal refuses the live order (fail closed).
- The error body shape of each code: the doc lists codes, the envelope is the one every
  documented answer carries (`retCode`, `retMsg`, `result`, `retExtInfo`, `time`).
- The wrong-`positionIdx` refusal code (an order sent with the other mode's index).
- Testnet and live differences (rules, rate limits) beyond the host.

### What stays unproven

Real fills, partial fills and their event order; real error bodies where they differ from the
doc; ack timing; undocumented filters, account-tier limits and regional restrictions; testnet
against live differences.

### Fixtures

Verbatim doc examples in `fixtures/trade/doc_*.json` (sources in `trade_tests.rs`). Synthetic
(`synthetic`, built on a documented shape): the catalog script's answers (the doc examples
with ids, sizes, prices, symbol and status changed), every error body (documented code in the
documented envelope), the closed-instrument variant of the instrument list, and the mock
answers of `otw-core/src/brokers/native_trade_tests.rs` (including the stream frames built from
the documented `order`, `execution` and `position` examples), and the `10001` "position idx"
refusal of the catalog's position mode scenario.
