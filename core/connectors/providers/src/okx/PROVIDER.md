# OKX (v5)

Adapter version: 0.1.0. Provider API version: v5. Maturity: M2 (mock catalog green; shadow
runs against the live venue, history, instrument list and socket, identical to the legacy
connector; account parity on mock, real probes refused by the venue as expected).

Sources:
- REST and socket reference: https://www.okx.com/docs-v5/en/
- Market data: https://www.okx.com/docs-v5/en/#order-book-trading-market-data
- Error codes: https://www.okx.com/docs-v5/en/#error-code

## Endpoints

| Endpoint id | Method + path | Canonical | Notes |
| --- | --- | --- | --- |
| `historyCandles` | `GET /api/v5/market/history-candles?instId&bar&after&limit=300` | `RequestHistory{Candles}` | reads backwards: `after` (exclusive) starts at the window end, next page at the oldest bar of a full page |
| `instrumentsSpot/Swap/Futures` | `GET /api/v5/public/instruments?instType=SPOT|SWAP|FUTURES` | `LoadInstruments` | three requests, listed in that order |
| `ticker` | `GET /api/v5/market/ticker?instId` | `RequestSnapshot{Quote}` | a one-sided book emits nothing |
| `tickers` | `GET /api/v5/market/tickers?instType=SPOT` | `RequestBoard` | `last` |
| `balance` | `GET /api/v5/account/balance` (signed) | `QueryAccount{Summary}` / `{Balances}` | unified account: one line per currency (`eq`), no cost basis |
| `positions` | `GET /api/v5/account/positions` (signed) | `QueryAccount{Positions}` | `posSide` long/short in hedge mode, the sign of `pos` in one-way mode; a linear swap or future in the base coin (`pos x ctVal`, multiplier 1), as the order path trades it |
| `ordersPending` | `GET /api/v5/trade/orders-pending?limit=100` (signed) | `QueryAccount{OpenOrders}` | trigger price from `slTriggerPx`; a linear swap or future in the base coin (`sz`, `accFillSz` x `ctVal`) |
| `ordersAlgoPending` | `GET /api/v5/trade/orders-algo-pending?ordType&limit=100[&after]` (signed) | `QueryAccount{OpenOrders}`, after `ordersPending` | one call per `ordType` the doc lists (`conditional,oco` together as it allows, `trigger`, `move_order_stop`, `chase`, `iceberg`, `twap`, `smart_iceberg`), paged `after` the last `algoId`; each row `cond:<algoId>`, trigger from `triggerPx`, `slTriggerPx`, `tpTriggerPx` or `moveTriggerPx`, limit from `ordPx`, `slOrdPx`, `tpOrdPx` or `pxLimit` (`-1`, at the market, is none); cursor `algo:<type>:<after>`. A type the venue refuses to list (InvalidRequest, NotFound, Permission) is logged and skipped, the other lists are read; a rate limit, a key refusal or a lost answer still fails the read. A linear contract's `sz` in the base coin |
| `termsSwap` / `termsFutures` | `GET /api/v5/public/instruments?instType=SWAP|FUTURES` (public) | inside `QueryAccount{Positions}` / `{OpenOrders}` | read before a positions or orders page that holds a contract whose `ctVal` is not known yet (the instrument load also teaches it); a contract the list does not name, or an inverse one (`ctVal` in USD), stays in contracts, valued by the instrument's multiplier |
| `fillsHistory` | `GET /api/v5/trade/fills-history?instType&begin&end&limit=100[&after]` (signed) | `QueryAccount{Fills}` | walks SPOT, MARGIN, SWAP, FUTURES, OPTION; pages by the last row's `billId`; 500 pages max; cursor `<book>:<after>:<pages>` |
| socket | `wss://ws.okx.com/ws/v5/business` (port 443: 8443 "will be retired soon", notice `64010`), `{"op":"subscribe","args":[{"channel":"candle1m","instId":..}]}` | `Subscribe{Candles}` | candles live on the business socket, not the public one |

## Field mapping

Candle row (all strings): `[ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm]`. `vol` is
the volume as OTW stores it: coins on spot, contracts on a derivative (a deliberate deviation
from "quantity in base units", to keep stored series unchanged). `confirm` `"0"` = forming.

Instrument row: display `base/quote` with `baseCcy` (else `ctValCcy`) and `quoteCcy` (else
`settleCcy`); `multiplier = ctVal x ctMult` (1 on spot); `settleCcy` as settlement currency;
`ctType = inverse` flags inverse contracts; `state = live` is active; `expTime` expiry.

Fill: `tradeId`, `ordId`, `fillSz`, `fillPx`, `fee` (OKX signs a charge negative: the sign
flips so a cost is positive and a rebate stays negative), `feeCcy`, `ts` (ms).

## Timeframes

`1m 5m 15m 1H 4H 1Dutc 1Wutc`. The daily and weekly bars are the UTC-anchored variants:
OKX's plain `1D` opens at 16:00 UTC.

## Auth

`OK-ACCESS-SIGN` = base64 HMAC-SHA256 of `timestamp + METHOD + path?query + body`, with
`OK-ACCESS-TIMESTAMP` in ISO-8601 to the millisecond, `OK-ACCESS-KEY` and
`OK-ACCESS-PASSPHRASE` (the passphrase chosen with the key). Signature checked against a
reference computed with an independent HMAC implementation. Secrets: `api_key`,
`api_secret`, `api_passphrase`. The host clock is used (no clock endpoint call), as before.
Demo trading (`env = demo`): same host, extra header `x-simulated-trading: 1`.

Hosts: REST `https://openapi.okx.com` (it replaced `www.okx.com`, per
[Production Trading Services](https://www.okx.com/docs-v5/en/#overview-production-trading-services)).
An account's `region` setting picks its host: empty or `global`, `eea` (`https://eea.okx.com`,
accounts registered on my.okx.com), `us` (`https://us.okx.com`, US and Australia, registered on
app.okx.com): "Using openapi.okx.com will not work for the regions below"
([Overview](https://www.okx.com/docs-v5/en/#overview)). Keyless market data stays on the global
host.

## Socket

Data frames name `arg.channel` and `arg.instId`: that pair routes them. A subscription
replays recent candles, so a row older than the newest seen on that channel is dropped. A
refusal (`event: error`) does not name its `arg`: the channel and instrument are read out of
the message text; a refusal naming neither is the socket's (reconnect). The venue drops a
socket silent for 30 s: the bare text `ping` goes out after 20 quiet seconds, `pong` is
ignored.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `0` | success | none |
| 50011, 50061 | rate limit | RateLimited |
| 50101 to 50105, 50111, 50113 | key, signature, timestamp, passphrase | Auth |
| 50114, 50120 | the key may not read this | Permission |
| 51001 | instrument does not exist | NotFound ("bad symbol") |
| 50001, 50004, 50013, 50026 | system busy, timeout | Transient |
| anything else | | InvalidRequest |

Most refusals come with HTTP 200: the envelope code decides, the status only when there is
no envelope.

## Account rules (shared bridge)

- Contract size and settlement currency come from the instrument list; a fill on an
  instrument it does not know falls back to the quote written in the id (`BTC-USDT-SWAP`).
- Positions and open orders of a linear swap or future are in the base coin (the order path's
  unit); fills stay in contracts with the instrument's multiplier (the journal's unit).
- Instrument ids are letters, digits, `-` and `_` (`BTC-USD_UM-SWAP`, the doc's instrument
  family table).
- An order's currency is the quote written in its id, as before.
- Holdings are valued on `<asset>-<quote>` for quote in USDT, USDC, USD, EUR, BTC, reversed
  too (`USDC-USDT` prices USDT at 1/price).

## Deliberate differences from the legacy connector

- A refused instrument on the history endpoint reads as a symbol problem in the chart's
  notice.
- An open order's time keeps its milliseconds (unchanged for OKX).
- Option contract sizes stay unknown (multiplier 1), as before: the instrument list for
  options needs an underlying.

## Regions and terms

OKX restricts some jurisdictions (including the US mainland); public market data answered
from the test machine. Demo trading needs its own key.

## Live best bid and ask

Channel `bbo-tbt` on the **public** socket (`/ws/v5/public`), its own socket class: the
business socket refuses it (60018, measured 2026-10-19). Rows `{asks, bids, ts}`, each side
`[price, size, 0, orders]`, on every change.

## Trading

Execution maturity: **E2D** (spot, perpetual swap and expiry futures books), built from the
documentation alone: no account, no demo key, no canary. Sandbox: OKX demo trading
(`env = demo`: the same REST host with the header `x-simulated-trading: 1` and a demo key; the
`wspap` socket hosts). Code: `trade.rs` (mapper), `trade_errors.rs` (error table); the order
path is the shared trader's (`connectors/engine/src/trading.rs`). Books: `Okx::spot()`,
`Okx::swap()`, `Okx::futures()` (`capabilities_spot.toml`, `capabilities_swap.toml`,
`capabilities_futures.toml`, each appended to `capabilities.toml`).

Doc snapshot: the single page https://www.okx.com/docs-v5/en/ read 2026-11-11 (HTTP
last-modified Wed, 11 Nov 2026 13:00:39 GMT, etag `W/"6af466f7-509646"`); the regional
editions https://my.okx.com/docs-v5/en/ (EEA) and https://app.okx.com/docs-v5/en/ (US and
Australia) read 2026-11-12 for their hosts. B below is https://www.okx.com/docs-v5/en/.

| Page | Used for |
| --- | --- |
| [Overview](https://www.okx.com/docs-v5/en/#overview) | "Regional API Domain Requirement": us.okx.com for US and AU users, eea.okx.com for EU users |
| [Production Trading Services](https://www.okx.com/docs-v5/en/#overview-production-trading-services), [Demo Trading Services](https://www.okx.com/docs-v5/en/#overview-demo-trading-services) | REST `https://openapi.okx.com`, sockets `wss://ws.okx.com/ws/v5/private`, demo `wss://wspap.okx.com`, header `x-simulated-trading: 1` |
| [EEA edition](https://my.okx.com/docs-v5/en/#overview-production-trading-services), [US edition](https://app.okx.com/docs-v5/en/#overview-production-trading-services) | REST `https://eea.okx.com` / `https://us.okx.com`; sockets `wseea` / `wsus`, demo `wseeapap` / `wsuspap` |
| [Notification](https://www.okx.com/docs-v5/en/#overview-websocket-notification) | `64010` port 8443 retired (sockets on 443), `64008` close announced 60 s ahead |
| [REST Authentication](https://www.okx.com/docs-v5/en/#overview-rest-authentication) | headers, signature `base64(HMAC-SHA256(timestamp + METHOD + requestPath + body))`, timestamp within 30 s (`50102`); `Content-Type: application/json` once per request (the signer's), `50006` otherwise |
| [WebSocket Login](https://www.okx.com/docs-v5/en/#overview-websocket-login), [Connect](https://www.okx.com/docs-v5/en/#overview-websocket-connect) | login frame, timestamp in seconds, sign over `timestamp + GET + /users/self/verify`; text `ping` before 30 s of silence |
| [General Info](https://www.okx.com/docs-v5/en/#overview-general-info) | 4,000 pending orders, 500 per symbol; a taker matching over 1,000 makers has its remainder cancelled; the `sCode` rule |
| [Transaction Timeouts](https://www.okx.com/docs-v5/en/#overview-transaction-timeouts) | `expTime` (not sent) |
| [Rate Limits](https://www.okx.com/docs-v5/en/#overview-rate-limits), [Sub-account rate limit](https://www.okx.com/docs-v5/en/#overview-rate-limits-sub-account-rate-limit), [Fill ratio based sub-account rate limit](https://www.okx.com/docs-v5/en/#overview-rate-limits-fill-ratio-based-sub-account-rate-limit) | `50011`, per instrument limits, 1,000 order requests per 2 s per sub-account (`50061`), "spot/margin orders are exempted from the sub-account rate limit", at most 3 amends in flight (`51513`) |
| [Place order](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order) | body, `tdMode` table, `clOrdId`, `posSide`, `ordType`, `sz`, `reduceOnly`, `tgtCcy`, `banAmend`, `slippagePct` (not sent, see doc gaps), `px` rounding (options only), STP |
| [Cancel order](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-cancel-order) | by `ordId`; "sCode equal to 0 ... only means that your cancellation request has been accepted" |
| [Cancel multiple orders](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-cancel-multiple-orders) | at most 20 per request, per-row `sCode`. Cancel all: the book's working orders are the plain ones (`orders-pending`) then its untriggered algo orders (`orders-algo-pending` per `ordType`, `chase` on contract books only); a batch of plain ids goes to `cancel-batch-orders`, a batch of algo ids to `cancel-algos` (10 at most), and what a batch call does not carry is cancelled on its own afterwards |
| [GET / Algo order list](https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-get-algo-order-list), [POST / Cancel algo order](https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-cancel-algo-order) | algo orders placed elsewhere listed in the orders panel and in the book's working orders (`ordType` required, "conditional and oco both can be used and separated with comma", `chase` "only applicable to FUTURES and SWAP", `instType`, `instId`, 100 a page, `after` the last `algoId`); a `cond:<algoId>` cancel goes to `cancel-algos` (`[{instId, algoId}]`, "A maximum of 10 orders can be canceled per request", 20 orders per 2 s). Placing an algo order from OpenTraderWorld is not declared. The doc's Python example says the call excludes iceberg, TWAP and trailing orders; the parameter table states no such limit: an exclusion shows as the venue's refusal |
| [Amend order](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-amend-order) | `newPx`, `newSz` (total target size), "less than or equal to the filled quantity ... changed to filled", acceptance only |
| [Close positions](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-close-positions) | read, not used (it closes the whole position only) |
| [Order details](https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-details) | lookup by `clOrdId` or `ordId`, the order fields, `state` words |
| [Order List](https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-list) | working orders by `instType`, paged `after` the last `ordId`, 100 a page |
| [Transaction details (last 3 months)](https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-transaction-details-last-3-months) | fills, `begin` / `end` on `ts`, paged `after` the last `billId`, `subType`, `fee` sign |
| [Mass cancel](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-mass-cancel-order), [Cancel All After](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-cancel-all-after) | not used (options in portfolio margin only; dead man's timer) |
| [Order channel](https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-order-channel) | `orders` frames, the update's `tradeId` / `fillSz` / `fillFee`, duplicate messages |
| [Positions channel](https://www.okx.com/docs-v5/en/#trading-account-websocket-positions-channel) | `positions` frames |
| [Get positions](https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions) | `pos`, `posSide`, `mgnMode`, `lever` |
| [Get account configuration](https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-account-configuration) | `posMode`, `acctLv` |
| [Get instruments](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-instruments) | trading rules, `ctVal`, `ctType`, `state` |
| [Pre-check order](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-order-precheck) | not used: "Only applicable to Multi-currency margin mode, and Portfolio margin mode" |
| [Error codes](https://www.okx.com/docs-v5/en/#error-code-rest-api-public) | error table (the 51256 to 51699 block of this page was read from the HTML, the text export had dropped it) |

### Order types and flags

| Canonical | Native | Notes |
| --- | --- | --- |
| `market` | `ordType=market` | spot: `tgtCcy=base_ccy` (the size in the base coin both ways: the default is the quote coin for a buy) and `banAmend=true` ("the entire order is rejected when balance is insufficient", never shrunk). `slippagePct` is not sent: "Maximum acceptable slippage for spot ... market-side orders, where tgtCcy is the received currency (base_ccy for buy ...). If not specified or empty, defaults to 0.00%", with no reference price and no outcome past the cap stated (doc gap): a base-coin market buy goes out under the venue's default. Contracts: "placed to order book with most aggressive price allowed by Price Limit Mechanism" |
| `limit` | `ordType=limit` with `px` | |
| `post_only` | `ordType=post_only` | GTC limit only. One that would take liquidity "will be canceled instead" (a cancel, not a refusal) |
| `reduce_only` | `reduceOnly=true` | contracts, net mode only ("Only applicable to ... FUTURES/SWAP orders in net mode"); in long/short mode "all closing orders apply the reduce-only feature", so the side closed is named (`posSide`) and the flag is not sent. Not in portfolio margin accounts (the venue refuses it) |
| `close_position` | a reduce-only market order for the size read now | the trader's close (whole or part); OKX's own `close-position` closes only the whole position and answers no `ordId`, so it is not used |
| `position_mode` | `posMode` (`net_mode`, `long_short_mode`) of the account configuration | read once per account, held, read again after `51000` or `51041`; never changed |
| margin | `tdMode` | spot `cash` always (never margin). Contracts: the held position's `mgnMode`, in long/short mode the row of the side the order acts on (`posSide`; each side carries its own), else `cross`: the doc's trade mode table lists cross for FUTURES/SWAP in Futures, Multi-currency and Portfolio margin modes. Read again after `50072` or `51043` |
| leverage | `lever` of the position | read only, never set |
| amend | `amend-order` by `ordId`, `newPx`, `newSz` | price and size; on a contract book the new size is sent in contracts of `ctVal`; a size at or under what filled is refused before I/O |
| not offered | | stop and trigger orders (`order-algo`: refused in net mode, `51298`; no trigger direction field), attached TP/SL (`attachAlgoOrds`: the doc does not say whether the TP and the SL cancel each other, nor the type and size of the generated algo), split and limit TPs, iceberg, TWAP, trailing, OCO, `optimal_limit_ioc`, MARGIN (borrowing), OPTION, EVENTS, inverse contracts |

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| `gtc` | `ordType=limit` (or `post_only`, or a market order: no TIF sent) | there is no GTD and no day order |
| `ioc` | `ordType=ioc` | a market order asked IOC is sent `market` (no TIF on a market order) |
| `fok` | `ordType=fok` | a market order FOK is refused before I/O; `stpMode=cancel_both` with `fok` is refused by OKX (`50016`, `54036`) |

### Client order id

`clOrdId`: "A combination of case-sensitive alphanumerics, all numbers, or all letters of up to
32 characters" (no `-`, no `_`). The OTW key is sent unchanged; a key outside that rule is
refused before I/O with the rule. Unique "among all currently pending (live or
partially_filled) orders"; a lookup by `clOrdId` answers "only the latest one". A duplicate
(`51016`, `50071`, and `51011` "Order ID already exists") is `Unknown`: looked up, never
resent. A lookup answered `51001` (the instrument no longer listed, a delivered future) is
`Rejected(not found)`.

### Order status

| Native | Canonical |
| --- | --- |
| `live` | `New` |
| `partially_filled` | `PartiallyFilled` |
| `filled` | `Filled` |
| `canceled` (an IOC or market remainder, a post-only that would cross) | `Cancelled` |
| `mmp_canceled` | `Cancelled` |
| any other word | unknown (alerts) |

OKX has no rejected state: a refusal is the place answer's `sCode`, and a refused order is
never queryable.

### Instrument trading rules

`GET /api/v5/public/instruments?instType=SPOT|SWAP|FUTURES` (public, the whole list of the
book, read once a day per instrument and after a rule refusal; the contract sizes once a day).
`tickSz` the tick; `lotSz` the size step ("All order quantities (sz) must be a multiple of
lotSz"); `minSz` the smallest size; `maxLmtSz` the largest limit order; contracts: `maxMktSz`
the largest market order. On a contract book every size is `ctVal` base coins per contract
(linear: "ctVal is in the base currency"), so the rules are stated in the base coin
(`lotSz x ctVal`, ...) and an order is sent `qty / ctVal` contracts. `state` `live` trades;
`suspend`, `rebase`, `preopen`, `test`, `settling` and `post_only` do not (held closed:
`post_only` "only post-only orders are accepted", OTW waits for `live`). Inverse contracts
(`ctVal` in USD) are listed closed, never sent an order; their rows are read with `ctVal` as
their size. No minimum order value is published (`51020`, `50122`, `51120` refuse it at the
venue). Spot `maxMktSz` is in USDT and `maxLmtAmt` / `maxMktAmt` in USD: not checked here.
An off-tick price on a non-option is checked here (the doc states rounding for options only).

### Errors

Most refusals come with HTTP 200. One order's result is `data[0].sCode` / `sMsg` / `subCode`
(read before the envelope `code`, which is `1` when the item failed); a sub-code maps as its
code (`51008_1000` is `51008`). A batch cancel is read row by row. A timeout, a 5xx or a
dropped connection is `Unknown`. A code not listed is unknown and alerts:
"OKX answered code X (msg), which OpenTraderWorld does not map". A refusal reads
"OKX refused it: msg (code X)".

| Code | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| `1`, `2` | envelope | Unknown | looked up by client id, never sent again |
| `50000`, `50002`, `50005`, `50006`, `50014`, `50015`, `50016`, `50024`, `50025`, `50036`, `50039`, `50072` | general | InvalidRequest | refused, the venue's words shown |
| `50001`, `50004`, `50013`, `50026`, `50042`, `50071` | general | Unknown | looked up by client id, never sent again |
| `50007`, `50008`, `50009`, `50010`, `50012`, `50017`, `50018`, `50019`, `50020`, `50021`, `50022`, `50023`, `50027`, `50028`, `50029`, `50032`, `50033`, `50037`, `50044`, `50045`, `50046`, `50047`, `50048`, `50049`, `50050`, `50051`, `50052`, `50053`, `50055`, `50056`, `50057`, `50058`, `50059`, `50060`, `50062`, `50063`, `50064`, `50067`, `50069`, `50075`, `50076`, `50077` | general | Rejected(other) | refused, the venue's words shown |
| `50011`, `50040`, `50061` | general | RateLimited | refused, not queued |
| `50030`, `50035`, `50038`, `50041` | general | Permission | refused, the venue's words shown |
| `50100`, `50101`, `50102`, `50103`, `50104`, `50105`, `50106`, `50107`, `50108`, `50109`, `50111`, `50112`, `50113`, `50114`, `50119` | API key | Auth | refused, the venue's words shown |
| `50110`, `50120`, `50121`, `50123` | API key | Permission | refused, the venue's words shown |
| `50115`, `50116`, `50118` | API key | Rejected(other) | refused, the venue's words shown |
| `50122` | API key | Rejected(venue_rule) | refused, rules read again |
| `51000`, `51001`, `51002`, `51003`, `51010`, `51015`, `51026`, `51041`, `51111` | orders | InvalidRequest | refused, the venue's words shown (`51001` on a lookup: not found) |
| `51004`, `51009`, `51012`, `51014`, `51018`, `51019`, `51024`, `51025`, `51032`, `51034`, `51035`, `51036`, `51037`, `51038`, `51039`, `51040`, `51042`, `51043`, `51044`, `51055`, `51056`, `51057`, `51058`, `51059`, `51065`, `51066`, `51068`, `51069`, `51070`, `51071`, `51072`, `51073`, `51074`, `51075`, `51076`, `51077`, `51078`, `51079`, `51080`, `51081`, `51082`, `51083`, `51084`, `51085`, `51086`, `51088`, `51089`, `51090`, `51091`, `51092`, `51093`, `51094`, `51095`, `51096`, `51098`, `51099`, `51100`, `51102`, `51103`, `51104`, `51105`, `51106`, `51107`, `51108`, `51109`, `51115`, `51117`, `51123`, `51128`, `51129`, `51130`, `51132`, `51140`, `51142`, `51144`, `51147`, `51152`, `51153`, `51154`, `51155`, `51156`, `51158`, `51159`, `51162`, `51163`, `51164`, `51165`, `51166`, `51167`, `51168`, `51173`, `51174`, `51175`, `51176`, `51177`, `51178`, `51179`, `51180`, `51181`, `51182`, `51186`, `51187`, `51192`, `51193`, `51194`, `51195`, `51204`, `51207`, `51209`, `51210`, `51211`, `51212`, `51213`, `51214`, `51220`, `51221`, `51222`, `51223`, `51224`, `51225`, `51226`, `51251`, `51252`, `51253`, `51254`, `51255` | orders | Rejected(other) | refused, the venue's words shown |
| `51005`, `51007`, `51020`, `51033`, `51101`, `51120`, `51121`, `51150`, `51185`, `51201`, `51202`, `51203` | orders | Rejected(venue_rule) | refused, rules read again |
| `51006`, `51031`, `51046`, `51047`, `51048`, `51049`, `51050`, `51051`, `51052`, `51053`, `51116`, `51122`, `51135`, `51136`, `51137`, `51138`, `51250` | orders | Rejected(price) | refused, the venue's words shown |
| `51008`, `51017`, `51127`, `51131`, `51143` | orders | Rejected(funds) | refused, the venue's words shown |
| `51011`, `51016`, `51054`, `51149` | orders | Unknown | looked up by client id, never sent again |
| `51021`, `51022`, `51027`, `51028`, `51029`, `51030`, `51087`, `51110`, `51124`, `51196`, `51197` | orders | Rejected(market closed) | refused, the venue's words shown |
| `51023`, `51112`, `51125`, `51126`, `51133`, `51134`, `51139`, `51148`, `51169`, `51170`, `51171`, `51205`, `51206` | orders | Rejected(reduce only) | refused, the venue's words shown |
| `51063` | orders | Rejected(not found) | cancel reconciled; lookup: not found |
| `51113` | orders | RateLimited | refused, not queued |
| `51256`, `51257`, `51260`, `51261`, `51262`, `51263`, `51264`, `51265`, `51267`, `51268`, `51269`, `51270`, `51271`, `51272`, `51273`, `51274`, `51275`, `51276`, `51281`, `51282`, `51283`, `51284`, `51285`, `51286`, `51287`, `51288`, `51289`, `51290`, `51291`, `51292`, `51293`, `51294`, `51295`, `51298`, `51299`, `51309`, `51310`, `51311`, `51312`, `51313`, `51317`, `51320`, `51321`, `51322`, `51323`, `51324`, `51325`, `51326`, `51327`, `51328`, `51329`, `51330`, `51331`, `51332`, `51333`, `51340`, `51341`, `51342`, `51343`, `51344`, `51345`, `51346`, `51347`, `51348`, `51349`, `51350`, `51351`, `51352`, `51353`, `51354`, `51355`, `51356`, `51357`, `51358`, `51359`, `51370`, `51380`, `51381`, `51382`, `51383`, `51384`, `51385`, `51386`, `51387`, `51388`, `51389`, `51390`, `51391`, `51392`, `51393`, `51394`, `51395`, `51396`, `51397`, `51398`, `51399` | algo, bot, iceberg and TWAP orders (never sent by OpenTraderWorld) | Rejected(other) | refused, the venue's words shown |
| `51258`, `51259`, `51277`, `51278`, `51279`, `51280`, `51300`, `51302`, `51303`, `51304`, `51305`, `51306`, `51307`, `51308` | algo, bot, iceberg and TWAP orders (never sent by OpenTraderWorld) | Rejected(price) | refused, the venue's words shown |
| `51400`, `51401`, `51402`, `51410` | cancel | Rejected(final) | cancel reconciled: the order's final state is shown |
| `51403`, `51405`, `51408`, `51413`, `51415`, `51416` | cancel | Rejected(other) | refused, the venue's words shown |
| `51404` | cancel | Rejected(market closed) | refused, the venue's words shown |
| `51406`, `51407`, `51409` | cancel | InvalidRequest | refused, the venue's words shown |
| `51411` | cancel | Permission | refused, the venue's words shown |
| `51412` | cancel | Unknown | looked up by client id, never sent again |
| `51500`, `51501`, `51512`, `51514` | amend | InvalidRequest | refused, the venue's words shown |
| `51502` | amend | Rejected(funds) | refused, the venue's words shown |
| `51503`, `51509`, `51510` | amend | Rejected(final) | cancel reconciled: the order's final state is shown |
| `51505`, `51506`, `51523`, `51524`, `51525`, `51526`, `51527`, `51528`, `51529`, `51530`, `51531`, `51532`, `51536`, `51537`, `51538`, `51539`, `51543` | amend | Rejected(other) | refused, the venue's words shown |
| `51507`, `51508` | amend | Rejected(market closed) | refused, the venue's words shown |
| `51511` | amend | Rejected(post only) | refused, the venue's words shown |
| `51513` | amend | RateLimited | refused, not queued |
| `51521`, `51522` | amend | Rejected(reduce only) | refused, the venue's words shown |
| `51600`, `51601`, `51602` | order queries and downloads | InvalidRequest | refused, the venue's words shown |
| `51603` | order queries and downloads | Rejected(not found) | cancel reconciled; lookup: not found |
| `51604`, `51605`, `51606`, `51607`, `51608`, `51610`, `51611`, `51620`, `51621`, `51625` | order queries and downloads | Rejected(other) | refused, the venue's words shown |
| `51784` | client order id (listed in the Finance class, 51700 to 51799) | Unknown | looked up by client id, never sent again |
| `51820`, `51821`, `51822`, `51823`, `51824`, `51825` | orders (quotes and payment) | Rejected(other) | refused, the venue's words shown |
| `52000` | public data | Rejected(other) | refused, the venue's words shown |
| `54000`, `54001`, `54004`, `54005`, `54006`, `54007`, `54008`, `54012`, `54024`, `54025`, `54026`, `54027`, `54028`, `54030`, `54031`, `54036`, `54039`, `54040`, `54041`, `54042`, `54043`, `54044`, `54045`, `54046`, `54051`, `54070`, `54071`, `54073`, `54074`, `54075`, `54078`, `54079`, `54092`, `54094`, `54109` | orders (54000 to 54109) | Rejected(other) | refused, the venue's words shown (`54005`: pre-market futures trade isolated only) |
| `54009`, `54029` | orders (54000 to 54109) | InvalidRequest | refused, the venue's words shown |
| `54011`, `54035` | orders (54000 to 54109) | Rejected(reduce only) | refused, the venue's words shown |
| `54018`, `54019` | orders (54000 to 54109) | Rejected(venue_rule) | refused, rules read again |
| `54047`, `54048`, `54049` | orders (54000 to 54109: speed bump, system busy) | RateLimited | refused, not queued |
| `54072` | orders (54000 to 54109) | Rejected(market closed) | refused, the venue's words shown |

### Rate limits for orders

Place, cancel and amend: 60 requests per 2 s each, per user and instrument; batch cancel 300
orders per 2 s; cancel algos 20 orders per 2 s; at most 1,000 new and amended orders per 2 s
per sub-account (`50061`), from which "spot/margin orders are exempted"; at most 3 amends in
flight per order (`51513`). Lookups 60 per 2 s, pending orders 60 per 2 s, algo order list 20
per 2 s, fills-history 10 per 2 s, account configuration 5 per 2 s, positions 10 per 2 s,
public instruments 20 per 2 s per IP and instrument type. Declared in `capabilities.toml`: 20
requests per 2 s for the whole connector, and `scope = "endpoint"` limits under it for
positions (10), account configuration (5) and fills-history (10, both endpoint ids); the
per-instrument limits are not expressed (the shared 20 per 2 s is under each). A rate limit
is `50011` (HTTP 200 or 429) and states no reset time (doc gap): `RateLimited`, with the wait
of a `Retry-After` header only where the answer carries one. IP ban rules: not documented.

### Stream

`wss://ws.okx.com/ws/v5/private` (EEA `wseea`, US `wsus`; demo `wspap`, `wseeapap`,
`wsuspap`), one socket per book: login frame (signer), then `orders` with the book's
`instType`, and `positions` on a contract book, one subscribe frame each. A text `ping`
every 20 s ("send the String 'ping'" before 30 s; `pong` back). The `orders` channel pushes no
snapshot: what happened while away is read from the book (`Resync`). An update with a
`tradeId` carries that fill (`fillSz`, `fillPx`, `fillFee`); a fill of a liquidation, ADL or
delivery (`category`, negative `tradeId`) reads the book. Duplicate messages are possible (the
doc's own warning): orders are applied by state, fills dropped by `tradeId`. `notice` `64008`
reconnects. The `business` socket carries algo orders only (`orders-algo`): none are sent, so it
is not opened. The `fills` channel is VIP4 and above only: not used.

### Disconnect behaviour

OKX keeps working orders when the client goes away. `cancel-all-after` (a timer of 10 to 120 s)
exists and is not armed: `cancel_on_disconnect` is not declared.

### Dry-run

None declared. `order-precheck` is "Only applicable to Multi-currency margin mode, and
Portfolio margin mode"; nothing exists for Spot or Futures mode accounts. The E2D session
check is skipped on these books.

### Quirks

- Live, 2026-11-12 (public conformance, no key): spot lists 1232 instruments (1228 trading),
  swap 543 (528), futures 291 (279), all trading ones with sound rules; a `preopen` listing
  states no tick or size step. An order signed with a key OKX never issued answers `50111`
  (Auth), an unsigned one `50103` (Auth).
- One instrument id, two books: SPOT and MARGIN share `BTC-USDT`, the trade mode picks the
  book; OTW sends `cash` only.
- Fees: OKX signs a charge negative and a rebate positive; the canonical fee is a cost, so the
  sign flips.
- Fills: `subType` 1 to 6 are an order's own trades; any other (liquidation, ADL, delivery,
  block trade, move position, conversion) moves a position and is kept, marked
  `position_only`.
- A spot market order sized in the quote coin placed outside OTW (`tgtCcy=quote_ccy`) is read
  with its filled size as its size (its `sz` is an amount of the quote coin).
- The amend answer's documented example is not valid JSON (no comma after `sMsg`): the answer
  is not read (the call's success is its envelope and item code).
- Demo trading: "some functions are not supported"; `instIdCode` "may be different between
  production and demo". No other difference is documented.
- The orders panel's ids are `BOOK:SYMBOL:ordId`; the book is told from the instrument id
  (`-SWAP` swap, a dated third part futures, two parts spot), as the doc's naming table writes
  them.

### Doc gaps

- Whether a place `sCode` 0 means accepted only (it is said for cancel and amend).
- What becomes of an unfilled remainder of a contract market order.
- Retention of a lookup by `clOrdId`; canceled orders with no fill stay 2 hours in the 7-day
  history.
- Off-tick price on a non-option: refused or rounded.
- `slippagePct` on a spot market buy sized in the base coin: the price the slippage is
  measured from, and what happens past the cap (refused, cut short, cancelled), are not
  stated; its default is 0.00%. Not sent: a base-coin market buy goes out under that default.
- Minimum order value.
- Rate-limit reset time and IP ban rules.
- The order of partial fill events; fill corrections or busts (T25).
- Whether `cash` is accepted for spot in Multi-currency and Portfolio margin modes (the trade
  mode table lists `cross` only).
- `ctMult`'s part in the contract size (the notional formula uses `ctVal` alone): the order path
  and the account read use `ctVal`, the instrument list's multiplier is `ctVal x ctMult`.
- Signature test vectors: none with a result (an independent HMAC is used).
- Demo versus live differences.
- Attached TP/SL: behaviour after a partial fill and cancel, the type and size of the generated
  algo, whether TP and SL cancel each other (why attached exits are not offered).
- Trigger direction when there is no `orderPx`, and whether a conditional algo can open a
  position (why stops are not offered).
