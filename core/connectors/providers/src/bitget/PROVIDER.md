# Bitget (v2, spot and USDT-M futures)

Adapter version: 0.1.0. Provider API version: v2. Maturity: M2 (mock catalog green; shadow
runs against the live venue, history, instrument lists and socket, identical to the legacy
connector; account parity on mock, real probes refused by the venue as expected).

One adapter per book (`Bitget::spot()`, `Bitget::usdt_futures()`): `BTCUSDT` is a spot pair
and a perpetual at once and they are different series. A data connector picks its book with
its `market` setting (`spot`, `usdt-futures`); a broker account reads the books its `books`
setting names (`spot`, `usdt-futures`, both by default).

Sources (the `api-doc` pages now answer a 301 to the new site, read 2026-11-11; the trading
section below cites the new pages):
- Spot market: https://www.bitget.com/api-doc/spot/market/Get-History-Candle-Data
- Futures market: https://www.bitget.com/api-doc/contract/market/Get-History-Candle-Data
- Authentication: https://www.bitget.com/api-doc/common/signature
- Socket: https://www.bitget.com/api-doc/spot/websocket/public/Candlesticks-Channel

## Endpoints

| Endpoint id | Spot | USDT-M futures | Canonical |
| --- | --- | --- | --- |
| `historyCandles` | `GET /api/v2/spot/market/history-candles?symbol&granularity&endTime&limit=200` | `GET /api/v2/mix/market/history-candles?...&productType=USDT-FUTURES` | `RequestHistory{Candles}`, backwards from `endTime` |
| `symbols` / `contracts` | `GET /api/v2/spot/public/symbols` | `GET /api/v2/mix/market/contracts?productType` | `LoadInstruments` (active = `online` / `normal`) |
| `tickers` / `ticker` | `GET /api/v2/spot/market/tickers?symbol` | `GET /api/v2/mix/market/ticker?symbol&productType` | `RequestSnapshot{Quote}` |
| `board` | `GET /api/v2/spot/market/tickers` | | `RequestBoard` (`lastPr`) |
| `assets` | `GET /api/v2/spot/account/assets[?assetType=hold_only]` (signed) | | `Summary`, `Balances` (available + frozen + locked) |
| `accounts` | | `GET /api/v2/mix/account/accounts?productType` (signed) | `Summary` (USDT equity) |
| `allPosition` | | `GET /api/v2/mix/position/all-position?productType` (signed) | `Positions` (`holdSide`) |
| `unfilledOrders` / `ordersPending` | `GET /api/v2/spot/trade/unfilled-orders?limit=100`, then `currentPlanOrder` | `GET /api/v2/mix/order/orders-pending?productType&status&limit=100` (`entrustedList`), `status` `live` then `partially_filled`, then `ordersPlanPending` per plan list | `OpenOrders`; a hedge-mode futures row's side from `posSide` and `tradeSide` |
| `fills` / `fillHistory` | `GET /api/v2/spot/trade/fills?startTime&endTime&limit=100[&idLessThan]`, 90-day slices | `GET /api/v2/mix/order/fill-history?...`, 7-day slices, `fillList` + `endId` (demo account: `GET /api/v2/mix/order/fills`, same page, since the history "does not support to query the data in demo trading") | `Fills`, slices walked backwards from the period's end, 500 pages per book |
| socket | `wss://ws.bitget.com/v2/ws/public`, `{"op":"subscribe","args":[{"instType":"SPOT"|"USDT-FUTURES","channel":"candle1m","instId":..}]}` | | `Subscribe{Candles}` |

## Field mapping and rules

- Granularity: spot `1min 5min 15min 1h 4h`, futures `1m 5m 15m 1H 4H`; both `1Dutc 1Wutc`
  (the plain daily bar opens at 16:00 UTC).
- History rows carry neither a close time nor a confirm flag: a bar is final once its period
  plus 60 s (Bitget rewrites a bar for about 25 s after it closes) has passed.
- Socket rows have no closed flag either: a candle is final when a newer one starts on its
  channel (the previous one is re-emitted final, with the venue time it last carried). Older
  rows (a snapshot replaying history) are dropped. A refusal names its `arg`: that pane fails,
  the socket carries on; a refusal naming nothing is the socket's. `ping` after 20 quiet
  seconds (the venue drops a socket silent for two minutes), `pong` ignored.
- Fees: Bitget signs a charge negative; filed positive. A futures fill sums its fee lines.
- Currency: spot from the instrument list (quote coin), futures the `marginCoin`.

## Auth

`ACCESS-SIGN` = base64 HMAC-SHA256 of `timestamp(ms) + METHOD + path?query + body`, plus
`ACCESS-KEY`, `ACCESS-TIMESTAMP`, `ACCESS-PASSPHRASE`, `locale: en-US`. Prehash checked
against the documented example, signature against a reference computed with an independent
HMAC implementation. Secrets: `api_key`, `api_secret`, `api_passphrase`. Host clock. A demo trading account
(`demo` setting) sends `paptrading: 1` on every signed call, the account read included (the
account selector `demo` carries the setting to it).

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `00000` | success | none |
| 429 | rate limit | RateLimited |
| 40006, 40008, 40009, 40012, 40018, 40037 | key, timestamp, signature, passphrase, IP | Auth |
| 40014 | permission | Permission |
| 40034 | parameter (symbol) does not exist | NotFound ("bad symbol") |
| 40010, 40015, 45001 | busy, timeout | Transient (reads only; Unknown on the order path, see Trading) |
| anything else | | InvalidRequest |

## Deliberate differences from the legacy connector

- A futures fee summed over several lines is exact (decimal), not an f64 sum: it can differ
  from the legacy value in the last bit.
- A spot pair quoted in a coin outside USDT, USDC, EUR, BTC gets its currency from the
  instrument list; the legacy suffix rule left it empty.
- An account time keeps its milliseconds (unchanged for Bitget).

## Regions and terms

Bitget is not available in some jurisdictions; public market data answered from the test
machine.

## Live best bid and ask

Channel `books1` on the candle socket, both books: a one-level snapshot on every change,
`{asks, bids, ts}` (measured 2026-10-19). An unknown pair is refused (30001) naming its `arg`.

## Trading

Execution maturity: **E2D** (spot and USDT-M futures books), built from the documentation
alone: no account, no demo key, no canary. Sandbox: Bitget demo trading (`demo` setting:
`1`, `true`, `yes` or `on`), the same REST host with the header `paptrading: 1` and a Demo API
key, the private socket on `wss://wspap.bitget.com`. Code: `trade.rs` (mapper),
`trade_errors.rs` (error table); the order path is the shared trader's
(`connectors/engine/src/trading.rs`). Only the Classic (v2) API: whether an account upgraded
to the unified account (UTA) still trades through it is doc silent (see Doc gaps).

Doc snapshot (read 2026-11-11 around 20:55 UTC, copies kept with the batch survey). Base
`https://www.bitget.com/docs/`:

| Page | Used for |
| --- | --- |
| [Spot Trade](https://www.bitget.com/docs/catalog/classic-spot-trade/classic-spot-trade) | Place Order (`orderType`, `force`, `size` "For Market-Buy orders ... the number of quote coins", `clientOid`), Cancel Order, Cancel Order by Symbol ("executed asynchronously"), Get Order Info by `clientOid`, Get Current Orders (`priceAvg` is the order price), Get Fills (90 days, `idLessThan` = `tradeId`); Get History Orders is a test fixture only, never called |
| [Spot Trigger](https://www.bitget.com/docs/catalog/classic-spot-plan/classic-spot-plan) | Place Plan Order (`triggerPrice`, `triggerType`, `planType` `amount` = base coin), Cancel Plan Order (`data.result` "success or failure"), Cancel Plan Orders in Batch (`symbolList`), Get Current and History Plan Orders (statuses, `nextFlag`) |
| [Spot Market](https://www.bitget.com/docs/catalog/classic-spot-market/classic-spot-market) | Get Symbol Info (rules, statuses), Get Ticker Information (`askPr`, `bidPr`) |
| [Futures Trade](https://www.bitget.com/docs/catalog/classic-contract-trade/classic-contract-trade) | Place Order (`marginMode`, `marginCoin`, `size` in base coin, `force` "Required if the orderType is limit", `tradeSide` in hedge mode, `reduceOnly` one-way only, the reduce-only ack without `orderId`), Cancel Order, Cancel All Orders (`productType`, `marginCoin`), Get Order Detail, Get Pending Orders, Get Pending Orders by `status` (`live`, `partially_filled`), Get Order Fill Details (three months; the account read's futures fills in demo), Get Historical Transaction Details (the account read's futures fills, not in demo), Flash Close Position (not used) |
| [Futures Trigger](https://www.bitget.com/docs/catalog/classic-contract-plan/classic-contract-plan) | Place Trigger Order (`normal_plan`, `triggerType`), Cancel Trigger Order (by id list, or every one of a product type and plan type; its `planType` enum: `normal_plan`, `profit_plan`, `loss_plan`, `pos_profit`, `pos_loss`, `moving_plan`), Get Pending and History Trigger Orders (`planStatus`, `executeOrderId`, `baseVolume`), position TP/SL (`place-pos-tpsl`, `modify-tpsl-order`, Cancel Trigger Order `planType` `pos_profit` / `pos_loss`) |
| [Futures Market](https://www.bitget.com/docs/catalog/classic-contract-market/classic-contract-market) | Get Contract Config (rules, `symbolStatus`) |
| [Futures Position](https://www.bitget.com/docs/catalog/classic-contract-position/classic-contract-position) | Get Single Position, Get All Positions (`holdSide`, `posMode`, `marginMode`, `leverage`) |
| [Futures Account](https://www.bitget.com/docs/catalog/classic-contract-account/classic-contract-account) | Get Single Account (`marginMode`, `posMode`), Change Position Mode ("for all symbol futures"; OTW never calls it) |
| [Signature](https://www.bitget.com/docs/classic/rest-api) | `timestamp + METHOD + path?query + body`, headers |
| [Error codes](https://www.bitget.com/docs/classic/error-code/restapi) | error table |
| [FAQ](https://www.bitget.com/docs/classic/faq) | client id charset (Q12), release-window codes (Q13), message loss on the socket (Q14), rate ban (Q9) |
| [Rate limit](https://www.bitget.com/docs/classic/rate-limit/rate-limit) | 10 orders a second per UID by default |
| [Demo trading REST](https://www.bitget.com/docs/classic/demo-trading/rest-api), [Demo trading WebSocket](https://www.bitget.com/docs/classic/demo-trading/websocket) | `paptrading: 1` (orders and the account read), demo socket host |
| [WebSocket intro](https://www.bitget.com/docs/classic/websocket/intro) | private host, login frame and signature, `ping` every 30 s, answers |
| [Spot Order Channel](https://www.bitget.com/docs/classic/websocket/spot/private/Order-Channel), [Spot Fill Channel](https://www.bitget.com/docs/classic/websocket/spot/private/Fill-Channel), [Futures Order Channel](https://www.bitget.com/docs/classic/websocket/contract/private/Order-Channel), [Futures Fill Channel](https://www.bitget.com/docs/classic/websocket/contract/private/Fill-Channel), [Futures Positions Channel](https://www.bitget.com/docs/classic/websocket/contract/private/Positions-Channel) | private stream frames |

### Order types and flags

| Canonical | Spot | USDT-M futures | Notes |
| --- | --- | --- | --- |
| market | `orderType=market` | `orderType=market` | A spot market **buy** is sized in the quote coin: the trader reads the best ask (`askPr`, Get Ticker Information) just before sending and sends `size` = quantity times ask, rounded down to `quotePrecision`; the filled size may differ a little, and the answer says so. A zero or unread ask refuses it, nothing sent. Spot sells and every futures order are sized in the base coin. |
| limit | `orderType=limit`, `price` | `orderType=limit`, `price` | |
| stop | Place Plan Order, `orderType=market`, `triggerPrice`, `planType=amount` | Place Trigger Order, `planType=normal_plan`, `orderType=market` | Followed as `cond:<orderId>` (`cond:<planType>:<orderId>` for a futures plan of another type, read from the account). No trigger direction field exists: the venue checks the trigger against the market (`43034`, `43035`). |
| stop_limit | not declared | Place Trigger Order, `orderType=limit`, `price` | A spot plan row states no traded amount once triggered, so a spot stop-limit's end is not known: not declared. |
| post-only | `force=post_only` | `force=post_only` | limit, good till cancelled |
| reduce-only | not declared (a spot sale of what is held is a plain sell) | one-way: `reduceOnly=YES` (`yes` on a trigger order); hedge: `tradeSide=close` | `22042`: in one-way mode with a position held, a trigger order "cannot only reduce positions". The caps say reduce-only for every order type (they cannot say it per type), so a one-way reduce-only stop is offered, sent, and refused by the venue with `22042` while a position is held: nothing checks it before. |
| trigger price | not declared (`fill_price` sent) | `last` = `fill_price`, `mark` = `mark_price` | |
| close position | | reduce-only market for the size the venue holds now (Get Single Position), part of it allowed | Flash Close Position is not used: it takes no client id. |
| position mode | | read from Get Single Account (`posMode`, the product type's, asked with `symbol=BTCUSDT` since the call needs one), held, read again after `43075`, `45021`, `45109`, `45020`; never changed. One-way: `side` is the trade's; hedge: `side` names the position (`buy` long, `sell` short) and `tradeSide` opens or closes it ("Close long: side=buy, tradeSide=close"). | |
| margin mode | | read from Get Single Account for the symbol (`marginMode`, `marginCoin`) or a position row, held, sent with every order; read again after `40716`, `22005`. An answer without a margin mode refuses the order (never picked). | |
| leverage | | read only (position `leverage`), shown, never set | |

Left out, from the doc survey's verdict: amend (futures Modify Order "will cancel the old
order; then create a new order asynchronously", spot Cancel-Replace "may cause the old order
to be successfully canceled but the new order placement to fail"), preset TP/SL on an order
(its children's ids and lifecycle are doc silent), spot `tpsl` orders, trailing orders
(`track_plan`), dry run (none in the Classic docs), Flash Close Position and Reversal.

Position take profit and stop loss (USDT-M futures, on a held position, not with an entry):
set with Place Position TP/SL (`place-pos-tpsl`, no size: the whole position, triggered on
the last price, closed at the market) when none of that kind works; moved with Modify the
TP/SL order (`modify-tpsl-order`, its `orderId`, `size` empty "for position TP/SL orders")
when one does, found first in Get Pending Trigger Orders (`planType=profit_loss`, rows
`pos_profit` / `pos_loss`); removed with Cancel Trigger Order under its plan type
(`pos_profit`, `pos_loss`). `holdSide`: `long` / `short` in hedge mode, the position's
direction (`buy` / `sell`) in one-way mode. Whether a second Place Position TP/SL replaces a
working one is doc silent, so it is never sent then. Declared as `position_exits` in the
futures caps: the positions panel sets one on a position that has none.

The account read lists every working order of the books it reads, plan orders included: spot
unfilled orders and current plan orders; futures pending orders `live` then
`partially_filled` (Get Pending Orders lists only `live` "if not specified"), then pending
trigger orders of each list (`normal_plan`, `track_plan`, `profit_loss`). A futures row's
direction is read as the order path reads it: in hedge mode from `posSide` and `tradeSide`
(a close of a long is a sell), else the row's `side`. A plan order is `cond:<orderId>`,
`cond:<planType>:<orderId>` for a futures plan type other than `normal_plan`: the same id in
the account read, the order path's lists and look-ups, and a cancel all's leftover rows, so a
look-up by that id finds it and its cancel names that plan type. A position take profit or
stop loss is `tpsl::tp:<side>` / `tpsl::sl:<side>` in the account read (the side that opened
the position: `posSide`, else, in one-way mode, the row's `side`, doc silent), changed and
removed on the position as above (the order path reads it as `cond:pos_profit:<orderId>` /
`cond:pos_loss:<orderId>` and names it by `<orderId>` in the change).

Cancel all: spot cancels each symbol's orders (Cancel Order by Symbol) and every listed
symbol's plan orders (Cancel Plan Orders in Batch). Futures cancels the product type's orders
(Cancel All Orders), then its trigger orders once per plan type the Cancel Trigger Order enum
lists (`normal_plan`, `profit_plan`, `loss_plan`, `moving_plan`); the position's own take
profit and stop loss (`pos_profit`, `pos_loss`) stay with the position. The working-order
read the cancel all checks afterwards includes the trailing stops (`track_plan`) and the
take profit and stop loss list without the position's own, and what it finds is cancelled one
by one.

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| GTC | `force=gtc` | |
| IOC | `force=ioc` | |
| FOK | `force=fok` | refused on a market order before I/O (spot: `force` "is invalid when orderType is market"; futures: doc silent) |
| any, on a stop | none | plan orders carry no `force`: a stop with a time in force other than GTC is refused before I/O |

Futures market orders send no `force` (it is "Required if the orderType is limit"); spot
market orders send the one asked (required, ignored).

### Client order id

`clientOid`: FAQ Q12 "supports [0-9], [a-z], [A-Z] and [-,+,_,#], with a length less than
50". OTW sends its key unchanged, checked against at most 49 of letters, digits, `-`, `+`, `_`,
`#` (the comma is left out: ambiguous in the list, and `40304` / `40305` disagree on the
length). Duplicates (`43118`, `45034`, `50060`) are Unknown: looked up, never resent. Look
up: spot Get Order Info by `clientOid`, then the symbol's current and past plan orders (they
take no client id filter, so the rows are searched); futures Get Order Detail, then pending and
past trigger orders, each by `clientOid`.

### Order status

| Native | Canonical |
| --- | --- |
| `live`, `new` (the Get Current Orders example) | New |
| `partially_filled` | PartiallyFilled |
| `filled` | Filled |
| `cancelled` (spot), `canceled` (futures) | Cancelled |
| plan `live`, `not_trigger` | New, trigger pending |
| plan `executing` ("order placing") | New |
| plan `executed` (futures "the order triggered", spot "Executed successfully") | Filled. Futures: when the history row's `baseVolume` is under the size, New (nothing traded yet) or PartiallyFilled: the order it placed (`executeOrderId`) still works, and is cancelled by its own id (it is listed as a plain working order), not by the plan's. Spot: the row states no traded amount, the stop's market order is taken as filled at its size (whether `executed` means traded or only placed is doc silent; Get Plan Sub Order is not read). |
| plan `fail_execute` | Cancelled (the venue dropped it) |
| any other | Unknown, alerts |

Bitget has no rejected state: a refused order answers an error code.

### Instrument trading rules

Spot Get Symbol Info: tick `10^-pricePrecision`, size step `10^-quantityPrecision`, quote step
`10^-quotePrecision` (the quote-sized market buy), minimum value `minTradeUSDT` and maximum
limit order value `maxLimitOrderValue` (both in USDT, so only on a USDT pair), status `online`
trades, `gray`, `halt`, `offline` and any other word do not. Futures Get Contract Config: tick
`priceEndStep x 10^-pricePlace`, size step `sizeMultiplier`, minimum `minTradeNum`, maximum
`maxOrderQty` (limit) and `maxMarketOrderQty` (market), minimum value `minTradeUSDT`, status
`normal` trades, `limit_open` takes reduce-only orders only, `listed`, `maintain`,
`restrictedAPI`, `off` and any other word do not. Read at the first order, daily, and again
after a rule refusal. Off-grid values are refused with the nearest valid ones, never adjusted.

### Errors

The order codes of the [error page](https://www.bitget.com/docs/classic/error-code/restapi)
(one flat list of 830 codes: those an order call can answer are listed; transfer, copy trade,
P2P, savings, broker and loan codes are not and would be Unknown with an alert), mapped in
`trade_errors.rs` (a test asserts each row maps as listed and an unlisted code is Unknown and
alerts). Envelope `{"code","msg","requestTime","data"}`, `00000` a success. Refusals read
"Bitget refused it: <msg> (code <code>)". The read path keeps its own table (`errors.rs`):
`40010`, `40015`, `45001` stay transient there; on the order path they are Unknown.

| Codes | Canonical | Meaning | Action |
| --- | --- | --- | --- |
| `429`, `22035`, `40918`, `45129`, `50066`, `59044`, `70228` | RateLimited | too many requests, demo opens too often, cancel of one id twice a second, position closing ("try again later"), operations too frequent | refused, new orders held for `Retry-After` or one second (the per-second order window) |
| `40001`, `40002`, `40003`, `40005`, `40006`, `40008`, `40009`, `40011`, `40012`, `40018`, `40036`, `40037`, `40038`, `40041`, `40078` | Auth | key, signature, passphrase, timestamp, receive window, IP | refused, nothing resent |
| `22010`, `22030`, `40013`, `40014`, `40016`, `40022`, `40023`, `40024`, `40025`, `40026`, `40031`, `40035`, `40040`, `40081`, `40100`, `40101`, `40103`, `40104`, `40105`, `40110`, `40111`, `40199`, `40301`, `40710`, `40752`, `40753`, `40760`, `40828`, `40916`, `45019`, `45107`, `45108`, `70227` | Permission | permissions, KYC, region, demo limits, account state | refused |
| `40010`, `40015`, `40200`, `40725`, `40808`, `40908`, `40910`, `43059`, `43118`, `45001`, `45023`, `45024`, `45034`, `49040`, `50031`, `50060`, `80002` | Unknown | timed out, system abnormal or upgrading, release window (FAQ Q13), concurrent operation, duplicate client id, create or cancel error | looked up by `clientOid`, never resent |
| `00171`, `00172`, `01001`, `01003`, `22005`, `40007`, `40017`, `40019`, `40020`, `40034`, `40053`, `40057`, `40058`, `40059`, `40064`, `40072`, `40079`, `40102`, `40304`, `40305`, `40402`, `40404`, `40716`, `40724`, `40778`, `40805`, `40811`, `40812`, `40813`, `40822`, `40912`, `40913`, `40919`, `40923`, `40924`, `40925`, `43011`, `43048`, `43049`, `43058`, `43075`, `45017`, `45018`, `45020`, `45021`, `45025`, `45026`, `45051`, `45052`, `45056`, `45109`, `45124` | InvalidRequest | malformed request, symbol, margin coin or margin mode not taken, position mode mismatch | refused (a bug signal); `43075`, `45021`, `45109`, `45020` read the position mode again, `40716`, `22005` the margin |
| `01002`, `22034`, `22038`, `40887`, `40888`, `40921`, `43006`, `43007`, `43010`, `43027`, `43028`, `43037`, `43038`, `43041`, `43042`, `45035`, `45103`, `45104`, `45110`, `45111`, `45112`, `45113`, `45114`, `45115`, `45120` | Rejected(venue_rule) | precision, size, value or price step off the rules | refused, rules read again |
| `22029`, `40722`, `40754`, `40756`, `40762`, `40798`, `40800`, `40826`, `40928`, `43012`, `45002`, `45004`, `45008` | Rejected(funds) | balance or margin | refused |
| `22006`, `22007`, `22008`, `22009`, `40706`, `40718`, `40719`, `40748`, `40815`, `40816`, `40820`, `40821`, `40829`, `40830`, `40831`, `40832`, `40833`, `40834`, `40835`, `40836`, `40915`, `40917`, `43008`, `43009`, `43013`, `43014`, `43015`, `43016`, `43034`, `43035`, `43039`, `43040`, `45060`, `45061`, `45062`, `45064`, `45065`, `45066`, `45067`, `45121`, `45122` | Rejected(price) | price band, liquidation price, trigger on the wrong side of the market | refused |
| `22004`, `22039`, `22040`, `22041`, `22043`, `22044`, `22067`, `40309`, `40721`, `40823`, `40824`, `40825`, `40844`, `40845`, `40878`, `42016`, `45043`, `45119`, `45123` | Rejected(market closed) | no API trade, delivery, ADL, maintenance, removed, not traded on spot, no market orders now | refused |
| `22002`, `22042`, `40717`, `40746`, `40755`, `40757`, `40758`, `40765`, `40804`, `40837`, `40867`, `40891`, `40939`, `43023`, `43029`, `43043`, `45003`, `45005`, `45006`, `45007`, `45012`, `45068` | Rejected(reduce only) | no or not enough position to close | refused |
| `40930` | Rejected(post only) | post-only quantity limit | refused |
| `22001`, `40109`, `43001`, `43004`, `43020`, `43025`, `45057` | Rejected(not found) | the order does not exist | a cancel is reconciled (looked up by id); a look-up moves to the next call |
| `40922`, `43021`, `43026`, `45031`, `45055`, `50062` | Rejected(final) | the order is already final | a cancel is reconciled: no error when it ended |
| `12002`, `12003`, `13007`, `13009`, `22045`, `40715`, `40723`, `40761`, `40764`, `40766`, `40846`, `40847`, `40859`, `40862`, `40863`, `40865`, `40879`, `40889`, `40890`, `40931`, `43002`, `43003`, `43005`, `43024`, `43030`, `43031`, `43050`, `43071`, `43128`, `45009`, `45011`, `45013`, `45014`, `45116`, `45118`, `45127`, `45128`, `46013`, `70220`, `70221`, `70222`, `70223`, `70224`, `70225` | Rejected(other) | order caps, risk, liquidity, position state, a coin's buy or sell quota | refused, the venue's words shown |

Not codes of the table: a timeout, a closed socket, HTTP 5xx or an unreadable answer is
Unknown (looked up, never resent); HTTP 429 is RateLimited whatever the code; a futures plan
order or position take profit or stop loss cancelled by id answers its refusal inside a
success (`failureList`): read with its `errorCode` when present, otherwise Unknown (the cancel
is then reconciled); a spot plan order cancelled by id answers `data.result` ("Result is
success or failure"): anything but `success` is Unknown and alerts (the cancel is then
reconciled by id); any other code is Unknown and alerts.

### Rate limits for orders

Place and cancel 10 per second per UID (spot and futures; VIP levels up to 100, Rate limit
page), spot Cancel Order by Symbol and Cancel Plan Orders in Batch 5 per second, Flash Close
Position 1 per second (not used). Overall "6000/IP/Min", "it takes 5 minutes to recover" (FAQ
Q9): no ban code is documented. A rate refusal holds new orders for `Retry-After` when the
answer carries one, else one second. Orders are refused, never queued.

### Stream

Private socket `wss://ws.bitget.com/v2/ws/private` (demo `wss://wspap.bitget.com`), login
`{"op":"login","args":[{apiKey, passphrase, timestamp, sign}]}` with sign = base64
HMAC-SHA256 of `timestamp + "GET" + "/user/verify"`, answered `{"event":"login","code":"0"}`;
one subscribe frame per channel, `instId` `default` (every symbol): spot `orders`, `fill`;
futures `orders`, `fill`, `positions`. The string `ping` every 30 s ("disconnect ... if no
string ping received for 2 min"), `pong` read as its answer. An `error` event refuses the
subscription it names, or the login. Trigger orders are not followed on the socket
(`orders-algo` is not subscribed): they are read on the reconciliation cycle. FAQ Q14
("possibility of message loss ... use the REST API as a fallback"): the reconciliation cycle
stays the truth.

### Disconnect behaviour

The doc names no cancel-on-disconnect: working orders stay at the venue (not declared).

### Dry-run

None in the Classic docs: not declared, so the E2D session check does not run for Bitget.

### Quirks

- Spot Get Current Orders writes the order's price in `priceAvg` (and the fill price in
  `basePrice`); Get Order Info and Get History Orders write it in `price` and the average in
  `priceAvg`. Read per endpoint.
- A spot market buy's `size` is the quote amount spent, in the orders, the history and the
  socket alike: its quantity is read as the base volume it bought.
- The futures order detail answers one object, the other lists an object holding the list
  (`entrustedList`, `orderList`, `fillList`), the spot lists a plain list: read by shape.
- The `orders` channel's `baseVolume` is the latest fill, `accBaseVolume` the total.
- Fees: Bitget writes a charge negative in REST examples and positive in the spot `fill`
  channel example; filed positive (a cost) as the account read does. A rebate is not described
  (see Doc gaps).
- The spot `orders` channel's field table names the order type `ordType`, its example
  `orderType`: either is read.
- The doc examples spell the margin mode `cross` where the enum says `crossed`: both read.

### Doc gaps

What the documentation does not state, so OTW does not rely on it (each closed only by a demo
run, E3, or the live canary, E4):

- Whether a second Place Position TP/SL replaces the one working (never sent then), and which
  side a one-way position TP/SL row states.
- Whether an account upgraded to the unified account (UTA) can still trade through the Classic
  (v2) endpoints ("The Classic Account is in maintenance mode and receives only essential
  updates").
- The futures tick formula: the doc gives `priceEndStep` ("price step length") and
  `pricePlace` (decimal places) without one; OTW reads the step in units of the last place
  (`45115` "price ... multiple of" refuses, and reloads the rules, if that is wrong). Seen
  2026-11-12: every one of the 884 listed USDT-M contracts answers `priceEndStep` `1`, so the
  tick is `10^-pricePlace` on all of them today.
- Rounding: whether a price or size off the grid is refused or rounded (OTW refuses first).
- The trigger direction of a plan order (no field; `43034` and `43035` hint at a check
  against the market).
- What a futures market order does with `force` (OTW sends none) and whether a spot market
  order refuses one (OTW sends the one asked, refused before I/O when FOK).
- The life of a triggered plan order: the order it places (`executeOrderId`, futures) is not
  followed to its end, and a cancel by the plan's id does not reach it (that order shows
  among the plain working orders, where a cancel or a cancel all reaches it); a spot plan row
  states no traded amount, and whether spot `executed` ("Executed successfully") means traded
  or only placed is not stated (Get Plan Sub Order is not read).
- How to cancel a trailing stop (`track_plan`) by its id: the Cancel Trigger Order `planType`
  enum does not list it (it lists `moving_plan`, a trailing take profit and stop loss placed
  elsewhere). OTW sends `track_plan`, the plan type the lists name it by; a refusal is shown
  as the venue's error. A cancel all sends no bulk `track_plan` call for that reason: it
  cancels each trailing stop it still finds by its id.
- Spot plan look-ups (by client id or by `cond:` id) read the symbol's newest 100 current and
  100 past plan orders (no id or client id filter exists on those lists): an older one is not
  found there.
- How long a client id stays queryable, and its uniqueness window.
- When an order becomes visible after the ack (the unknown window is the reconciliation
  cycle's, not measured).
- The error code for a wrong `tradeSide` or position mode (`43075` "Position pattern mismatch"
  is read as it).
- Whether Get Pending Orders without `status` lists partially filled orders ("If not
  specified, all ordered with a status of live"): OTW asks `live` and `partially_filled`
  apart, in the order path and the account read alike.
- In hedge mode, whether a fill's `side` is the trade's or the position's (read as the trade's,
  in the order path and the account read alike; the Get Order Fill Details example shows
  `side` `buy` with `tradeSide` `close` in `hedge_mode` and does not say which); an order's
  direction is read from `posSide` and `tradeSide`.
- Whether `gray` (spot "grey scale") trades: not offered.
- The socket login timestamp unit (milliseconds in the text and the JavaScript sample, seconds
  in the Java sample): milliseconds sent.
- Spot fill `cTime` ("Unix second timestamp", the example is in milliseconds): read as
  milliseconds, as the account read does.
- Demo trading: which product types and symbols it lists. Get Historical Transaction Details
  (`fill-history`) "does not support to query the data in demo trading": the order path and,
  on a demo account, the account read take futures fills from Get Order Fill Details
  (`fills`), which states no such limit.
- Whether a fee can be a rebate and how its sign reads then: the REST examples write a charge
  negative, the spot `fill` channel example positive; every fee is filed as a cost
  (its absolute value).
- The error body of each code: the doc lists codes, the envelope is the one every documented
  answer carries.

### What stays unproven

Real fills, partial fills and their event order; real error bodies where they differ from the
doc; ack timing; undocumented filters, account-tier limits and regional restrictions; demo
against live differences; UTA accounts.

### Fixtures

Verbatim doc examples in `fixtures/trade/doc_*.json` (sources in `trade_tests.rs`). Synthetic
(`synthetic`, built on a documented shape): the catalog script's answers (the doc examples
with ids, sizes, prices, symbol, sides and status changed), every error body (documented code
in the documented envelope), the closed-instrument variants of the instrument lists, the
reduce-only ack without `orderId` (the place example with `orderId` null), the hedge close and
the zero-volume trigger history rows, the ticker with a non-zero ask, and the one-way position
row.
