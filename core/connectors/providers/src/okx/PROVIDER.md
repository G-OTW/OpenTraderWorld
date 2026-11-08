# OKX (v5)

Adapter version: 0.1.0. Provider API version: v5. Maturity: M2 (mock catalog green; shadow
runs against the live venue, history, instrument list and socket, identical to the legacy
connector; account parity on mock, real probes refused by the venue as expected).

Sources:
- REST and socket reference: https://www.okx.com/docs-v5/en/
- Market data: https://www.okx.com/docs-v5/en/#order-book-trading-market-data
- Error codes: https://www.okx.com/docs-v5/en/#error-code

## Endpoints

| Endpoint id | Method + path | Canonical | Notes |
| --- | --- | --- | --- |
| `historyCandles` | `GET /api/v5/market/history-candles?instId&bar&after&limit=300` | `RequestHistory{Candles}` | reads backwards: `after` (exclusive) starts at the window end, next page at the oldest bar of a full page |
| `instrumentsSpot/Swap/Futures` | `GET /api/v5/public/instruments?instType=SPOT|SWAP|FUTURES` | `LoadInstruments` | three requests, listed in that order |
| `ticker` | `GET /api/v5/market/ticker?instId` | `RequestSnapshot{Quote}` | a one-sided book emits nothing |
| `tickers` | `GET /api/v5/market/tickers?instType=SPOT` | `RequestBoard` | `last` |
| `balance` | `GET /api/v5/account/balance` (signed) | `QueryAccount{Summary}` / `{Balances}` | unified account: one line per currency (`eq`), no cost basis |
| `positions` | `GET /api/v5/account/positions` (signed) | `QueryAccount{Positions}` | `posSide` long/short in hedge mode, the sign of `pos` in one-way mode |
| `ordersPending` | `GET /api/v5/trade/orders-pending?limit=100` (signed) | `QueryAccount{OpenOrders}` | trigger price from `slTriggerPx` |
| `fillsHistory` | `GET /api/v5/trade/fills-history?instType&begin&end&limit=100[&after]` (signed) | `QueryAccount{Fills}` | walks SPOT, MARGIN, SWAP, FUTURES, OPTION; pages by the last row's `billId`; 500 pages max; cursor `<book>:<after>:<pages>` |
| socket | `wss://ws.okx.com:8443/ws/v5/business`, `{"op":"subscribe","args":[{"channel":"candle1m","instId":..}]}` | `Subscribe{Candles}` | candles live on the business socket, not the public one |

## Field mapping

Candle row (all strings): `[ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm]`. `vol` is
the volume as OTW stores it: coins on spot, contracts on a derivative (a deliberate deviation
from "quantity in base units", to keep stored series unchanged). `confirm` `"0"` = forming.

Instrument row: display `base/quote` with `baseCcy` (else `ctValCcy`) and `quoteCcy` (else
`settleCcy`); `multiplier = ctVal x ctMult` (1 on spot); `settleCcy` as settlement currency;
`ctType = inverse` flags inverse contracts; `state = live` is active; `expTime` expiry.

Fill: `tradeId`, `ordId`, `fillSz`, `fillPx`, `fee` (OKX signs a charge negative: the sign
flips so a cost is positive and a rebate stays negative), `feeCcy`, `ts` (ms).

## Timeframes

`1m 5m 15m 1H 4H 1Dutc 1Wutc`. The daily and weekly bars are the UTC-anchored variants:
OKX's plain `1D` opens at 16:00 UTC.

## Auth

`OK-ACCESS-SIGN` = base64 HMAC-SHA256 of `timestamp + METHOD + path?query + body`, with
`OK-ACCESS-TIMESTAMP` in ISO-8601 to the millisecond, `OK-ACCESS-KEY` and
`OK-ACCESS-PASSPHRASE` (the passphrase chosen with the key). Signature checked against a
reference computed with an independent HMAC implementation. Secrets: `api_key`,
`api_secret`, `api_passphrase`. The host clock is used (no clock endpoint call), as before.
Demo trading (`env = demo`): same host, extra header `x-simulated-trading: 1`.

## Socket

Data frames name `arg.channel` and `arg.instId`: that pair routes them. A subscription
replays recent candles, so a row older than the newest seen on that channel is dropped. A
refusal (`event: error`) does not name its `arg`: the channel and instrument are read out of
the message text; a refusal naming neither is the socket's (reconnect). The venue drops a
socket silent for 30 s: the bare text `ping` goes out after 20 quiet seconds, `pong` is
ignored.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `0` | success | none |
| 50011, 50061 | rate limit | RateLimited |
| 50101 to 50105, 50111, 50113 | key, signature, timestamp, passphrase | Auth |
| 50114, 50120 | the key may not read this | Permission |
| 51001 | instrument does not exist | NotFound ("bad symbol") |
| 50001, 50004, 50013, 50026 | system busy, timeout | Transient |
| anything else | | InvalidRequest |

Most refusals come with HTTP 200: the envelope code decides, the status only when there is
no envelope.

## Account rules (shared bridge)

- Contract size and settlement currency come from the instrument list; a fill on an
  instrument it does not know falls back to the quote written in the id (`BTC-USDT-SWAP`).
- An order's currency is the quote written in its id, as before.
- Holdings are valued on `<asset>-<quote>` for quote in USDT, USDC, USD, EUR, BTC, reversed
  too (`USDC-USDT` prices USDT at 1/price).

## Deliberate differences from the legacy connector

- A refused instrument on the history endpoint reads as a symbol problem in the chart's
  notice.
- An open order's time keeps its milliseconds (unchanged for OKX).
- Option contract sizes stay unknown (multiplier 1), as before: the instrument list for
  options needs an underlying.

## Regions and terms

OKX restricts some jurisdictions (including the US mainland); public market data answered
from the test machine. Demo trading needs its own key.

## Live best bid and ask

Channel `bbo-tbt` on the **public** socket (`/ws/v5/public`), its own socket class: the
business socket refuses it (60018, measured 2026-10-19). Rows `{asks, bids, ts}`, each side
`[price, size, 0, orders]`, on every change.
