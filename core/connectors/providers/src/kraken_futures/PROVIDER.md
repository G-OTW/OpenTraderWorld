# Kraken Futures (Kraken Derivatives)

Adapter version: 0.1.0        Provider API version: derivatives REST `v3`, history REST `v3`        Maturity: M1

Sources (snapshot 2026-11-12):
- OpenAPI, derivatives: https://docs.kraken.com/openapi/futures-rest.yaml (last-modified 2026-11-09)
- OpenAPI, history: https://docs.kraken.com/openapi/futures-history-rest.yaml (last-modified 2026-09-08)
- Introduction, demo host: https://docs.kraken.com/exchange/guides/futures/introduction
- REST authentication: https://docs.kraken.com/exchange/guides/futures/rest
- Rate limits: https://docs.kraken.com/exchange/guides/futures/ratelimits
- Socket, challenge signing: https://docs.kraken.com/exchange/guides/futures/websockets

M1: every declared read has a doc fixture (or a tagged synthetic one where the doc has no
example), unit tests and a mock-venue golden; junk-key conformance against the live host.
Never run against a funded account.

No market data is declared (Kraken spot keeps the data connector). Orders on the
multi-collateral contracts: "Trading" below.

## Endpoints

| Endpoint id | Method + path | Used for | Cost | Paging |
| --- | --- | --- | --- | --- |
| `instruments` | `GET /derivatives/api/v3/instruments` (public) | `LoadInstruments` | none | none |
| `contracts` | the same | session contract list for account rows (kept one hour) | none | none |
| `expired` | `GET /derivatives/api/v3/instruments?expired=true` (public) | asked only when a row names a contract the current list lacks (matured) | none | none |
| `accounts` | `GET /derivatives/api/v3/accounts` (signed) | `QueryAccount{Summary}`, `{Balances}` | 2 | none |
| `tickers` | `GET /derivatives/api/v3/tickers` (public) | marks for positions (`markPrice`) | none | none |
| `openpositions` | `GET /derivatives/api/v3/openpositions` (signed) | `QueryAccount{Positions}` | 2 | none |
| `openorders` | `GET /derivatives/api/v3/openorders` (signed) | `QueryAccount{OpenOrders}` | 2 | none |
| `executions` | `GET /api/history/v3/executions?since&before&sort=asc&count=1000[&continuation_token]` (signed) | `QueryAccount{Fills}` | 1 history token | `continuationToken` in the body, else the `Next-Continuation-Token` header, 1000 a page, 500 pages max |

`/fills` (derivatives) is not used: it carries no fee and pages 100 at a time backwards by
`lastFillTime` (cost 25 per page).

## Auth

Headers `APIKey`, `Nonce`, `Authent`. `Authent` = base64 `HMAC-SHA512(base64-decoded secret,
SHA256(postData ++ nonce ++ endpointPath))`:
- `postData`: the query string as sent (url-encoded, "Upcoming Changes" in the REST guide), or
  the body of a POST.
- `endpointPath`: the path without `/derivatives` (doc example `/api/v3/orderbook`); a history
  path is hashed whole (`/api/history/v3/executions`).
- `Nonce`: optional in the doc; sent, from a process-wide strictly increasing counter seeded
  by the clock in milliseconds.

The socket's signed challenge is the same construction over the challenge alone; the doc's
worked example (challenge `c100b894-...`, signed `4JEpF3ix...3A==`) is the byte test of this
signer. The REST guide's own sample secret is not valid base64 (87 characters), so the REST
inputs are checked with the socket example's secret (synthetic reference, computed with
Python). Secrets: `api_key`, `api_secret`; a secret that is not base64 is refused before any
call. Futures keys are made on the futures site, not the spot one.

## Environments

`env = demo` sends every read to `https://demo-futures.kraken.com` (the doc: "identical to
production ... The only difference is the base URL"). A demo account with no demo driver is
refused, never sent live (pinned by `a_kraken_futures_demo_account_never_reads_live`).
Measured 2026-11-12 from the test machine: `demo-futures.kraken.com` answers HTTP 301 to
`www.kraken.com/gb/features/futures`, so the demo may be closed or region-gated.

## Rate limits

- `/derivatives`: 500 cost units per 10 s; `accounts`, `openpositions`, `openorders` cost 2.
  Over it: HTTP 200 `{"result":"error","error":"apiLimitExceeded"}` (RateLimited).
- `/history`: a pool of 100 tokens refilled 100 every 10 minutes; `executions` costs 1.
  Declared as an endpoint bucket `executions` of 100 refilled over 600 000 ms; every history
  read counts in it (the order lookups' ids `executions:lookup_orders` and
  `executions:lookup_executions` too, counted in the bucket their part before `:` names). Over
  it: HTTP 429 text with `rate-limit-reset` (seconds), read as the retry delay.

## Contracts and units

The list's `type` decides:

| `type` | Prefixes seen | Sizing | Read as |
| --- | --- | --- | --- |
| `flexible_futures` | `PF_` (perpetual), `FF_` (dated) | base coin, `contractSize` 1, step `10^-contractValueTradePrecision` | coins = size × contractSize |
| `futures_inverse` | `PI_`, `FI_` (and old `F-`, `T-`, `TI_`) | USD contracts, settled in the coin | positions and orders: coins = size × contractSize / price (entry, else limit, else stop); fills not imported |
| `futures_vanilla`, `options` | `FV_`, `PV_`, `OF_` | doc silent | left out with a warning |

Evidence the `PF_` size is the coin: the socket example's `PF_XBTUSD` position `balance: 0.04`
at 26911.75. Doc silent on what one inverse contract is worth; `contractSize: 1` with quote USD
read as 1 USD (as on Bybit and Deribit inverse). Dated: `lastTradingTime` present (Future),
else Perp. `active` = not `suspended`, not `isExpired`, `tradeable`. `maxPositionSize` is the
max quantity rule. The live list (2026-11-12, this machine) held only `PF_` and `FF_`; the
expired list (2.9 MB) holds the inverse ones, hence it is read only on demand.

Numbers are JSON numbers, sometimes in exponent form (`0E-20`), read from their text.

## Account rows

- Balances: `flex` currencies (`quantity`, `available`), the `cash` wallet's balances, and a
  `marginAccount`'s coin balances (its contract keys, with an `_`, left out). Each row names its
  wallet. Fiat codes (USD, EUR, GBP, CHF, CAD, AUD, JPY) are filed as fiat. Codes kept as the
  venue writes them, upper-cased (`XBT`).
- Summary: the same `/accounts`, with the flex wallet's `portfolioValue` and `availableMargin`.
- Positions: `side` long or short, `size`, `price` (entry), `unrealizedPnl` carried only when
  `pnlCurrency` is absent or the contract's quote (an inverse row's never). Leverage read only:
  `maxFixedLeverage` (isolated) or `crossMaxLeverage` (cross) on the row, written in its label.
  Mark from `/tickers`; a failed tickers call leaves the mark out.
- Open orders: limit and trigger orders in one list and one id space (`order_id`, a UUID that
  `/cancelorder` takes): no `cond:` tag. `orderType` kept verbatim (`lmt`, `stop`,
  `take_profit`; the doc's description also says `stp`). qty = `filledSize` + `unfilledSize`.
  `receivedTime` RFC 3339.
- Fills: `elements[].event.execution.execution`: `uid` (fill id), `order.uid`,
  `order.tradeable`, `order.direction` (`Buy`/`Sell`), `timestamp` ms, `quantity`, `price`,
  `orderData.fee`. The doc names no fee currency and no sign convention on an execution: the
  fee is carried as written, its currency left empty. (The socket `fills` feed says
  `fee_currency` and signs a maker rebate negative.)

## Enum tables

| Kraken | Canonical |
| --- | --- |
| side `buy` / `sell`, direction `Buy` / `Sell` | `Side::Buy` / `Side::Sell`; anything else `Unknown(raw)` |
| position side `long` / `short` | sign of the quantity; anything else is a decode error |
| open order `status` `untouched`, `partiallyFilled` | working (not mapped further on the read path) |

## Errors (read path)

| Answer | Canonical |
| --- | --- |
| `"result":"success"` | none |
| `authenticationError`, `nonceBelowThreshold`, `nonceDuplicate` | Auth (nonce ones say: give OTW a key of its own) |
| `apiLimitExceeded`, history HTTP 429 | RateLimited |
| `accountInactive`, `cashAccountTypeAccessForbidden`, coded 403 `95` | Permission |
| `notFound`, `accountNotFound`, `cashAccountTypeNotFound` | NotFound |
| `Server Error`, `Unavailable`, `unknownError`, `marketUnavailable`, HTTP 5xx, timeout | Transient |
| history HTTP 401 `{"status":"unauthorized"}` | Auth |
| any other word (`invalidArgument`, `requiredArgumentMissing`, `Json Parse Error`, `cancelOnly`...) | InvalidRequest |

## Trading

Execution maturity: **E2D**, built from the documentation alone: no account, no demo key, no
canary. Sandbox: the demo platform (`env = demo`, `https://demo-futures.kraken.com`, its own
sign-up and keys); measured 2026-11-12 from the test machine it answers HTTP 301 to
`www.kraken.com/gb/features/futures` (closed or region-gated), so no demo run was possible.
Code: `trade.rs` (mapper), `trade_errors.rs` (error table); the order path is the shared
trader's (`connectors/engine/src/trading.rs`). Book: the multi-collateral contracts
(`flexible_futures`: `PF_` perpetuals, `FF_` dated), one book `futures` in otw-core.

Doc snapshot read 2026-11-12 and checked against the live files the same day (same sha256):
Y = https://docs.kraken.com/openapi/futures-rest.yaml (last-modified Mon, 09 Nov 2026 11:24:12
GMT, sha256 `6c7e8c44...898d66d`), H = https://docs.kraken.com/openapi/futures-history-rest.yaml
(last-modified Tue, 08 Sep 2026 12:38:35 GMT, sha256 `fbc0536d...f473078`), and the guides (G)
under https://docs.kraken.com/exchange/guides/futures/.

| Page | Used for |
| --- | --- |
| [Y `/sendorder`](https://docs.kraken.com/api-reference/order-management/send-order) | `orderType` (`lmt`, `post`, `ioc`, `mkt` "an immediate-or-cancel order with 1% price protection", `stp`, `take_profit`, `trailing_stop`, `fok`), `symbol`, `side`, `size`, `limitPrice` ("If no limitPrice is provided the stp or take_profit order will trigger a market order"), `stopPrice`, `cliOrdId` (maxLength 100, "must be globally unique"), `triggerSignal` (`mark`, `index`, `last`), `reduceOnly`; `sendStatus.status` enum and description; examples `placed`, `rejected`, `executed` |
| [Y `/editorder`](https://docs.kraken.com/api-reference/order-management/edit-order) | `orderId`, `size`, `limitPrice`, `stopPrice`, `qtyMode` ("'ABSOLUTE' means that the total order size, including past fills, is set to size", default RELATIVE); `editStatus.status` |
| [Y `/cancelorder`](https://docs.kraken.com/api-reference/order-management/cancel-order) | `order_id`; `cancelStatus.status` `cancelled`, `filled`, `notFound` |
| [Y `/cancelallorders`](https://docs.kraken.com/api-reference/order-management/cancel-all-orders) | "If no arguments are specified all open orders will be cancelled"; `cancelled` / `noOrdersToCancel` |
| [Changelog 2026-11-12](https://docs.kraken.com/exchange/changelog) | cancel all "now returns a CANCEL event in orderEvents for each trigger order it cancels ... status stays noOrdersToCancel when only trigger orders were cancelled" |
| [Y `/orders/status`](https://docs.kraken.com/api-reference/order-management/get-specific-orders-status) | `orderIds`, `cliOrdIds`; "orders which are open or were filled/cancelled in the last 5 seconds"; `OrderStatusJson` words; `CachedOrderJson` (`type` `ORDER` / `TRIGGER_ORDER`, `priceTriggerOptions.triggerPrice`) |
| [Y `/openorders`](https://docs.kraken.com/api-reference/order-management/get-open-orders) | limit and trigger orders in one list, `cliOrdId`, `status` `untouched` / `partiallyFilled` |
| [Y `/openpositions`](https://docs.kraken.com/api-reference/account-information/get-open-positions) | `side` long or short, `size`, `maxFixedLeverage`, `crossMaxLeverage` |
| [Y `/instruments`, `/trading/instruments`](https://docs.kraken.com/api-reference/instrument-details/get-instruments) | `contractType`, `tickSize`, `contractSize`, `contractValueTradePrecision`, `postOnly`, `suspended`, `isExpired`, `tradeable`, `maxPositionSize`, `makerProtectionMillis`; `minimumTradeSize` "Not populated for any markets" |
| [H `/executions`](https://docs.kraken.com/api-reference/account-history/get-execution-events) | fills: `since`, `before`, `sort`, `count` ("silently capped at 1000"), `continuation_token`, `tradeable`; `HistoricalExecution` (`uid`, `order`, `quantity`, `price`, `orderData.fee`) |
| [H `/orders`](https://docs.kraken.com/api-reference/account-history/get-order-events) | lookup: `OrderCancelled`, `OrderRejected` events and their `HistoricalOrder` (`clientId`, `filled`) |
| [G REST](https://docs.kraken.com/exchange/guides/futures/rest) | "Calls to endpoints that do change the state of the server should be submitted with request type POST or PUT and with request parameters submitted in the request body"; `Authent` over `postData`; "if a call comes back with result equal to success, this merely means that the request has been received" |
| [G Rate limits](https://docs.kraken.com/exchange/guides/futures/ratelimits) | 500 every 10 s; costs `sendorder` 10, `editorder` 10, `cancelorder` 10, `cancelallorders` 25, `openorders` 2, `openpositions` 2, `orders/status` 1; history 100 tokens per 10 minutes |
| [G Maker protection](https://docs.kraken.com/exchange/guides/futures/maker-protection) | REST "connection is simply held for the duration of the delay"; "The window can never exceed 100 ms"; a cancel of a filled held order answers `notFound` |
| [G Introduction](https://docs.kraken.com/exchange/guides/futures/introduction) | demo host, "The only difference is the base URL"; key access levels (General API: Full Access to trade) |
| [G WebSocket](https://docs.kraken.com/exchange/guides/futures/websockets) | the challenge login (not used: see Stream) |

### Order types and flags

| Canonical | Native | Notes |
| --- | --- | --- |
| `market` | `orderType=mkt` | "an immediate-or-cancel order with 1% price protection": may end part filled, the rest cancelled |
| `limit` | `orderType=lmt` with `limitPrice` | |
| `post_only` | `orderType=post` | a GTC limit only; one that would take liquidity is refused (`postWouldExecute`) |
| `stop` | `orderType=stp`, `stopPrice`, no `limitPrice` | "will trigger a market order" |
| `stop_limit` | `orderType=stp`, `stopPrice`, `limitPrice` | "limitPrice denotes the worst price at which the stp ... can get filled at" |
| `mit` | `orderType=take_profit`, `stopPrice`, no `limitPrice` | take profit at the market |
| `lit` | `orderType=take_profit`, `stopPrice`, `limitPrice` | |
| triggers | `triggerSignal` `mark`, `last`, `index` | sent only when the ticket names one; the doc states no default (doc gap), the venue's then applies |
| `reduce_only` | `reduceOnly=true` | "Any order which increases an existing position will be rejected" (`wouldNotReducePosition`) |
| `close_position` | a reduce-only `mkt` order for the size read now | the trader's close (whole or part) |
| `position_mode` | one position per contract (`openpositions` `side` long or short) | the doc has no hedge mode: one-way, read once per account with Get open positions (which also proves the key reads), never changed |
| leverage | `maxFixedLeverage` (isolated) or `crossMaxLeverage` (cross) of the position row; neither is cross with no cap (`crossMaxLeverage` "Absent when the position is uncapped or isolated") | read only, shown; an order carries none (leverage is per contract at Kraken, `PUT /leveragepreferences`, never called) |
| amend | `editorder` by `orderId`, `qtyMode=ABSOLUTE` always | size (the new total, above what filled), limit price (an order with one), trigger price (a trigger order); never the time in force |
| not offered | | `trailing_stop` and relative limits (`limitPriceOffsetValue`), batch orders, attached exits (none on `/sendorder`), `processBefore`, `cancelallordersafter`, inverse and vanilla contracts (`PI_`, `FI_`, `FV_`, `PV_`, sized in USD contracts) and options: refused before I/O |

Sizes: the order's size in the base coin, sent as is: a multi-collateral contract is one coin
(`contractSize` 1; the socket's `PF_XBTUSD` position example shows `balance: 0.04` at
26911.75). A listed contract whose `contractSize` is not 1 is held closed, never sent an order.
Every quantity shown on the read path is in the same coin. The order path's open orders on an
inverse contract (cancel all sees them) are in coins too, `size × contractSize / price` at the
order's limit, else its trigger, as on the read path; the contract list read for the rules
carries the inverse contracts' `contractSize` for that.

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| `gtc` | `lmt`, `post`, `stp`, `take_profit` | no GTD (`processBefore` is a submission deadline, not a TIF) and no day order |
| `ioc` | `ioc` | a market order asked IOC is sent `mkt` (already immediate) |
| `fok` | `fok` | a market order FOK, a stop or take profit IOC or FOK are refused before I/O |

### Client order id

`cliOrdId`: maxLength 100, "It must be globally unique" (no window stated). The REST doc names
no charset (doc gap; only the FIX gateway says "UUIDv4 required for ClOrdID"): letters, digits
and `-` are sent, the OTW key unchanged; a key outside that rule is refused before I/O.
`clientOrderIdAlreadyExist` is `Unknown` (looked up, never resent).

### Order status

| Native | Canonical |
| --- | --- |
| `orders/status` `ENTERED_BOOK` | `New`, `PartiallyFilled` when `filled` is above zero |
| `TRIGGER_PLACED` | `New` (a trigger waiting) |
| `FULLY_EXECUTED` | `Filled` |
| `CANCELLED` | `Cancelled` |
| `REJECTED`, `TRIGGER_ACTIVATION_FAILURE` | `Rejected` |
| `openorders` `untouched` / `partiallyFilled` | `New` / `PartiallyFilled` |
| history: `OrderCancelled` / `OrderRejected` | `Cancelled` / `Rejected` |
| history: the order's executions on a page, or its own `filled` on one, reach its `quantity` | `Filled` |
| history: executions short of its `quantity` | `Cancelled`, with what filled |
| any other word | unknown (alerts) |

Lookup by client id (a lost answer, a restart): `POST /orders/status` `cliOrdIds` first (open, or
ended in the last 5 seconds); then the history API newest first, from a minute before the order
was made (`since`, the trader's time anchor), on the order's contract: its `OrderCancelled` /
`OrderRejected` event (with what filled), then its executions (`order.clientId`). An order the
live call did not list and the order history names no end of is not open, so what filled is
the most of its executions on the page added up and its own `filled` as an execution carries
it (its executions may run over several pages); short of its size it ended part filled. An order
cancelled unfilled and older than the history pages read, or a trigger order cancelled before it
fired (its events are on `/triggers`, not read), is not found: the reconciliation settles it as
not received. Lookup by venue id: the same with `orderIds`, over the same 3 pages of each
history call.

### Instrument trading rules

`GET /derivatives/api/v3/instruments` (public, the whole list of every type, read once a day per
contract and after a rule refusal; an inverse contract's `contractSize` is kept for its open
orders, see Sizes above). `tickSize` the tick;
`10^-contractValueTradePrecision` the size step (negative precision: steps of 10, 100...). No
smallest size (`minimumTradeSize` is "Not populated for any markets", on the signed list only)
and no largest order size (`maxPositionSize` bounds a position): the venue checks both. State:
`suspended`, `isExpired`, not `tradeable` held closed; `postOnly` takes post-only limits only;
any type but `flexible_futures` or a symbol outside `PF_` / `FF_` closed.

### Errors

Two places (G REST, Returns): the envelope (`"result":"error"`, the word in `error`, or
`errors[]` with `{code, message}` on the 403) refuses the request; a request read answers
`"result":"success"` and the order's own refusal is its status word (`sendStatus.status`,
`editStatus.status`, `cancelStatus.status`). Success words: Send order `placed`,
`partiallyFilled`, `filled`, `cancelled`, `edited`; Edit order `edited`, `filled`; Cancel order
`cancelled`; Cancel all `cancelled`, `noOrdersToCancel`. A timeout, a dropped connection or an
HTTP 5xx is `Unknown` (looked up, never resent). On Edit order, `postWouldExecute` ("thus is
cancelled"), `iocWouldNotExecute` ("the part that did not fill was removed from the book"),
`selfFill`, `insufficientAvailableFunds` and `invalidSize` ("The order was not placed", which
does not say whether the order still works) are `Unknown`: the order is read again once, by its
venue id, right after the answer. Ended, the change is refused naming the state it ended in;
still working unchanged, the change is refused with Kraken's word; working with the values
asked, the change went through; not found, it stays unknown and is read again before it is
changed or cancelled. A word not listed is unknown and alerts:
"Kraken Futures answered code X (msg), which OpenTraderWorld does not map". A refusal reads
"Kraken Futures refused it: words (code X)".

| Code | Canonical | Meaning | Action |
| --- | --- | --- | --- |
| `Server Error`, `unknownError`, `Unavailable` | Unknown | the request may have been processed | looked up by client id, never sent again |
| `marketUnavailable` | Unknown | "The market is currently unavailable": not said whether it was received | looked up, never sent again |
| `clientOrderIdAlreadyExist` | Unknown | the client id is in use: the order may be there | looked up, never sent again |
| `authenticationError`, `nonceBelowThreshold`, `nonceDuplicate` | Auth | key, signature or nonce refused | nothing placed |
| `accountInactive`, `cashAccountTypeAccessForbidden`, `95`, `invalidDomainConfiguration` | Permission | account or key not allowed | nothing placed |
| `apiLimitExceeded` | RateLimited | 500 per 10 s spent | refused, not queued; a `Retry-After` is read when sent (the doc states no wait) |
| `invalidArgument`, `requiredArgumentMissing`, `Json Parse Error`, `accountNotFound`, `cannotTransferBonusCurrency`, `cashAccountTypeNotFound`, `invalidAccount`, `invalidAmount`, `invalidCurrency`, `invalidUnit`, `clientOrderIdTooBig`, `clientOrderIdTooLong`, `clientOrderIdInvalid`, `orderForEditNotAStop`, `cannotEditTriggerPriceOfTrailingStop`, `cannotEditLimitPriceOfTrailingStop`, `invalidOrderType`, `invalidSide` | InvalidRequest | the request is malformed | refused, the venue's words shown |
| `insufficientFunds`, `insufficientAvailableFunds`, `wouldCauseLiquidation` | Rejected(funds) | margin | refused |
| `marketSuspended`, `marketInactive`, `invalidPrice`, `invalidSize`, `marketIsPostOnly` | Rejected(venue_rule) | the contract's rules or state | refused, the rules read again |
| `cancelOnly`, `marketIsCancelOnly` | Rejected(market closed) | the venue takes cancels only (also HTTP 409) | refused |
| `outsidePriceCollar`, `wouldIncreasePriceDislocation` | Rejected(price) | price band | refused |
| `postWouldExecute` | Rejected(post only) | a post-only order would take liquidity | refused |
| `wouldNotReducePosition` | Rejected(reduce only) | a reduce-only order would add | refused |
| `notFound`, `orderForEditNotFound` | Rejected(not found) | the order is not known (a cancel of a filled order answers it too) | the order is read again: a final state is no error |
| `filled` | Rejected(final) | on a cancel: "found completely filled and could not be cancelled" | the order is read again, no error |
| `tooManySmallOrders`, `maxPositionViolation`, `iocWouldNotExecute`, `selfFill`, `tooManyOrders`, `makerProtectionOrderLimitExceeded`, `fixedLeverageTooHigh`, `wouldProcessAfterSpecifiedTime`, `editAlreadyHeld`, `transferFailed` | Rejected(other) | the venue's words | refused |

The history API refuses with HTTP 401 `{"status":"unauthorized"}` (Auth) and HTTP 429 with
`rate-limit-reset` in seconds (RateLimited, the wait read). Any other 4xx without an envelope
is InvalidRequest, a 403 Permission.

### Rate limits for orders

`/derivatives`: 500 cost units every 10 s (G Rate limits); `sendorder`, `editorder`,
`cancelorder` 10 each, `cancelallorders` 25, `openorders`, `openpositions` 2, `orders/status`
1, each request carrying its cost as its weight (declared as an account bucket of 500 over
10 000 ms). Over it: `apiLimitExceeded`, the order refused, never queued. History: 100 tokens
refilled 100 every 10 minutes, 1 per page, the lookups' history reads included; a lookup by
client id or by venue id reads at most 3 pages of each history call. No ban is documented. The
order path's history decoders page by the body's `continuationToken`, else the
`Next-Continuation-Token` header, as the account read does.

### Stream

None: polled only. The private socket (`wss://futures.kraken.com/ws/v1`, demo
`wss://demo-futures.kraken.com/ws/v1`) logs in by a round trip: OTW sends a `challenge`
request, the venue answers a UUID, and every private `subscribe` must carry
`original_challenge` and `signed_challenge` (G WebSocket). The shared layer opens a socket with
one login frame and subscribe frames fixed before it opens, so it cannot build them from the
venue's answer. The account is reconciled by polling (open orders, positions, executions).

### Disconnect behaviour

Working orders stay working: OTW does not arm `cancelallordersafter` (the dead man's switch).

### Dry-run

None: the doc has no validate or test-order endpoint (`processBefore` is a deadline, not a
check). The E2D session check of the venue's own cannot run: an untested-venue account sends
its first order without it.

### Quirks

- Maker protection holds a liquidity-taking REST call for the market's window, which "can never
  exceed 100 ms": far under the transport's timeout. A cancel of a held order answers when it
  releases; `notFound` if it filled whole, read again as above.
- A cancel of an order that filled answers `notFound` (G Maker protection, "exactly as a cancel
  chasing a filled unheld order is"), not only `filled`: both are read again, never an error.
- Cancel all covers trigger orders (changelog 2026-11-12); what it leaves working is cancelled
  one by one.
- The nonce is optional and the venue "tolerates nonces that are out of order for a brief period
  of time" (G REST): signed calls are not serialized per key.

### Doc gaps

- Fee currency and sign on a history execution (`orderData.fee`): not stated. The fee is carried
  as written and its currency left empty (the socket's `fills` feed states `fee_currency` and
  signs a rebate negative, but the socket is not used). The account log (`/account-log`) carries
  a `fee` beside `collateral` ("The currency of the associated entry"), without the order id: not
  joined.
- `HistoricalOrder.clientId` is read as the `cliOrdId` the order was sent with (its name; the doc
  does not say so), and `HistoricalOrder.quantity` as the order's whole size (beside `filled`).
- The history API's order event `reason` words are not enumerated, and whether a fully filled
  order leaves an event there is not said: only `OrderCancelled` and `OrderRejected` are read.
  Whether the `order.filled` an execution carries counts that execution is not said: it is
  read as what the order had filled.
- Whether the history API sends the next page's token in the body, the header or both: the
  OpenAPI shows both.
- Whether a stop or take profit that fires keeps its order id and `cliOrdId` (the socket says
  "The system created a limit order because an existing stop order triggered"): not said.
- `triggerSignal` default when not sent; which side of the market a `stp` and a `take_profit`
  trigger on (the history's `triggerSide` `Above` / `Below` is not set by the request).
- `cliOrdId` charset on REST and its uniqueness window.
- `/openorders` `orderType` enum `stop` against its description `stp`; `triggerSignal` there
  says `spot`, the request takes `index`.
- `minimumTradeSize` not populated; no minimum order value.
- Whether POST parameters are taken in the body (the guide) or the query (the OpenAPI writes
  `in: query`): the body is sent and hashed.
- No dry run; the demo host redirects (Environments).
- No worked `Authent` output for REST (its sample secret is not base64): the POST signature is
  checked against an independent Python implementation (`a_post_hashes_its_body_and_the_trimmed_path`).

What stays unproven: real fills and partial fills and their order, real error bodies where they
differ from the doc, the maker protection hold in practice, demo against live differences.
