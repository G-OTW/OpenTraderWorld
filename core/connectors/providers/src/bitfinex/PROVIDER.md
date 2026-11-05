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
  `sort=-1`, 1000 rows a call (the docs allow 2500, ccxt writes 1000; the smaller is used):
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
