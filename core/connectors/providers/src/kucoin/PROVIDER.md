# KuCoin (spot and futures)

Adapter version: 0.1.0. Provider API version: v1 (v2 for the spot symbol list). Maturity: M2
(mock catalog green; history, instrument lists, quotes and both sockets checked against the
live venue on 2026-10-19; account parity on mock only, no key lent).

Two adapters, two OTW providers: `Kucoin::spot()` (`kucoin`, venue `KUCOIN`) and
`Kucoin::futures()` (`kucoin_futures`, venue `KUCOIN_FUTURES`). Spot and futures are separate
platforms on separate hosts with their own symbols (`BTC-USDT`, `XBTUSDTM`), so nothing is
inferred from a ticker. Futures are the USDT- and USDC-margined contracts; coin-margined
(inverse, `XBTUSDM`) contracts are left out of the list and refused by name, since their
volume is counted in USD and their profit in the coin.

Sources:
- KuCoin universal SDK specification, `spec/original/meta.json`:
  https://github.com/Kucoin/kucoin-universal-sdk (endpoint parameters, limits, examples)
- Signing: https://www.kucoin.com/docs-new/authentication and the SDK's
  `sdk/python/kucoin_universal_sdk/internal/infra/default_signer.py` / `default_transport.py`
- Spot candles: https://www.kucoin.com/docs-new/rest/spot-trading/market-data/get-klines
- Futures candles: https://www.kucoin.com/docs-new/rest/futures-trading/market-data/get-klines
- Measured answers of the public endpoints, 2026-10-19 (recorded in `tests.rs`).

## Endpoints

| Endpoint id | Spot (`api.kucoin.com`) | Futures (`api-futures.kucoin.com`) | Canonical |
| --- | --- | --- | --- |
| `candles` / `kline` | `GET /api/v1/market/candles?type&symbol&startAt&endAt` (s) | `GET /api/v1/kline/query?symbol&granularity&from&to` (ms) | `RequestHistory{Candles}` |
| `contract` | | `GET /api/v1/contracts/{symbol}` | contract size before a candle or quote call |
| `symbols` / `contracts` | `GET /api/v2/symbols` | `GET /api/v1/contracts/active` | `LoadInstruments` |
| `level1` / `ticker` | `GET /api/v1/market/orderbook/level1?symbol` | `GET /api/v1/ticker?symbol` | `RequestSnapshot{Quote}` |
| `allTickers` | `GET /api/v1/market/allTickers` | | `RequestBoard` (`last`) |
| `bullet` | `POST /api/v1/bullet-public` | `POST /api/v1/bullet-public` | socket address and token (`socket_bootstrap`) |
| `apiKey` | `GET /api/v1/user/api-key` (signed) | | `Summary`: permissions, can trade |
| `accounts` | `GET /api/v1/accounts` (signed) | | `Balances` (`main` + `trade` summed per coin) |
| `activeSymbols` / `activeOrders` | `GET /api/v1/hf/orders/active/symbols`, then `GET /api/v1/hf/orders/active?symbol` per symbol | | `OpenOrders` |
| `hfFills` | `GET /api/v1/hf/fills?symbol&startAt&endAt&limit=100[&lastId]` (signed) | | `Fills`, per named pair |
| `accountOverview` | | `GET /api/v1/account-overview?currency=USDT` (signed) | `Summary` |
| `positions` | | `GET /api/v1/positions?currency=USDT`, then `USDC` | `Positions` |
| `orders` | | `GET /api/v1/orders?status=active&currentPage&pageSize=1000` | `OpenOrders` |
| `fills` | | `GET /api/v1/fills?startAt&endAt&currentPage&pageSize=1000` | `Fills` |

Socket: the bullet answer names `instanceServers[0].endpoint` and a `token`; the socket opens
at `endpoint?token=...`, says `welcome`, and takes
`{"id","type":"subscribe","topic","privateChannel":false,"response":true}`. Topics: spot
`/market/candles:<symbol>_<interval>`, futures `/contractMarket/limitCandle:<symbol>_<interval>`.

## Field mapping and rules

- Interval tokens: `1min 3min 5min 15min 30min 1hour 2hour 4hour 6hour 8hour 12hour 1day 1week`
  (spot history and both sockets); futures history takes minutes
  (`1 5 15 30 60 120 240 480 720 1440 10080`).
- Candle columns (measured): spot `[time s, open, close, high, low, volume, turnover]` on the
  archive and the socket; futures archive `[time ms, open, high, low, close, volume,
  turnover]` as JSON numbers; futures socket `[time s, open, close, high, low, turnover,
  volume]`. Futures volume is in contracts and is multiplied by the contract size
  (`multiplier`, e.g. 0.001 XBT) into the coin.
- Paging (measured): spot answers the newest 1500 bars of `[startAt, endAt)`, walked
  backwards with `endAt` = the oldest bar's open. Futures answers the oldest 200 of
  `[from, to]`, walked forwards from the newest bar plus one period.
- Daily bars open at midnight UTC on both. Spot weeks open on **Thursday** (measured
  `1792022400` = Thu 2026-10-15), so spot does not declare `1w`: the engine folds days into
  Monday weeks. Futures weeks open on Monday (measured) and are native.
- No archive row carries a closed flag. Measured 2026-10-19 over four minute boundaries: no
  spot bar changed after its close; futures bars were rewritten up to 12.3 s after. A bar is
  final once its period plus 10 s (spot) or 60 s (futures) has passed.
- The socket carries no closed flag either: every candle goes out forming and the engine
  closes it when the next period starts (`candles_final_flag = false`). Spot event time is in
  nanoseconds, futures in milliseconds.
- The socket acknowledges a symbol it does not list and then stays silent (measured), so a
  live session loads the instrument list and refuses an unlisted pane by name. A ping
  (`{"id","type":"ping"}`) goes out 5 s before the bullet's `pingInterval` (18 s).
- Futures quotes: `bestBidSize` / `bestAskSize` are contracts, multiplied into coins.
- Account rows: spot quantities are in the coin; futures quantities are contracts, and the
  instrument list's `multiplier` is the point value. Fees as written, positive for a cost.
  Futures rows settled outside USDT/USDC (inverse) are not read. Spot balances sum the
  funding (`main`) and trading (`trade`) accounts; a margin account is not read.
- Spot fills: the endpoint requires `symbol`, so pairs are named (`fills_need_symbols`) and
  checked against the list before any signed call. Both fill endpoints answer at most 7 × 24
  hours per call: slices `[to − 7 d + 1 ms, to]` walked backwards without overlap. Spot pages
  by `lastId` (100 per page), futures by `currentPage` (1000 per page). 500 pages per pull.
- How far back KuCoin keeps fills is not documented for these endpoints: no limit is
  claimed.

## Auth

`KC-API-SIGN` = base64 HMAC-SHA256 of `timestamp(ms) + METHOD + path?query + body`, the query
written unencoded; `KC-API-PASSPHRASE` = base64 HMAC-SHA256 of the passphrase under the
secret; `KC-API-KEY`, `KC-API-TIMESTAMP`, `KC-API-KEY-VERSION: 3` (what KuCoin's SDKs send).
Signature and passphrase checked against references computed with Python's `hmac`. Secrets:
`api_key`, `api_secret`, `api_passphrase`. Host clock. The key test reads the key's
permissions: `General` reads, `Spot`, `Margin` or `Futures` could trade, which the test says.

## Rate limits

Public pool: 2000 weight per 30 s per IP (`gw-ratelimit-limit: 2000` measured); a candle
page weighs 3. Spot private pool 4000 per 30 s (fill page weight 2), futures pool 2000 per
30 s (fill page weight 5, account overview 5).

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `200000` | success | none |
| `429000`, HTTP 429 | rate limit | RateLimited |
| `400001` to `400006`, `411100` | header missing, timestamp, key, passphrase, signature, IP, frozen | Auth |
| `400007` | permission | Permission |
| `900001`, `404000`, `200003` | pair or contract does not exist, invalid symbol | NotFound (naming the symbol) |
| `400100` on a market-data call | parameter error (`Unsupported trading pair.` for a bad pair, measured) | NotFound naming the symbol |
| `500000`, `503000` | internal, busy | Transient |
| anything else | | InvalidRequest |

The spot best bid/ask answers `data: null` for a pair it does not list (measured): NotFound.

## Not covered

- Unified Trading Account (`/api/ua/...`) endpoints: the classic account endpoints above.
- Coin-margined futures, margin accounts, options.

## Regions and terms

KuCoin is not licensed in some jurisdictions (the United States among them); public market
data answered from the test machine.

## Live best bid and ask

Spot `/market/ticker:<symbol>` (`bestBid`, `bestAsk`, sizes, `time` ms), futures
`/contractMarket/tickerV2:<symbol>` (`bestBidPrice`, `bestAskPrice`, sizes in contracts, `ts`
ns), on the candle socket (measured 2026-10-19).
