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
  Price-triggered orders (`/spot/price_orders`, `/futures/usdt/price_orders`) are not read.
- Summary: spot reads `/account/detail` (user id, key mode, IP list size); futures the USDT
  futures account total and unrealised PnL. Gate.io publishes no call that names a key's
  permissions, so `can_trade` is unknown.

## Auth

`SIGN` = lowercase hex HMAC-SHA512 of
`METHOD + "\n" + /api/v4/path + "\n" + query + "\n" + hex(SHA512(body)) + "\n" + timestamp`,
the query written unencoded (the SDK signs `unquote_plus(urlencode(query))`), the timestamp in
seconds; sent with `KEY` and `Timestamp`. Checked against a reference computed with Python's
`hmac` and `hashlib`. Secrets: `api_key`, `api_secret`. Host clock: the venue refuses a
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
- Price-triggered (stop) orders.

## Regions and terms

Gate.io does not serve some jurisdictions (the United States among them); public market
data answered from the test machine.

## Live best bid and ask

`spot.book_ticker` / `futures.book_ticker` on the candle socket, payload `[symbol]`: `{t, s, b,
B, a, A}` on every change; futures sizes are numbers in contracts, multiplied by the contract
size (measured 2026-10-19). Spot acks a pair it does not list and sends nothing.
