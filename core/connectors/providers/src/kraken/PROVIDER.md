# Kraken (spot)

Adapter version: 0.1.0        Provider API version: REST `0`, socket v2        Maturity: M2

Sources:
- REST OHLC: https://docs.kraken.com/api/docs/rest-api/get-ohlc-data
- REST pairs, assets, ticker: https://docs.kraken.com/api/docs/rest-api/get-tradable-asset-pairs
- Socket v2 OHLC: https://docs.kraken.com/api/docs/websocket-v2/ohlc
- Authentication: https://docs.kraken.com/api/docs/guides/spot-rest-auth
- Rate limits: https://docs.kraken.com/api/docs/guides/spot-rest-ratelimits
- Errors: https://docs.kraken.com/api/docs/guides/spot-errors

M2: mock catalog green; shadow against the live venue (history, pair list, socket)
identical to the legacy connector; account parity on mock, real shadow ready with a key.

## Endpoints

| Endpoint id | Method + path / WS channel | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `ohlc` | `GET /0/public/OHLC?pair&interval&since` | `RequestHistory{Candles}` | 1 | none: the last 720 bars whatever the window, filtered to it |
| `assetPairs` | `GET /0/public/AssetPairs` | `LoadInstruments` | 1 | none |
| `pairNames` | `GET /0/public/AssetPairs` | session names for account rows and the board (kept one hour) | 1 | none |
| `ticker` | `GET /0/public/Ticker?pair` | `RequestSnapshot{Quote}` | 1 | none |
| `tickers` | `GET /0/public/Ticker` | `RequestBoard` | 1 | none |
| `assets` | `GET /0/public/Assets` | session asset names for balances | 1 | none |
| `private:balance` | `POST /0/private/Balance` (signed) | `QueryAccount{Summary}`, `{Balances}` | 1 counter point | none |
| `private:openOrders` | `POST /0/private/OpenOrders` (signed) | `QueryAccount{OpenOrders}` | 1 counter point | none |
| `private:tradesHistory` | `POST /0/private/TradesHistory` (signed), body `type=all&trades=false&start&end&with_cursor=true&limit=100[&cursor]` | `QueryAccount{Fills}` | 2 counter points | cursor: `cursor` from the previous page's `cursor.next` ("Only present when `with_cursor` is `true` and more results are available"), no cursor ends the pull; 100 rows ("1-100"), 200 pages max; OTW cursor `<pages>:<cursor.next>`; `ofs` (deprecated) never sent |
| socket `ohlc` | `wss://ws.kraken.com/v2`, `{"method":"subscribe","params":{"channel":"ohlc","symbol":[..],"interval":N}}` | `Subscribe{Candles}` | | one socket per interval |

## Auth

`API-Sign` = base64 `HMAC-SHA512(base64-decoded secret, path ++ SHA256(nonce ++ body))`,
`API-Key` header, body form-encoded with the nonce first. The nonce is a process-wide
strictly increasing counter (two calls in one millisecond are otherwise refused with
`EAPI:Invalid nonce`). No recv window, no clock call. Secrets: `api_key`, `api_secret`; a
secret that is not base64 is refused before any call. Signature checked against Kraken's
documented example. A key's signed calls (account read, orders, socket token) go out one at a
time, each answered before the next is signed (`Signer::serial_key`, a hash of the key), so
their nonces reach Kraken in the order they were made: "Problems can arise from requests
arriving out of order due to API keys being shared across processes"
([Authentication](https://docs.kraken.com/exchange/guides/rest/authentication)). A key also used
by another program can still meet `EAPI:Invalid nonce`; Kraken's optional "nonce window" on the
key is the fix.

## Rate limits

- Public REST: about one request per second per IP; OTW's worker paces history at 1.1 s.
- Private REST: a call counter per key ([REST rate limits](https://docs.kraken.com/exchange/guides/rest/ratelimits)):
  Starter at most 15, decaying 0.33/s; Intermediate 20, 0.5/s; Pro 20, 1/s. "Ledger/trade
  history calls increase the counter by `2`. All other API calls increase this counter by `1`
  (except AddOrder, CancelOrder which operate on a different limiter)". OTW has no tier setting:
  the Starter counter is declared (the most conservative), one bucket of 15 refilled in
  15 / 0.33 s (`endpoint = "private"`). Every counted call goes out under an endpoint id
  `private:...` (the account read, the order lookups and lists, AmendOrder, CancelAll, the
  socket token), TradesHistory with weight 2; AddOrder (validate included) and CancelOrder do
  not count. The bucket is the shared driver's, so several keys share it (slower, never over).
- Over the limit: HTTP 200 with `EAPI:Rate limit exceeded` (`RateLimited`); the guide gives no
  wait ("restricted for a few seconds"), so none is set. No ban status.

## Symbols

Kraken spells a pair three ways: key (`XXBTZUSD`), altname (`XBTUSD`) and wsname
(`XBT/USD`). Native id = altname (what OTW stores for market data); display symbol =
wsname (the altname when absent); base and quote are the wsname's two halves. Account
rows, the price board and the pairs a holding is valued on use the wsname, as OTW stored
them before the port (`account.display_symbols`). Rows naming a pair by key or altname are
turned into the wsname through the pair list. The socket names pairs as `BTC/USD`.

Balance codes (`XXBT`) are named through the asset list (`XBT`); an unknown code
(`ETH2.S`) reads as itself.

## Enum tables

| Kraken | Canonical |
| --- | --- |
| side `buy` / `sell` | `Side::Buy` / `Side::Sell`; anything else `Unknown(raw)` |
| interval 1, 5, 15, 60, 240, 1440, 10080 | `1m 5m 15m 1h 4h 1d 1w` (30 and 21600 exist, not declared) |
| pair status absent or `online` | active; any other status inactive |
| order type (`limit`, `stop-loss`...) | kept verbatim |
| open order `descr.price` / `price2` | by type, as AddOrder states them: `price` the limit of `limit` and `iceberg`, the trigger of `stop-loss`, `take-profit` and their `-limit` forms; `price2` the limit of `stop-loss-limit` and `take-profit-limit`; a trailing order's offsets are not prices (none shown) |

Errors:

| Error | Canonical |
| --- | --- |
| `error` array with no `E...` string (empty, or warnings only: "`W` for warning" on a request that succeeded, [REST introduction](https://docs.kraken.com/exchange/guides/rest/introduction)) | none (HTTP status decides) |
| `EAPI:Rate limit exceeded`, any "rate limit" / "too many requests", `EService: Throttled: [UNIX timestamp]` | RateLimited |
| `EAPI:Invalid key`, `EAPI:Invalid signature`, `EAPI:Invalid nonce`, `EGeneral:Temporary lockout`, `EAuth:Account temporary disabled`, `EAuth:Account unconfirmed` | Auth |
| `EGeneral:Permission denied`, `EAccount:Invalid permissions` | Permission |
| `EQuery:Unknown asset pair` | NotFound ("bad symbol") |
| `EService:Unavailable`, `EService:Deadline elapsed`, `EGeneral:Internal error` | Transient |
| anything else | InvalidRequest |

Codes are the [error guide](https://docs.kraken.com/exchange/guides/general/errors)'s (and
`EQuery:Unknown asset pair`, TradesHistory `pair`), matched by prefix on a `:` without case, the
spaces around `:` dropped, as the trade path matches them: "The error may include a numeric code
(e.g. `EGeneral:Internal error:5`)".

## Units and time

- Quantities in base units (spot); prices and volumes as decimal strings on REST, JSON
  numbers on the socket (read from their text).
- OHLC row `[time, o, h, l, c, vwap, volume, count]`, `time` in seconds, candle open time.
  `last` is the open time of the newest committed bar; a row after it is forming.
- Fill `time` and order `opentm`: seconds with a fraction, kept to the millisecond.
- Fees positive (cost), in the pair's quote currency.
- Candles are UTC epoch aligned; `1w` follows Kraken's 10080-minute interval.
- Socket rows carry no closed flag (`live.candles_final_flag = false`): every candle goes
  out forming and the engine closes it when the next period starts.

## Quirks

- `since` keeps bars opening at or after it, and the answer always ends with the forming
  bar whatever `since` is (checked against the live venue).
- The OHLC series sits under whatever key Kraken normalized the pair to, next to `last`.
- Open orders name the pair by altname, fills by key.
- Socket rows do carry `interval` (the legacy note said otherwise); a socket keeps one
  interval and drops a row of another.
- A subscription refusal (`success: false`) names its symbol: it fails that pane; one
  naming nothing is the socket's.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers the window, committed bars only.
- An unknown pair reads as a symbol problem in the chart's notice; `EService:*` is retried.
- Fill and order times keep their milliseconds.
- An open order on a pair whose key differs from its altname is named by its wsname with
  its currency.
- Suggestions name held assets through the asset list (legacy turned `XXBT` into `BT`)
  and add pairs with a working order.
- Journal symbols filter fills by wsname; a raw pair key no longer matches.
- A fill pull longer than 200 pages (20,000 rows) is an error asking for a shorter period.
- The live provider time reads to the second, as on every ported feed.
- A refused socket subscription reaches its pane.

## Regions and terms

Kraken does not serve some jurisdictions; public market data answered from the test
machine. No sandbox for spot.

## Live best bid and ask

v2 channel `ticker` with `event_trigger: "bbo"`, a socket of its own (class `""`; candle
sockets are per interval): snapshot then an update on every change of the best bid or ask
(`bid`, `ask`, `bid_qty`, `ask_qty`, `timestamp`). An unknown pair is refused by name (measured
2026-10-19).

## Trading

Execution maturity: **E2D** (spot), built from the documentation alone: no account, no key, no
canary. Sandbox: none for spot (the "Beta" socket hosts run on "the production trading engine";
the Futures demo is another venue), so every order is live money. Code: `trade.rs` (mapper),
`trade_errors.rs` (error table); the order path is the shared trader's
(`connectors/engine/src/trading.rs`). Caps: `[orders]` in `capabilities.toml`.

Doc snapshot, read 2026-11-12: the spot REST OpenAPI https://docs.kraken.com/openapi/spot-rest.yaml
(last-modified Tue, 27 Oct 2026 13:47:31 GMT, etag `W/"ROdEgtqQ0bOt87xc"`, sha256
`4e730d0c...0e1f`, `info.version` 1.1.0), from which the API reference pages are generated
(cited below as Y, with the schema or path), and https://docs.kraken.com/llms-full.txt (sha256
`407fd242...fd43`) for the guides and the socket reference.

| Page | Used for |
| --- | --- |
| [Add Order](https://docs.kraken.com/api-reference/trading/add-order) (Y `/private/AddOrder`, `add`, `add-2`, `ordertype`, `oflags`) | `pair` "Asset pair `id` or `altname`", `volume` "Order quantity in terms of the base asset", `price` (limit, or trigger of the triggered types), `price2` (the limit of the `-limit` types), `trigger` `last`/`index`, `timeinforce` GTC (default) IOC GTD FOK, `expiretm`, `oflags` `post` / `fciq` / `fcib` / `viqc`, `cl_ord_id`, `validate`, answer `txid` and `descr` |
| [Amend Order](https://docs.kraken.com/api-reference/trading/amend-order) (Y `amend`, `amend-2`), [Atomic amends](https://docs.kraken.com/exchange/guides/general/amends) | `txid`, `limit_price`, `trigger_price`, `order_qty`, `post_only` ("If `true`, the limit price change will be rejected if the order cannot be posted passively in the book"); identifiers kept; "amending the order quantity to below the filled quantity will be accepted" (the rest cancelled); "Orders with conditional close terms attached are not supported" |
| [Cancel Order](https://docs.kraken.com/api-reference/trading/cancel-order), [Cancel All Orders](https://docs.kraken.com/api-reference/trading/cancel-all-orders) (Y `cancel`, `cancel-2`) | by `txid`; "Cancel all open orders"; answer `count`, `pending` |
| [Get Open Orders](https://docs.kraken.com/api-reference/account-data/get-open-orders), [Get Closed Orders](https://docs.kraken.com/api-reference/account-data/get-closed-orders), [Query Orders Info](https://docs.kraken.com/api-reference/account-data/query-orders-info) (Y `openOrders`, `closedOrders`, `query`, `open`, `closed`, `query-2`) | `cl_ord_id` "Restrict results to given client order id", `with_cursor`, `limit` (100 at most with a cursor), `cursor.next`; `status`, `vol`, `vol_exec`, `misc`, `oflags`, `reduce_only`, `opentm` |
| [Get Trades History](https://docs.kraken.com/api-reference/account-data/get-trades-history) (Y `history`, `trade-2`) | `start` (exclusive), `end` (inclusive), `limit` 1 to 100, `with_cursor`; `ordertxid`, `pair`, `time`, `vol`, `price`, `fee` "Total fee (quote currency)" |
| [Get Tradable Asset Pairs](https://docs.kraken.com/api-reference/market-data/get-tradable-asset-pairs) (Y `pairs`) | `altname`, `wsname`, `tick_size`, `lot_decimals`, `ordermin`, `costmin`, `cost_decimals`, `lot_multiplier`, `status` |
| [Get Websockets Token](https://docs.kraken.com/api-reference/trading/get-websockets-token), [WebSocket authentication](https://docs.kraken.com/exchange/guides/websockets/authentication) | the token "should be used within 15 minutes of creation" |
| [executions](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/executions), [ping](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/ping), [heartbeat](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/heartbeat), [WebSockets introduction](https://docs.kraken.com/exchange/guides/websockets/introduction) | `wss://ws-auth.kraken.com/v2`, subscribe with `token`, `snap_orders`, `snap_trades`, `order_status`; frames "dependant on the `exec_type`"; one minute of inactivity closes the socket |
| [Spot Client Order Identifiers](https://docs.kraken.com/exchange/guides/general/clordid) | UUID (long or short) or free text up to 18 characters, "stored as a 128-bit integer", unique "across open orders for each client" |
| [Order lifecycle](https://docs.kraken.com/exchange/guides/general/order-lifecycle) | states, `pending` to `canceled` on "Validation failure (insufficient funds, invalid params, rate limit)", "CLOSED (fully filled from partial, or canceled partial)", `reason` |
| [Error messages](https://docs.kraken.com/exchange/guides/general/errors), [REST introduction](https://docs.kraken.com/exchange/guides/rest/introduction) | the error table, `E`/`W` severity, non-200 statuses |
| [REST rate limits](https://docs.kraken.com/exchange/guides/rest/ratelimits), [Spot trading limits](https://docs.kraken.com/exchange/guides/general/ratelimits) | the call counter, `EService: Throttled: [UNIX timestamp]`, the per-pair engine counter, open orders per pair |
| [Authentication](https://docs.kraken.com/exchange/guides/rest/authentication), [API key permissions](https://docs.kraken.com/exchange/guides/rest/api-keys) | signature and nonce (see Auth above); "Create & Modify Orders", "Cancel/Close Orders", "Query Open Orders & Trades", "Query Closed Orders & Trades", "Access WebSocket API" |

### Order types and flags

| Canonical | Native | Notes |
| --- | --- | --- |
| `market` | `ordertype=market` | sized in the base coin (`viqc` is not sent: a market buy is never an amount to spend). Market price protection cannot be turned off (`nompp` "no longer supported"), so a market order can end cancelled with part filled |
| `limit` | `ordertype=limit`, `price` | |
| `stop` | `ordertype=stop-loss`, `price` = trigger, `trigger=last` | "A market order is triggered when the reference price reaches the stop price (from an unfavourable direction)" |
| `stop_limit` | `ordertype=stop-loss-limit`, `price` = trigger, `price2` = limit | |
| `mit` | `ordertype=take-profit`, `price` = trigger | "(from a favourable direction)" |
| `lit` | `ordertype=take-profit-limit`, `price` = trigger, `price2` = limit | |
| `post_only` | `oflags=post` | "available when ordertype = limit" |
| fee currency | `oflags=fciq` on every order | "`fcib` prefer fee in base currency (default if selling)": sent always, so a sell's fee is in the quote too, as fills are read |
| trigger | `last` | the only one offered; `index` is left out |
| amend | `AmendOrder` by `txid`: `limit_price`, `trigger_price`, `order_qty` | in place, ids kept (`amend` declared). The time in force is not amendable: refused before I/O. A post-only order (read with `oflags` `post`) has its new limit sent with `post_only=true`, so it stays a maker order |
| not offered | | margin (`leverage`, `reduce_only`, `settle-position`, positions), the attached close order (`close[ordertype]`: one exit, one child per fill, linked by the parent's venue id), trailing stops, iceberg, `viqc`, xStocks (`asset_class`), `EditOrder` (cancel-new, "`cl_ord_id` is not supported"), batch add and cancel, orders over the socket, the dead man's switch (`CancelAllOrdersAfter`) |

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| `gtc` | none sent | "GTC ... is default if the parameter is omitted" |
| `ioc` | `timeinforce=IOC` on a limit | a market order asked IOC goes as a market order (it fills at once); a triggered order IOC is refused before I/O (the doc says nothing of it) |
| `fok` | `timeinforce=FOK` | "Available for `limit` orders only" ([add_order v2](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/add_order)): any other type FOK is refused before I/O |
| `gtd` | `timeinforce=GTD`, `expiretm` (unix seconds) | "up to one month in future"; a market order GTD is refused before I/O |

### Client order id

`cl_ord_id`: "Long UUID ... 32 hex characters separated with 4 dashes", "Short UUID ... 32 hex
characters with no dashes", or "Free format ascii text up to 18 characters". An OTW key of 1 to
18 letters, digits, `-` and `_` goes as it is (chart and trade-on-alert keys). A key of 19 to 24
lower-case letters and digits (a robot's, 24, starting `ots`) is packed into a long UUID: the key
read as a base-36 number past every shorter key (so its length is read back), in 124 bits, the
UUID's version nibble (13th hex digit) set to `a`. No UUID version (1 to 8) has `a` there, so a
UUID placed elsewhere is never read as an OTW key (without the mark, 6.8% of v4 UUIDs did). It is
read back from any spelling of the UUID the venue echoes (long or short, any case), leading zeros
included. A 24-character key from about `x` up does not fit in 124 bits, and any other key, is
refused before I/O naming the rule. Unique "across open orders for each client" only: a closed
order's id can be reused. The REST AddOrder answer echoes no client id (`txid` only). A
duplicate's refusal code is not documented: it lands as an unmapped answer (`Unknown`, looked
up, never resent). Lookup: OpenOrders by `cl_ord_id`, then ClosedOrders by `cl_ord_id`.

### Order status

| Native | Canonical |
| --- | --- |
| `pending` "order pending book entry" | `PendingNew` |
| `open`, `vol_exec` 0 | `New` |
| `open`, `vol_exec` above 0 | `PartiallyFilled` |
| `closed`, `vol_exec` equal to `vol` | `Filled` |
| `closed`, `vol_exec` under `vol` | `Cancelled` with what filled ("CLOSED (fully filled from partial, or canceled partial)": inference, see doc gaps) |
| `canceled` | `Cancelled` (what filled from `vol_exec`; `reason` "User requested", "Immediate or cancel", "Order would trigger immediately"...) |
| `expired` | `Expired` |
| anything else | `Unknown` (alerts) |

There is no rejected state on REST: an AddOrder answered with a `txid` can still end
`canceled` for funds ("pending -> canceled: Validation failure"). A triggered type (`stop-loss`,
`take-profit`, their `-limit` and trailing forms) working with neither `stopped` nor `touched`
in `misc` is shown waiting for its trigger (inference: the REST rows carry no trigger state).
A market buy sized in the quote currency placed elsewhere (`oflags` `viqc`) reads its filled
size as its size, on the read path and the trade path alike: sizes are in the base coin on both.
An order row's `descr.price` / `price2` that is not a number (an order placed elsewhere priced
relative, `+5%`) is read as no price: the row is kept and the list reads. A pair the list read
does not name is an error on the trade path (the order and fill decoders), never its raw
spelling; a fill's fee currency is the quote of its wsname (a pair without one is an error).

### Instrument trading rules

`GET /0/public/AssetPairs` (public, the whole list), read once a day per pair and after a rule
refusal, and once a day whole for the name table. Each pair is listed twice: under its altname
(`XBTUSD`, how market data and the chart name it) and under its wsname (`XBT/USD`, how the
account read names it), each naming the other and the pair key as aliases. `tick_size` (else
`pair_decimals`) is the tick, `10^-lot_decimals` the size step, `ordermin` the smallest size
("in terms of base currency"), `costmin` the smallest value ("in terms of quote currency"),
`10^-cost_decimals` the quote step. No largest size is stated. A pair whose `lot_multiplier` is
not 1 is listed as not taken (its volume is not the base coin). `status`: `online` trades;
`post_only` takes post-only limit orders only; `limit_only` limit orders only; `cancel_only`,
`reduce_only` (a margin position's state), `delisted`, `maintenance`, `work_in_progress` and
any new word are closed, the word shown. An order on a wsname goes out under its altname, from
the pair list read (never by cutting the slash).

### Errors

HTTP 200 with `{"error":[...],"result":...}`; any other status is an answer lost (`Unknown`,
looked up, never resent): "Status codes other than 200 indicate that there was an issue with
the request reaching our servers". A `W...` string is a warning on a success. Codes are matched
by prefix, without case, the spaces around `:` dropped (`EGeneral:Invalid arguments:ordertype`,
`EGeneral:Internal error:5`, `EService: Throttled: 1700000000`); the longest code listed wins. A
refusal reads "Kraken refused it: <the venue's strings> (code <code>)"; an unlisted code
"Kraken answered code X (...), which OpenTraderWorld does not map" (`Unknown`, alerts). A cancel
answered `count: 0` cancelled nothing: `Rejected(not found)`, and the order is read by its id
to say how it ended.

| Code | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| `EAPI:Invalid key` | key missing, malformed or revoked | Auth | fix the key; retrying an unknown key locks the IP out |
| `EAPI:Invalid signature` | signature mismatch | Auth | fix the secret |
| `EAPI:Invalid nonce` | nonce not above the last one accepted | Auth | another program uses the key: give OpenTraderWorld its own key, or set a nonce window |
| `EGeneral:Temporary lockout` | too many unknown keys from the IP, "about 20 minutes" | Auth | wait |
| `EGeneral:Permission denied` | the key lacks the permission | Permission | add Create & Modify Orders, Cancel/Close Orders |
| `EAuth:Account temporary disabled` | account suspended | Auth | Kraken support |
| `EAuth:Account unconfirmed` | email not verified | Auth | verify it |
| `EAuth:Rate limit exceeded` | too many authentication attempts | RateLimited | back off |
| `EAuth:Too many requests` | too many authentication attempts | RateLimited | back off |
| `EOrder:Insufficient funds` | balance short | Rejected(funds) | |
| `EOrder:Order minimum not met` | under `ordermin` | Rejected(venue rule) | rules read again |
| `EOrder:Cost minimum not met` | under `costmin` | Rejected(venue rule) | rules read again |
| `EOrder:Tick size check failed` | off `tick_size` | Rejected(venue rule) | rules read again |
| `EOrder:Invalid price` | zero, negative or invalid | Rejected(price) | |
| `EOrder:Rate limit exceeded` | the pair's engine counter | RateLimited | |
| `EOrder:Orders limit exceeded` | too many open orders on the pair | Rejected(other) | |
| `EOrder:Domain rate limit exceeded` | master and subaccounts together | RateLimited | |
| `EOrder:Scheduled orders limit exceeded` | too many trigger orders | Rejected(other) | |
| `EOrder:Positions limit exceeded` | margin | Rejected(other) | |
| `EOrder:Reduce only` (`:Non-PC`, `:No position exists`) | margin | Rejected(reduce-only) | |
| `EOrder:Reduce only:Position is closed` | "it partially fills and the remainder is canceled" | Unknown | looked up (part of it traded) |
| `EOrder:Unknown position` | margin | Rejected(other) | |
| `EOrder:Cannot open position` | margin | Rejected(other) | |
| `EOrder:Cannot open opposing position` | margin | Rejected(other) | |
| `EOrder:Margin allowance exceeded` | margin | Rejected(funds) | |
| `EOrder:Margin level too low` | margin | Rejected(funds) | |
| `EOrder:Margin position size exceeded` | margin | Rejected(other) | |
| `EOrder:Insufficient margin` | Kraken's margin pool | Rejected(funds) | |
| `EService:Unavailable` | engine or API offline | Unknown | looked up |
| `EService:Market in cancel_only mode` | | Rejected(market closed) | |
| `EService:Market in post_only mode` | | Rejected(market closed) | send it post-only |
| `EService:Deadline elapsed` | "the order may have been placed despite the timeout" | Unknown | looked up |
| `EGeneral:Invalid arguments:Index unavailable` | index trigger unavailable | Rejected(other) | |
| `EGeneral:Invalid arguments` | malformed request | InvalidRequest | |
| `EGeneral:Internal error` | | Unknown | looked up |
| `ETrade:Invalid request` | structurally invalid | InvalidRequest | |
| `EAccount:Invalid permissions` | account permissions | Permission | |
| `EAPI:Rate limit exceeded` | the REST call counter | RateLimited | |
| `EService:Throttled` | "Try again after [timestamp]" | RateLimited, held until that moment | |
| `EQuery:Unknown asset pair` | an unrecognized pair | InvalidRequest | |
| `EOrder:Unknown order` | named only in the socket v1 cancelOrder example | Rejected(not found) | the order is read by its id |
| timeout, dropped connection, non-200, unreadable body | | Unknown | looked up, never resent |

### Rate limits for orders

The REST call counter (declared at Starter: 15, decaying 0.33/s; history calls cost 2; see Rate
limits above) counts reads, AmendOrder, CancelAll, the lookups and the socket token, but
"AddOrder, CancelOrder ... operate on a different limiter": the engine's counter per pair
(threshold 60 / 125 / 180 by tier; add +1, amend +1 plus a decay by resting time, cancel up to
+8 for an order younger than 5 s), and at most 60 / 80 / 225 open orders per pair. Over a limit:
`EOrder:Rate limit exceeded`, `EAPI:Rate limit exceeded` (no wait stated) or `EService:
Throttled: [UNIX timestamp]` (held until that moment). The declared counter paces the trade
path's reads as it paces the account read's (each stream frame's order read counts 1). No ban status;
the socket's Cloudflare limit is about 150 connections per 10 minutes per IP (the follower backs
off from 1 s to 60 s).

### Stream

`wss://ws-auth.kraken.com/v2`. Before each connection a token from `GetWebSocketsToken` (key
permission "Access WebSocket API"); the token goes in the subscribe frame:
`{"method":"subscribe","params":{"channel":"executions","token":...,"snap_orders":false,
"snap_trades":false,"order_status":true}}` (no snapshot: the book is read when the stream
connects). A `{"method":"ping"}` every 30 s (one minute of inactivity closes the socket); the
server's `heartbeat` frames are read as nothing. Each `executions` entry names an order whose
frame carries only what changed ("the fields included in the message is dependant on the
`exec_type`"): the order is read again by its `txid` (QueryOrders), once per frame (a
`pending_new` entry is skipped: "not live yet", its `new` or `canceled` follows). Each read
costs the key's REST call counter, so a burst of fills can meet `EAPI:Rate limit exceeded`; the
order is then read on the reconciliation cycle. Fills on
the socket are not read (the order's own read says how far it filled). The `sequence` number
is not checked: a gap shows as a reconnect, and the book is read again then. The socket's pair
spelling (`BTC/USD`) is not the list's: the read needs no pair. Without the WebSocket
permission the token is refused and the account is reconciled by polling.

### Disconnect behaviour

Kraken keeps working orders when the client goes away (cancel on disconnect is FIX only).
`CancelAllOrdersAfter` exists and is not armed: `cancel_on_disconnect` is not declared.

### Dry-run

`AddOrder` with `validate=true` ("the order will be validated only, it will not trade in the
matching engine"), the same fields as the order, never its client id: `dry_run = "validate"`,
run before the first order of each session at E2D. Whether it checks funds, and its answer's
shape, are not documented (an answer without `txid` is read as passed). It counts on the
engine's counter ("Fixed count ... even if the transaction fails validation", inference).

### Quirks

- Spot only: margin, xStocks and Kraken Futures (another API) are not traded here.
- The orders panel's ids are `spot:SYMBOL:txid`, the symbol as the account read names it
  (`XBT/USD`); an order placed from the chart answers under the chart's spelling (`XBTUSD`).
  Cancel and lookups need no pair, so both spellings reach the same order.
- A stop that would trigger at once is accepted, then cancelled with `reason` "Order would
  trigger immediately" ([order lifecycle](https://docs.kraken.com/exchange/guides/general/order-lifecycle)).
- Fills are read for the whole account over the window (`pair` takes spellings the doc does not
  list); `start` is exclusive and in whole seconds, so the window starts one second early and a
  repeated execution is dropped by its id.
- Open orders are listed for the whole account (OpenOrders takes no pair).
- The doc's ClosedOrders and QueryOrders examples show `price2` `0` on `stop-loss-limit` and
  `take-profit-limit` rows whose `descr.order` reads "@ limit <price>": such a row shows its
  trigger and no limit, and an amend of its limit is still sent (by the order's type).
- Live, 2026-11-12 (public conformance, no key): AssetPairs decodes to 2862 rule rows (each
  pair under its altname and its wsname), every one sound with a tick and a size step;
  `XBTUSD` tick 0.1, step 0.00000001, `ordermin` 0.00006, `costmin` 0.5. An order signed with a
  key Kraken never issued answers `EAPI:Invalid key` (Auth).

### Doc gaps

1. Duplicate `cl_ord_id` refusal code; the spelling of an echoed UUID `cl_ord_id` (read back
   from any).
2. Whether QueryOrders takes `cl_ord_id` without `txid` (the schema lists `txid` as required):
   not used.
3. What a REST cancel of a closed or unknown order answers (`EOrder:Unknown order` appears only
   in a socket v1 example; `count: 0` is read as not found).
4. `closed` with `vol_exec` under `vol`: filled or cancelled-partial (read as cancelled with what
   filled).
5. The trigger state of a stop on REST (only `misc` `stopped` / `touched`).
6. `descr.price` / `price2` on stop-limit rows (the examples show `price2` `0`).
7. Retention of closed orders for ClosedOrders and QueryOrders.
8. Whether a `pending` order is listed by OpenOrders (the lookup right after a lost answer).
9. TradesHistory: the `pair` spellings taken, window and page caps.
10. Token lifetime on a live socket ("does not expire once a successful Websockets connection
    ... is maintained" against "WebSocket tokens expire after 15 minutes").
11. FOK on types other than limit on REST.
12. The validate answer's shape, and whether it checks funds.
13. The refusal code of a pair restricted in the account's region.
14. A largest order size (none stated).
15. Whether `EService:Unavailable` means nothing was placed (read as unknown).
16. Whether a UUID `cl_ord_id` must be an RFC 4122 one (the doc says "32 hex characters"; the
    FIX guide's example is a version 1 UUID): a packed robot id has version nibble `a`, no
    RFC version.
17. Fill corrections or busts (T25); the order of partial fill events.
