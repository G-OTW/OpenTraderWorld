# Bitget (v2, spot and USDT-M futures)

Adapter version: 0.1.0. Provider API version: v2. Maturity: M2 (mock catalog green; shadow
runs against the live venue, history, instrument lists and socket, identical to the legacy
connector; account parity on mock, real probes refused by the venue as expected).

One adapter per book (`Bitget::spot()`, `Bitget::usdt_futures()`): `BTCUSDT` is a spot pair
and a perpetual at once and they are different series. A data connector picks its book with
its `market` setting (`spot`, `usdt-futures`); a broker account reads the books its `books`
setting names (`spot`, `usdt-futures`, both by default).

Sources:
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
| `unfilledOrders` / `ordersPending` | `GET /api/v2/spot/trade/unfilled-orders?limit=100` | `GET /api/v2/mix/order/orders-pending?productType&limit=100` (`entrustedList`) | `OpenOrders` |
| `fills` / `fillHistory` | `GET /api/v2/spot/trade/fills?startTime&endTime&limit=100[&idLessThan]`, 90-day slices | `GET /api/v2/mix/order/fill-history?...`, 7-day slices, `fillList` + `endId` | `Fills`, slices walked backwards from the period's end, 500 pages per book |
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
HMAC implementation. Secrets: `api_key`, `api_secret`, `api_passphrase`. Host clock.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `00000` | success | none |
| 429 | rate limit | RateLimited |
| 40006, 40008, 40009, 40012, 40018, 40037 | key, timestamp, signature, passphrase, IP | Auth |
| 40014 | permission | Permission |
| 40034 | parameter (symbol) does not exist | NotFound ("bad symbol") |
| 40010, 40015, 45001 | busy, timeout | Transient |
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
