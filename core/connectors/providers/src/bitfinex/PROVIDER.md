# Bitfinex (spot pairs and perpetual derivatives)

Adapter version: 0.1.0. Provider API version: v2. Maturity: M2 (mock catalog green; history,
instrument lists, quotes, the board and the socket checked against the live venue on
2026-10-19; account parity on mock only, no key lent: the only signed answers recorded are
the refusals of a made-up key).

One OTW provider, `bitfinex`, two adapters chosen by the connector's `market` setting:
`Bitfinex::spot()` (venue `BITFINEX`) and `Bitfinex::derivatives()` (venue `BITFINEX_PERP`,
the perpetuals `<BASE>F0:<QUOTE>F0`, settled in USDt or BTC). A pair is written `tBTCUSD`, or
`tAVAX:USD` when either code is longer than three letters; a perpetual `tBTCF0:USTF0`. The
leading `t` may be left out when typing a name. Both books ride the same endpoints; the
account of both is read once, by the spot adapter.

Sources:
- API v2 reference: https://docs.bitfinex.com/docs (pages `rest-public-candles`,
  `rest-public-ticker`, `rest-public-tickers`, `rest-public-conf`, `rest-auth-wallets`,
  `rest-auth-positions`, `rest-auth-retrieve-orders`, `rest-auth-trades`, `key-permissions`,
  `ws-general`, `ws-public-candles`, `requirements-and-limitations`, `abbreviations-glossary`,
  and the "[REST][Python]" and "[REST][Javascript] Authenticated request example" recipes).
- ccxt `python/ccxt/bitfinex.py` (master, 2026-10-19): error codes and messages, the trade
  and order row layouts, the fee sign, the nonce rule.
- Measured answers of the public endpoints and socket, 2026-10-19 (recorded in `tests.rs`).

## Endpoints

Public data below `https://api-pub.bitfinex.com`, the account below `https://api.bitfinex.com`.

| Endpoint id | Method + path | Canonical |
| --- | --- | --- |
| `candles` | `GET /v2/candles/trade:{tf}:{symbol}/hist?start&end&limit=10000&sort=1` | `RequestHistory{Candles}` |
| `candles` | `GET /v2/candles/trade:1M:{symbol}/hist?limit=1` | whether an empty page's name ever traded |
| `conf` | `GET /v2/conf/pub:info:pair` (spot), `GET /v2/conf/pub:info:pair:futures` (derivatives) | `LoadInstruments` |
| `ticker` | `GET /v2/ticker/{symbol}` | `RequestSnapshot{Quote}` |
| `tickers` | `GET /v2/tickers?symbols=ALL` | `RequestBoard` (spot) |
| `permissions` | `POST /v2/auth/r/permissions` (signed) | `Summary` |
| `wallets` | `POST /v2/auth/r/wallets` (signed) | `Summary`, `Balances` |
| `positions` | `POST /v2/auth/r/positions` (signed) | `Positions` |
| `orders` | `POST /v2/auth/r/orders` (signed) | `OpenOrders` |
| `tradesHist` | `POST /v2/auth/r/trades/hist` body `{start, end, limit: 1000, sort: -1}` (signed) | `Fills` |

Socket: `wss://api-pub.bitfinex.com/ws/2`, one for both books.

- Subscribe `{"event":"subscribe","channel":"candles","key":"trade:<tf>:<symbol>"}`, answered
  `{"event":"subscribed","channel":"candles","chanId","key"}`. Every later frame is routed by
  `chanId`: a snapshot `[chanId, [[MTS, OPEN, CLOSE, HIGH, LOW, VOLUME], ...]]` (the newest
  240 bars, newest first), updates `[chanId, [MTS, ...]]`, a heartbeat `[chanId, "hb"]` every
  15 s. Unsubscribe `{"event":"unsubscribe","chanId"}`, answered `unsubscribed`.
- Measured: an unknown pair, a name without its `t` and an unknown timeframe (`7m`) are all
  *accepted*, with an empty snapshot `[chanId, []]`. Bars only exist where something traded,
  so the newest 240 of a real pair are never empty: an empty snapshot refuses the pane by
  name (NotFound) and its channel is unsubscribed.
- Measured: every update carries the previous bar next to the forming one; the older is no
  news (the engine keeps the forming one). Only the snapshot's newest bar goes out.
- Refusals `{"event":"error","code","msg","channel","key"}` are routed by their key:
  10305 `subscribe: limit` (RateLimited), 10301 `subscribe: dup` (ignored, the first stands).
  A refusal naming no pane (10400 `unsubscribe: invalid`, 10000 `event: invalid`) is
  reported as an `Error` event and the stream goes on.
- `info` 20051 (server restarting) and 20061 (maintenance over) end the socket, so the hub
  reconnects and subscribes again; 20060 (maintenance starting) is waited out.
- A pane closed before its `subscribed` arrived is unsubscribed when the channel id does,
  on the next keepalive.
- `{"event":"ping","cid"}` answers `pong`; it is only sent to draw a frame when a pane was
  refused before anything else went out (the heartbeats keep the socket alive).

## Field mapping and rules

- Timeframe tokens (REST and socket): `1m 5m 15m 30m 1h 3h 4h 6h 12h 1D 1W 1M`. `4h` answers
  though the docs leave it out; `2h`, `8h` and `1d` answer an empty list; `7D` opens on
  Thursday (epoch weeks) and `14D` is not a calendar unit, so neither is used (measured).
  Bars open at midnight UTC and on the hour for 3h to 12h, `1W` on Monday, `1M` on the first
  of the calendar month (measured).
- Candle row: `[MTS (ms, open), OPEN, CLOSE, HIGH, LOW, VOLUME]`: close comes before high and
  low. Volume counts the base currency on both books; no quote volume, no trade count.
- Bars are sparse: a period without a trade has no bar (measured: 6638 one-minute bars over
  6726 minutes on BTCUSD).
- Paging (measured): `limit` takes 10000 at most (`limit: invalid` beyond), `start` and `end`
  in ms are both inclusive on the open time, `sort=1` answers the oldest first. Pages walk
  forwards from the range's start, 10000 bars of the timeframe at a time, `end` one ms short
  of the next window. A calendar month is one call over the whole range.
- Lookback: every timeframe back to the listing (measured: BTCUSD minutes from 2013-04-01).
- Closed flags: neither archive nor socket carries one. Measured over six minute boundaries
  on BTCUSD, ETHUSD and BTCF0:USTF0: a closed minute was rewritten up to 20 s after its close
  (11 rewrites), so an archive bar is final 60 s after its close. Socket candles go out as
  forming (`candles_final_flag = false`): the engine closes one when the next starts.
- Unknown names: candle history answers an empty list for a name that does not exist, like
  a quiet period (measured). An empty page of a name not yet known (not in the instrument
  list, never answered with bars) asks for its newest monthly bar: none means the name never
  traded (NotFound, naming the spelling); one means the period was quiet. Delisted pairs keep
  their bars (measured on BSVUSD, EOSUSD, BTCJPY) while the ticker refuses them.
- Books: each book's market data refuses the other's names (NotFound, naming the fix), since
  the venue would answer them under the wrong venue id. `instrument_id` takes either
  spelling and gives the id the venue of the name's own book.
- Instrument lists: `pub:info:pair` (and `:futures`) list every pair with `[?, null, null,
  MIN_ORDER, MAX_ORDER, ...]`, the same set as `pub:list:pair:*` (checked). `min_qty` is
  `MIN_ORDER`; no tick size (Bitfinex prices to five significant digits) and no lot step are
  published per pair. Paper-trading pairs (`TEST...`) are left out of the list and the
  board. Derivatives: asset class perp, multiplier 1 (amounts count the base currency),
  settled in their quote.
- Currencies: the account and the lists use Bitfinex's codes (`UST` is USDt, `UDC` USDC,
  `EUT` EURt...). The derivatives suffix is dropped (`USTF0` is `UST`, `BTCF0` is `BTC`), as
  Bitfinex's `pub:map:currency:sym` maps them.
- Quotes: `[BID, BID_SIZE, ASK, ASK_SIZE, DAILY_CHANGE, DAILY_CHANGE_RELATIVE, LAST_PRICE,
  VOLUME, HIGH, LOW, ...]`. Board: the `tickers` rows `[SYMBOL, BID, ..., LAST_PRICE (7),
  ...]` of the spot pairs; funding (`fUSD`, laid out otherwise) and derivatives rows are left
  out.
- `pair_symbol`: `t` + base + quote, with a colon when either is longer than three letters
  (every listed pair follows it, checked).

## Account

- One key sees the exchange, margin and funding wallets, the margin positions (on spot
  pairs) and derivatives positions, every working order and every fill: the spot adapter
  reads them all, the derivatives adapter declares no account.
- Each order and position row carries the label of its book (`Bitfinex` for a spot pair,
  `Bitfinex derivatives` for a perpetual, `rows_carry_terms`): a cancel or a close from the
  account's panel goes to that book's orders ("Trading"). Fills keep `Bitfinex`.
- Summary: the key's permissions (`[[SCOPE, READ, WRITE], ...]`, written `orders: read,
  write`; `can_trade` is the orders write flag), then the wallets when the key may read them.
- Balances: the wallets `[TYPE, CURRENCY, BALANCE, UNSETTLED_INTEREST, AVAILABLE_BALANCE,
  ...]` summed per currency across the three wallets; an unknown (null) available balance
  counts as the whole. USD, EUR, GBP and JPY are fiat.
- Positions: `[SYMBOL, STATUS, AMOUNT (signed, base currency), BASE_PRICE, FUNDING,
  FUNDING_TYPE, PL, ...]`, `ACTIVE` ones only; `PL` is the unrealised profit (it includes the
  0.2% fee to close). A derivatives row is marked perp.
- Open orders: `[ID, GID, CID, SYMBOL, MTS_CREATE, MTS_UPDATE, AMOUNT (left, signed),
  AMOUNT_ORIG, ORDER_TYPE, ..., PRICE (16), PRICE_AVG, PRICE_TRAILING, PRICE_AUX_LIMIT (19),
  ...]`. Side from the sign. Order types (the `EXCHANGE ` prefix marks the exchange wallet
  and changes nothing else): `LIMIT`, `FOK`, `IOC` limit at `PRICE`; `MARKET` no price;
  `STOP`, `TRAILING STOP` trigger at `PRICE`; `STOP LIMIT` trigger at `PRICE`, limit at
  `PRICE_AUX_LIMIT`. Anything else is logged and read as a limit at `PRICE`.
- Fills: `trades/hist` answers every instrument in one list, so no name is needed
  (`fills_need_symbols` false; names passed are not used). Rows `[ID, SYMBOL, MTS, ORDER_ID,
  EXEC_AMOUNT (signed), EXEC_PRICE, ORDER_TYPE, ORDER_PRICE, MAKER, FEE (negative when
  paid), FEE_CURRENCY, CID]`; the fee is negated (positive = cost). Paged newest first with
  `sort=-1`, 1000 rows a call (the docs allow 10000 since the Trades page of 2026-10-28,
  ccxt writes 1000; the smaller is used):
  a full page asks again with `end` at its oldest millisecond, leaving out the ids of that
  millisecond already read; more than a page of fills within one millisecond is refused. No
  history window is documented: the whole account history is readable.

## Auth

- Every signed call is a POST with a JSON body (`{}` when it takes no parameter).
  `bfx-signature` is the lowercase hex HMAC-SHA384, keyed with the API secret, of `/api` +
  path + nonce + body (the body exactly as sent); `bfx-apikey` the key, `bfx-nonce` the
  nonce, `Content-Type: application/json`. Checked against references computed with
  Python's `hmac`.
- Nonce: Bitfinex only asks that it strictly increase per key and stay below 2^53 (docs).
  It is microseconds, as Bitfinex's JavaScript example and libraries write it, from a
  process-wide counter. A key also used by another program with larger nonces is refused
  (10114 `nonce: small`): the refusal says to give OpenTraderWorld a key of its own (the
  docs' advice for several clients).
- Clock: none to read. `SyncTime` is not declared; the nonce is a counter, not a time window.
- Secrets: `api_key`, `api_secret`. A read-only key needs read access to the `account`,
  `wallets`, `orders`, `positions` and `history` scopes (the scopes `auth/r/permissions`
  names), and nothing written.

## Rate limits

Per endpoint and IP, per minute (docs): candles 30, tickers 30, ticker 90, conf 90, and 90
for each authenticated read. An IP over a limit is blocked for 60 s (`{"error":
"ERR_RATE_LIMIT"}`). Socket: 20 connections per minute on `api-pub`; subscriptions per
socket 30 by the socket docs (25 by the limits page; 56 were accepted on 2026-10-19 before
`subscribe: limit`): a 31st pane is refused by the adapter.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| HTTP 429, `ERR_RATE_LIMIT`, 11010 | rate limit | RateLimited (60 s when no `Retry-After`) |
| 10100, 10111, 10112, 10113 | key, payload, signature (measured: `apikey: invalid`, `apikey: digest invalid`) | Auth, naming what to check |
| 10114 | nonce | Auth, asking for a key of its own |
| 10001 with "permission" | the key lacks the permission | Permission, naming it |
| 10020 `symbol: invalid` | unknown symbol (measured) | NotFound, naming the symbol and the spelling |
| 10020 otherwise | bad parameter (measured: `limit: invalid`, `time_interval: invalid`) | InvalidRequest |
| 10000, 10008, 11000, 20060, `temporarily_unavailable` | venue error, not ready, maintenance | Transient |
| anything else | | InvalidRequest |

Refusals come as `["error", code, "message"]` with HTTP 500 (measured); the code alone is
coarse, so the message decides where it does not. A body that is not a refusal falls back to
the HTTP status (a 404 page for an unknown path, measured).

## Not covered

- Funding (lending) offers, credits and loans, funding candles (`fUSD`), the ledger.
- Trades history, deep order books and tickers as live streams; candles and the one-level book (best bid and ask) stream.
- Paper-trading pairs (`TEST...`) in the instrument list.

## Regions and terms

Bitfinex does not serve residents of the United States and some other jurisdictions (its
terms of service); public market data answered from the test machine. The API terms apply
to automated access.

## Live best bid and ask

Channel `book` with `prec: P0, freq: F0, len: 1` on the candle socket: a snapshot of one bid
(amount > 0) and one ask (amount < 0), then `[price, count, amount]` updates, count 0 emptying
the side until the next level arrives (measured 2026-10-19). The `ticker` channel's bid/ask
only moves every few seconds and is not used. An unknown symbol is refused (10300) by `channel`
and `symbol`.

## Trading

Execution maturity: **E2D** on both books, built from the documentation alone: no account, no
paper key, no canary. No test host: every account is live (paper trading is a sub-account on
the live hosts, left out, see below). Code: `trade.rs` (mapper), `trade_errors.rs` (error
table); the order path is the shared trader's (`connectors/engine/src/trading.rs`); otw-core
registers both books in `brokers/native_trade_bitfinex.rs`.

Books: `spot` (the exchange wallet: `EXCHANGE`-prefixed types on a spot pair) and
`derivatives` (the perpetuals `<BASE>F0:<QUOTE>F0`, unprefixed types). The margin book (the
unprefixed types on a spot pair, the margin wallet) is not traded: it shares its pairs with
the exchange book, so an order there would need a third book over the same symbols.

Doc snapshot (read 2026-11-12, pages fetched as Markdown, copies kept with the batch survey).
Base `https://docs.bitfinex.com/`:

| Page | Used for |
| --- | --- |
| [Submit Order](https://docs.bitfinex.com/reference/rest-auth-submit-order) | `POST /v2/auth/w/order/submit`: `type` (the `EXCHANGE` prefix), `symbol`, `amount` (positive buy, negative sell), `price`, `price_aux_limit` ("only for STOP LIMIT"), `flags`, `tif` ("UTC datetime ... e.g. 2020-01-15 10:45:23"), `cid` ("unique in the day (UTC+0). (int45)"), `lev` ("supported by derivative symbol orders only ... between 1 and 100 ... If omitted the default leverage value of 10 will be used"); the notification answer and the order array |
| [Update Order](https://docs.bitfinex.com/reference/rest-auth-update-order) | `POST /v2/auth/w/order/update`: `id` required, `price`, `price_aux_limit`, `lev` ("Mandatory for derivative order updates"), `tif` ("can be updated but not removed") |
| [Cancel Order](https://docs.bitfinex.com/reference/rest-auth-cancel-order) | `POST /v2/auth/w/order/cancel` by `id`, or `cid` + `cid_date`; answers "Submitted for cancellation; waiting for confirmation" |
| [Cancel Orders (multiple)](https://docs.bitfinex.com/reference/rest-auth-cancel-orders-multiple) | `POST /v2/auth/w/order/cancel/multi`: `id` (array), `cid` (`[[cid, date], ...]`), `all: 1` "cancels all open orders (both trading and derivative)" (never sent) |
| [Order Multi-OP](https://docs.bitfinex.com/reference/rest-auth-order-multi) | "Max 75 operations per request": the batch size used by analogy (see Doc gaps); not called |
| [Retrieve Orders](https://docs.bitfinex.com/reference/rest-auth-retrieve-orders), [by symbol](https://docs.bitfinex.com/reference/rest-auth-retrieve-orders-by-symbol) | working orders; `cid` + `cid_date` filter; `id` an integer on the first, an array on the second |
| [Orders History](https://docs.bitfinex.com/reference/rest-auth-orders-history), [by symbol](https://docs.bitfinex.com/reference/rest-auth-orders-history-by-symbol) | ended orders "up to 2 weeks in the past", `start`/`end` on `MTS_CREATE`, `limit` "max. 2500", `id` array; no client id filter |
| [Trades by symbol](https://docs.bitfinex.com/reference/rest-auth-trades-by-symbol) | fills: `start`, `end`, `limit` "max. 10000", `sort` (+1 ascending); the trade array |
| [Retrieve Positions](https://docs.bitfinex.com/reference/rest-auth-positions) | `POST /v2/auth/r/positions` (no symbol): `AMOUNT` "Positive for long and negative for short", `LEVERAGE` (9), `TYPE` (15) "0 = Margin position, 1 = Derivatives position" |
| [Configs](https://docs.bitfinex.com/reference/rest-public-conf) | `pub:info:pair`, `pub:info:pair:futures`: `MIN_ORDER_SIZE` (3), `MAX_ORDER_SIZE` (4) |
| [Introduction](https://docs.bitfinex.com/docs/introduction) | price precision ("up to 5 significant digits and up to 8 decimals ... Prices submit with a precision larger than 5 will be cut by the API"), amount precision ("allows up to 8 decimals. Anything exceeding this will be rounded"), "Do not parse text descriptions, use only codes", derivatives symbols |
| [Flag Values](https://docs.bitfinex.com/docs/flag-values) | Reduce Only 1024, Post Only 4096 (Hidden 64, Close 512, OCO 16384 not used) |
| [Abbreviation Glossary](https://docs.bitfinex.com/docs/abbreviations-glossary) | the error and info codes, the order statuses ("A complete overview"), the stream message types |
| [Requirements and Limitations](https://docs.bitfinex.com/docs/requirements-and-limitations) | the nonce ("strictly increasing", one per key across REST and socket), `{"error": "ERR_RATE_LIMIT"}`, the IP "blocked for 60 seconds" |
| [Authenticated Channels](https://docs.bitfinex.com/docs/ws-auth), [Account Info](https://docs.bitfinex.com/reference/ws-auth-account-info) | `wss://api.bitfinex.com/ws/2`, the login frame (`AUTH` + nonce, HMAC-SHA384), "Subscribing to the Account Info channel is automatically done when you authenticate", the `trading` filter, `dms` |
| [WS General](https://docs.bitfinex.com/docs/ws-general) | heartbeat "Every 15 seconds", ping/pong, info codes 20051, 20060, 20061 |
| [WS Orders](https://docs.bitfinex.com/reference/ws-auth-orders), [WS Positions](https://docs.bitfinex.com/reference/ws-auth-positions), [WS Trades](https://docs.bitfinex.com/reference/ws-auth-trades) | `os`/`on`/`ou`/`oc`, `ps`/`pn`/`pu`/`pc`, `te`/`tu` frames |
| [WS New Order](https://docs.bitfinex.com/reference/ws-auth-input-order-new) | "Price (Not required for market orders)" |
| [REST JavaScript authenticated request](https://docs.bitfinex.com/recipes/restjavascript-authenticated-request-example) | `/api/` + path + nonce + JSON body, HMAC-SHA384 |
| [Paper Trading](https://docs.bitfinex.com/docs/paper-trading), [Changelog](https://docs.bitfinex.com/docs/changelog) | paper sub-accounts on the live hosts; "adjust the remaining amount" on an update |

### Order types and flags

| Canonical | Spot | Derivatives | Notes |
| --- | --- | --- | --- |
| Market | `EXCHANGE MARKET` | `MARKET` | no `price` (the socket page: "Not required for market orders"); as IOC it is the same order |
| Limit GTC / GTD | `EXCHANGE LIMIT` | `LIMIT` | `price`; GTD as `tif` |
| Limit IOC | `EXCHANGE IOC` | `IOC` | a type of its own, limit at `price` |
| Limit FOK | `EXCHANGE FOK` | `FOK` | a type of its own, limit at `price` |
| Stop | `EXCHANGE STOP` | `STOP` | trigger at `price`, market once triggered; GTC or GTD |
| Stop limit | `EXCHANGE STOP LIMIT` | `STOP LIMIT` | trigger at `price`, limit at `price_aux_limit` |
| Post-only | flag 4096 | flag 4096 | a resting limit only (GTC or GTD) |
| Reduce-only | refused | flag 1024 | "Ensures that the executed order does not flip the opened position" |
| Close position | refused | reduce-only `MARKET` of the held size | the close flag 512 is not used (its effect on `amount` is not documented) |

Refused before anything leaves: a time in force a type cannot carry (a stop IOC or FOK, a
market FOK or GTD), post-only on anything but a resting limit, a GTD between two whole
seconds (both offered), a symbol of the other book. Not declared, refused by the caps:
trailing stop, OCO (flag 16384), hidden (64), attached exits, iceberg, day orders, dry run.

### Amend

Update Order, in place, by the order's `id`: prices only (`price`, the stop limit's
`price_aux_limit`). A size change is refused (the doc does not say whether `amount` is the new
size or what is left, and `AMOUNT_ORIG` keeps the size "before any update", so what filled
could no longer be read); a time in force is left as placed ("can be updated but not
removed"). A derivatives change carries the order's own leverage ("Mandatory for derivative
order updates"): `META.$F33` ("Leverage (int)", Retrieve Orders) of the read made just before,
else the one the working orders on the instrument state; with none, the change is refused,
naming cancel and replace.

### Client order id

`cid`, an integer of at most 45 bits, "unique in the day (UTC+0)", not enforced. OpenTraderWorld
mints it (`mints_client_ids`): the milliseconds of the venue clock it was made at, as the doc's
own examples do (`cid: Date.now()`), strictly increasing across the process, stored with the leg
before anything leaves, handed back after a restart. Its `cid_date` is its
own date; within five minutes of midnight UTC the next or previous day is asked too (the doc
does not say whether the date is the caller's or the venue's creation time).

Lookups: the working orders by `cid` + `cid_date` (Retrieve Orders), one call per date, then the
ended orders on the symbol created from five minutes before to an hour after the mint time
(Orders History has no client id filter: rows are matched by their `CID`). A full page of
ended orders ("max. 2500") is followed, up to four pages: the doc states no sort, so the page's
own order says which bound moves (newest first: `end` to its last `MTS_CREATE`; oldest first:
`start`). Orders History reaches back "up to 2 weeks": an ended order older than that is not
found. By venue id: the
working orders on the symbol, then the ended ones (`id` array). A stream order maps back to its
OTW key through the minted ids held; an order placed elsewhere (another or no `cid`) has none.

### Order status

Read on the head of the status string (`HEAD[ @ PRICE(AMOUNT)][ was: ...]`), from the glossary:

| Head | Canonical |
| --- | --- |
| `ACTIVE` | New; PartiallyFilled when `AMOUNT` differs from `AMOUNT_ORIG` |
| `PARTIALLY FILLED` | PartiallyFilled on the working lists; Cancelled (with its fill) on Orders History ("the remainder may have been canceled") |
| `EXECUTED`, `FORCED EXECUTED` | Filled |
| `CANCELED`, `POSTONLY CANCELED`, `FILLORKILL CANCELED`, `IOC CANCELED` | Cancelled |
| `INSUFFICIENT BALANCE (G1)`, `INSUFFICIENT BALANCE (U1)`, `INSUFFICIENT MARGIN` | Cancelled |
| `RSN_POS_REDUCE_INCR`, `RSN_POS_REDUCE_FLIP`, `RSN_POS_NOTFOUND` | Cancelled (reduce-only) |
| `RSN_BOOK_SLIP`, `RSN_PAUSE` | Cancelled |
| `RSN_DUST` | Unknown: a dust position closed by a market order, the doc says nothing of a fill |
| anything else | Unknown, alerts |

Filled size: `AMOUNT_ORIG` minus `AMOUNT` (what is left), both signed like the order. An order of
a type the doc does not list (`LIMIT`, `MARKET`, `STOP`, `STOP LIMIT`, `TRAILING STOP`, `FOK`,
`IOC`, with or without `EXCHANGE`) is read with an Unknown state: its prices cannot be told
apart. Reduce-only from the `FLAGS` sum.

### Instrument trading rules

From `pub:info:pair` (spot) and `pub:info:pair:futures`: `min_qty` = `MIN_ORDER_SIZE`, `max_qty`
= `MAX_ORDER_SIZE`, `lot_step` 0.00000001 (8 decimals), sizes in the base coin on both books
(the measured lists and the account read agree; no contract size). No tick: prices are checked
against 5 significant digits and 8 decimals before anything leaves, refused with the two nearest
prices that pass (`123456` gives `123450 or 123460`), never rounded. No status per pair: a listed
pair trades, a delisted one leaves the list. Paper pairs (`TEST...`) are left out.

### Positions, leverage, margin

Derivatives only. One net position per instrument ("Positive for long and negative for short"):
the mode is one-way whatever the rows say, so an order on a flat instrument is not refused for
an unknown mode. Retrieve Positions takes no symbol: a read for one instrument keeps that
instrument's rows (an endpoint id per instrument, `positions:<symbol>`, at most 512 held). Margin
rows (`TYPE` 0, spot pairs) are not this book's.

Leverage: OpenTraderWorld never chooses it. An order carries `lev` = the leverage the working
orders on the instrument state (`META.$F33`, "Leverage (int)", Retrieve Orders by symbol, read
before the order when none is held, held until a positions read or a flat stream frame). None
states one, or they state different ones: no `lev`, and the venue applies its own default
("If omitted the default leverage value of 10 will be used"). A position's `LEVERAGE` is its
measured ratio (`1.409...`, `3.067...` in the doc's examples), not a leverage an order carries:
it is not read. Margin mode: none per order (doc silent on cross or isolated).

### Errors

Read from the body before the HTTP status: a refusal is `["error", CODE, "message"]` with HTTP
500, or an HTTP 200 notification whose `STATUS` is not `SUCCESS`.

| Code or answer | Canonical |
| --- | --- |
| 10000 ERR_UNK, 10001 ERR_GENERIC, 10008 ERR_CONCURRENCY, 11000 ERR_READY, 20051, 20060, 20061, 5000 | Unknown: looked up, never sent again |
| 10001 with "permission" (ccxt's sample) | Permission |
| 10001 with "Invalid order: ..." (an order refusal's words, ccxt) | Rejected, in the venue's words |
| 10020 ERR_PARAMS | InvalidRequest |
| 10050, 10200, 10300, 10301, 10302, 10305, 10400, 10401 (socket codes) | InvalidRequest |
| 10100, 10111, 10112, 10113, 10114 | Auth |
| `{"error": "ERR_RATE_LIMIT"}`, 11010 `ratelimit: error`, HTTP 429 | RateLimited, new orders held 60 s ("the IP is blocked for 60 seconds") |
| notification `ERROR` or `FAILURE` on a submit or an update | Rejected, in the venue's words ("Bitfinex refused it: TEXT", with `(code CODE)` when `CODE` is set) |
| notification `ERROR` or `FAILURE` on a cancel | Rejected(Final): the order is read by its id; already final is no error, still working shows the refusal |
| notification with another status word | Unknown, alerts |
| HTTP 5xx without the error array, a timeout, an answer not read | Unknown |
| any other code | Unknown: "Bitfinex answered code X (msg), which OpenTraderWorld does not map", alerts |

The read path (`errors.rs`) keeps its own mapping: there 10000, 10008, 11000 and 20060 are
Transient; on the order path they are Unknown. 11010 (`ratelimit: error`, ccxt only, not in the
glossary) is RateLimited on both paths.

### Rate limits for orders

"Rate Limit: 90 reqs/min" on each order and read page, per IP for REST
(requirements-and-limitations): declared per endpoint id (`orderSubmit`, `orderUpdate`,
`orderCancel`, `orderCancelMulti`, `orders`, `ordersSymbol`, `ordersHist`, `tradesSymbolHist`,
`positions`). Signed calls of one key go out one at a time (`serial_key`), the socket login in
the same turn: the nonce must reach the venue in the order it was made.

### Cancel all

The kill switch names the book's working orders by id on Cancel Orders (multiple), 75 a call,
then cancels what is left one by one. `all: 1` is never sent: it "cancels all open orders (both
trading and derivative)", other programs' and the other book's alike. The spot book's list is
every working order on a spot pair (exchange and margin types: a margin order placed elsewhere
is covered); the derivatives book's every order on a perpetual.

### Stream

`wss://api.bitfinex.com/ws/2`, one per book on the same key. Login `{"event":"auth","apiKey",
"authSig","authNonce","authPayload":"AUTH"+nonce,"filter":["trading"]}`, answered `{"event":
"auth","status":"OK"}` or `FAIL`; no subscribe frame (the login subscribes), no `dms`. Ping
`{"event":"ping","cid":1}` every 15 s, as often as the server's heartbeat `[0,"hb"]`. Channel 0:
orders `os` (a list), `on`/`ou`/`oc` (one; `oc` is "canceled or fully executed", its status
says which), positions `ps`/`pn`/`pu`/`pc` (`pc`, and a `pn`/`pu` whose status is `CLOSED` or
whose amount is 0, read as flat), trades `te` (no fee yet) and
`tu` (with the fee). Each book reads its own symbols. `info` 20051 and 20061 reconnect; 20060
("maintenance starting", "pause any activity") is held: while it is announced, a new order
(and a position close) sent through OpenTraderWorld is refused at once, naming the
maintenance; cancels still go out. Nothing is queued, held or retried. 20061 or a reconnect
clears it. Orders are never sent over the socket: its inputs carry no request id to match
an answer by.

### Disconnect behaviour

No dead man's switch (`dms: 4` "when socket is closed, cancel all account orders": every book,
every program): never asked for. Working orders stay working when OpenTraderWorld stops.

### Dry-run

None: the venue has no order check (`/auth/calc/order/avail` answers an available amount, not
a check). The E2D session check does not apply.

### Paper accounts

Not handled. Paper trading is a sub-account on the live hosts, on paper pairs only (`tTESTBTC:
TESTUSD`, `tTESTBTCF0:TESTUSDTF0`); the account says it is one in `/auth/r/info/user`
(`PPT_ENABLED`), which needs a signed read the sandbox flag cannot make. A paper key is
treated as a live one (E2D opt-in), and its `TEST` pairs are left out of the lists.

### Doc gaps

- Whether a notification `ERROR` on a submit ever leaves an order, and what `FAILURE` means
  apart from `ERROR` (both read as refusals); refusals carry no code (`CODE` is "W.I.P.").
- `cid_date`: the caller's date or the venue's creation date; behaviour near midnight UTC.
- No client id filter on Orders History; its sort order with `start`/`end`.
- The most ids one `cancel/multi` takes (75 used by analogy with Multi-OP), and how a failed id
  is shown in its answer.
- `amount` on Update Order: the new size or what is left (the changelog says "adjust the
  remaining amount"; the REST page says "Amount of order").
- `price` on a market order: required on the REST schema, "Not required for market orders" on
  the socket page; no value for a market order is stated anywhere (not sent, as ccxt does).
- Whether an order without `lev` on an instrument with a position changes that position's
  leverage; whether all working orders on one instrument always state the same `$F33`.
- Whether Update Order without `flags` keeps or clears post-only and reduce-only (not sent).
- `info` 20060 asks to "pause any activity" until 20061, without saying whether orders sent
  meanwhile are refused or queued: OpenTraderWorld refuses new ones itself until 20061 or a
  reconnect; cancels are not held back.
- The stop's trigger price source (last, mark); the status of a stop not yet triggered.
- The unit of `MIN_ORDER_SIZE`/`MAX_ORDER_SIZE` and the contract size of a perpetual (base coin
  assumed, as measured on the lists and the account read).
- `RSN_DUST` outcome; the fee sign of a rebate.
- Notification `MTS`: seconds on submit, milliseconds elsewhere (not read).
- Reduce-only on derivatives: the 2018 changelog says "Applies to Margin orders only"; the flag
  page has no such limit.

### What stays unproven

Real refusal bodies and texts, the ack timing, the stream's event order on a fill, the
leverage a derivatives position states, whether a market order without `price` is taken. The
first live order on an account opted in at E2D is the canary.

### Fixtures

`fixtures/trade/`: the doc's examples with only their `//` comments and `[...]` placeholders
taken out (`doc_*.json`), and two measured public lists (`measured_info_pair*.json`). The
catalog's and the derivatives answers are built on them (`synthetic`). Listed in
`trade_tests.rs`.
