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
| `balance` | `POST /0/private/Balance` (signed) | `QueryAccount{Summary}`, `{Balances}` | 1 | none |
| `openOrders` | `POST /0/private/OpenOrders` (signed) | `QueryAccount{OpenOrders}` | 1 | none |
| `tradesHistory` | `POST /0/private/TradesHistory` (signed), body `type=all&trades=false&start&end&ofs` | `QueryAccount{Fills}` | 2 counter points | offset `ofs`, 50 rows, 400 pages max |
| socket `ohlc` | `wss://ws.kraken.com/v2`, `{"method":"subscribe","params":{"channel":"ohlc","symbol":[..],"interval":N}}` | `Subscribe{Candles}` | | one socket per interval |

## Auth

`API-Sign` = base64 `HMAC-SHA512(base64-decoded secret, path ++ SHA256(nonce ++ body))`,
`API-Key` header, body form-encoded with the nonce first. The nonce is a process-wide
strictly increasing counter (two calls in one millisecond are otherwise refused with
`EAPI:Invalid nonce`). No recv window, no clock call. Secrets: `api_key`, `api_secret`; a
secret that is not base64 is refused before any call. Signature checked against Kraken's
documented example.

## Rate limits

- Public REST: about one request per second per IP; OTW's worker paces history at 1.1 s.
- Private REST: a counter per key that decays over time (starter tier 15, -0.33/s);
  `TradesHistory` costs 2. Declared as an endpoint bucket of one page per 1.2 s, applied by
  the engine's token bucket on the shared driver.
- Over the limit: HTTP 200 with `EAPI:Rate limit exceeded` (`RateLimited`). No ban status.

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

Errors:

| Error | Canonical |
| --- | --- |
| empty `error` array | none (HTTP status decides) |
| `EAPI:Rate limit exceeded`, any "rate limit" / "too many requests" | RateLimited |
| `EAPI:Invalid key`, `EAPI:Invalid signature`, `EAPI:Invalid nonce`, `EAPI:Bad request` | Auth |
| `EGeneral:Permission denied`, `EAPI:Feature disabled` | Permission |
| `EQuery:Unknown asset pair`, `EQuery:Unknown asset` | NotFound ("bad symbol") |
| `EService:Unavailable`, `EService:Busy`, `EService:Deadline elapsed`, `EGeneral:Internal error` | Transient |
| anything else | InvalidRequest |

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
- A fill pull longer than 400 pages is an error asking for a shorter period.
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
