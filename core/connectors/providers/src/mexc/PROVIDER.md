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
- Spot v3 reference: https://mexcdevelop.github.io/apidocs/spot_v3_en/
- Contract v1 reference: https://mexcdevelop.github.io/apidocs/contract_v1_en/
- Spot socket protobuf definitions: mexcdevelop/websocket-proto at commit
  `0c9c4f35dd0fadc3a46a350e909a93379d81e811` (`PushDataV3ApiWrapper.proto`,
  `PublicSpotKlineV3Api.proto`).
- ccxt `python/ccxt/mexc.py` (master, 2026-10-19): contract signing (`sign`), the
  open-orders call without a symbol, the contract fill sample.
- Measured answers of the public endpoints and sockets, 2026-10-19 (recorded in `tests.rs`).

## Endpoints

Spot below `https://api.mexc.com`, contracts below `https://contract.mexc.com`.

| Endpoint id | Spot | Futures | Canonical |
| --- | --- | --- | --- |
| `klines` / `kline` | `GET /api/v3/klines?symbol&interval&startTime&endTime&limit=500` | `GET /api/v1/contract/kline/{symbol}?interval&start&end` | `RequestHistory{Candles}` |
| `contract` | | `GET /api/v1/contract/detail?symbol` | contract size before a candle page |
| `exchangeInfo` / `contracts` | `GET /api/v3/exchangeInfo` | `GET /api/v1/contract/detail` | `LoadInstruments` |
| `bookTicker` / `ticker` | `GET /api/v3/ticker/bookTicker?symbol` | `GET /api/v1/contract/ticker?symbol` | `RequestSnapshot{Quote}` |
| `tickerPrice` | `GET /api/v3/ticker/price` | | `RequestBoard` |
| `time` / `ping` | `GET /api/v3/time` | `GET /api/v1/contract/ping` | `SyncTime` |
| `account` | `GET /api/v3/account` (signed) | | `Summary`, `Balances` |
| `openOrders` | `GET /api/v3/openOrders` (signed, no symbol) | `GET /api/v1/private/order/list/open_orders?page_num&page_size=100` (signed) | `OpenOrders` |
| `myTrades` / `orderDeals` | `GET /api/v3/myTrades?symbol&startTime&endTime&limit=100` (signed) | `GET /api/v1/private/order/list/order_deals?symbol&start_time&end_time&page_num&page_size=100` (signed) | `Fills` |
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
- Spot list: tick = 10^-`quotePrecision`, lot = `baseSizePrecision`, active when `status` is
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
- Futures quantities are contracts; the instrument list's contract size is the point value.

## Auth

- Spot: lowercase hex HMAC-SHA256 of the query string with `recvWindow=10000` and
  `timestamp` (ms) appended, sent as `signature`; key in `X-MEXC-APIKEY` (the SDK's
  `HmacQuerySigner`). Checked against example 2 of MEXC's docs; their example 1 prints a
  different signature for the same string, and Python's `hmac` agrees with example 2.
- Contracts: lowercase hex HMAC-SHA256 of `key + Request-Time + sorted query`, sent as
  `ApiKey`, `Request-Time` (ms) and `Signature` headers; the query travels in the order
  signed. Checked against a reference computed with Python's `hmac`.
- Clock: `SyncTime` reads the venue's clock (spot `/api/v3/time`, contracts
  `/api/v1/contract/ping`) before a signed read. Spot refuses a timestamp outside
  `recvWindow` (700003), contracts one more than 10 s off (513).
- Secrets: `api_key`, `api_secret`. A key bound to no IP address expires after 90 days
  (docs).

## Rate limits

Spot: 500 requests per 10 s per endpoint, per IP for public calls and per account for signed
ones; `/api/v3/account` 2 per second. Contracts: 20 per 2 s per endpoint;
`contract/detail` once per 5 s (docs; not enforced on 2026-10-19, declared anyway).

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

- Coin-settled and dated futures, margin, sub-accounts, stop and trigger orders.
- More than 100 spot fills of one pair within one second.

## Regions and terms

MEXC does not serve some jurisdictions (the United States among them); public market data
answered from the test machine.

## Live best bid and ask

Spot `spot@public.aggre.bookTicker.v3.api.pb@100ms@<symbol>`: protobuf, wrapper field 315
`PublicAggreBookTickerV3Api` (`bidPrice` 1, `bidQuantity` 2, `askPrice` 3, `askQuantity` 4).
Contracts `sub.ticker` / `push.ticker`: `bid1`, `ask1`, `timestamp`, about once a second,
answered `rs.sub.ticker` in order like a candle (measured 2026-10-19).
