# Bybit (v5: spot, linear, inverse, options; Bybit EU spot)

Adapter version: 0.2.0. Provider API version: v5. Maturity: M2 (mock catalog green; history,
instrument lists, quotes and the socket of every book checked against the live public
endpoints; account reads on mock and on a lent key when one is set, see Verification).

One adapter per book:

| Constructor | Venue | Provider id | Book |
| --- | --- | --- | --- |
| `Bybit::spot()` | `BYBIT` | `bybit` | bybit.com spot |
| `Bybit::linear()` | `BYBIT_LINEAR` | `bybit` | USDT perpetuals, USDC perpetuals (`BTCPERP`), USDT dated futures (`BTCUSDT-26DEC26`) |
| `Bybit::inverse()` | `BYBIT_INVERSE` | `bybit` | coin-settled contracts, one USD each (`BTCUSD`, `BTCUSDZ26`) |
| `Bybit::option()` | `BYBIT_OPTION` | `bybit` | options (`BTC-26DEC26-100000-C-USDT`); no candles exist |
| `Bybit::eu_spot()` | `BYBIT_EU` | `bybit_eu` | Bybit EU (bybit.eu) spot |

`BTCUSDT` is a spot pair and a perpetual at once and they are different series. A data
connector picks its book with its `market` setting (`spot`, `linear`, `inverse`); a broker
account reads the books its `books` setting names (`spot`, `linear`, `inverse`, `option`, all
by default). Bybit EU is its own provider and broker: a separate platform with its own order
book (thinner, its own pairs: `BTCEUR`, `BTCUSDC`, no `BTCUSDT`), its own hosts and its own
keys, checked on the live venue (2026-10-18).

Sources:
- Kline: https://bybit-exchange.github.io/docs/v5/market/kline
- Instruments: https://bybit-exchange.github.io/docs/v5/market/instrument
- Tickers: https://bybit-exchange.github.io/docs/v5/market/tickers
- Socket: https://bybit-exchange.github.io/docs/v5/websocket/public/kline
- Authentication, hosts: https://bybit-exchange.github.io/docs/v5/guide
- API key info: https://bybit-exchange.github.io/docs/v5/user/apikey-info
- Wallet: https://bybit-exchange.github.io/docs/v5/account/wallet-balance
- Positions: https://bybit-exchange.github.io/docs/v5/position
- Open orders: https://bybit-exchange.github.io/docs/v5/order/open-order
- Executions: https://bybit-exchange.github.io/docs/v5/order/execution
- Account modes (classic retired): https://bybit-exchange.github.io/docs/v5/acct-mode
- Error codes: https://bybit-exchange.github.io/docs/v5/error
- Rate limits: https://bybit-exchange.github.io/docs/v5/rate-limit
- Official SDK used as signing reference: https://github.com/bybit-exchange/pybit (`_http_manager.generate_signature`)

## Endpoints

| Endpoint id | Method + path | Canonical |
| --- | --- | --- |
| `kline` | `GET /v5/market/kline?category&symbol&interval&start&end&limit=1000` | `RequestHistory{Candles}`, newest first, backwards from `end` (not options) |
| `instruments` | `GET /v5/market/instruments-info?category[&baseCoin=All][&limit=1000&cursor]` | `LoadInstruments`; spot whole, other books paged on `nextPageCursor` (follow-up) |
| `tickers` | `GET /v5/market/tickers?category&symbol` | `RequestSnapshot{Quote}` (`bid1Price`, `ask1Price`) |
| `board` | `GET /v5/market/tickers?category=spot` | `RequestBoard` (`lastPrice`) |
| `queryApi` | `GET /v5/user/query-api` (signed) | `Summary`, step 1: read-only or not, unified or classic, permissions |
| `walletBalance` | `GET /v5/account/wallet-balance?accountType=UNIFIED` (signed) | `Summary` step 2 (`totalEquity`), `Balances` (spot) |
| `positionList` | `GET /v5/position/list?category[&settleCoin]&limit=200[&cursor]` (signed) | `Positions`; linear walked USDT then USDC; inverse and options answer all |
| `orderRealtime` | `GET /v5/order/realtime?category[&settleCoin]&limit=50[&cursor]` (signed) | `OpenOrders`, linear walked USDT then USDC |
| `executionList` | `GET /v5/execution/list?category&startTime&endTime&limit=100[&baseCoin][&cursor]` (signed) | `Fills`, 7-day slices backwards from the period's end, each paged on `nextPageCursor`; options per base coin; 2000 pages per book |
| socket | `wss://stream.bybit.com/v5/public/{spot,linear,inverse}`, `wss://stream.bybit.eu/v5/public/spot`, `{"op":"subscribe","req_id":"kline.1.BTCUSDT","args":["kline.1.BTCUSDT"]}` | `Subscribe{Candles}` |

Cursors OTW hands back: fills `f|coins|coin_index|slice_end|pages|venue_cursor`, the option
coin listing `c|coins|venue_cursor`, settle-coin walks `coin_index|venue_cursor`. The venue's
own cursor (already percent-encoded, accepted either way) is carried verbatim.

## Hosts (`site` setting)

A key works only on the site that issued it. Public market data is read on the book's own
host (bybit.com or bybit.eu) whatever the site. Every regional bybit.com host was checked to
serve the same book as the global one (identical candles, 2026-10-18); bybit.eu does not.

| `site` | Host | Book |
| --- | --- | --- |
| `global` (default) | `api.bybit.com` | bybit.com |
| `bytick` | `api.bytick.com` | bybit.com |
| `tr` / `kz` / `ge` / `ae` / `id` | `api.bybit.tr` / `api.bybit.kz` / `api.bybitgeorgia.ge` / `api.bybit.ae` / `api.bybit.id` | bybit.com |
| `jp` / `hk` | `api.manepa.jp` / `api.spark-fintech.com` | bybit.com |
| `testnet` | `api-testnet.bybit.com` | testnet (own prices) |
| `eu` (Bybit EU default) / `eu-testnet` | `api.bybit.eu` / `api-testnet.bybit.eu` | Bybit EU |

A bybit.com account naming an EU site, or the other way round, is refused when saved, with
the fix. A refused key names the host it was sent to.

## Field mapping and rules

- Interval: `1 3 5 15 30 60 120 240 360 720 D W M`, the same on every book and the socket.
  The daily bar opens at midnight UTC, the weekly on Monday, the monthly on the 1st (checked
  on the live venue), so no variant is needed.
- History rows `[start, open, high, low, close, volume, turnover]`, strings, newest first.
  No closed flag: a bar is final 10 s after its close. Measured on 2026-10-18 by polling the
  archive through minute boundaries: on bybit.com a closed minute was last rewritten up to
  2.2 s after its close (20 bars over spot, linear and inverse), on Bybit EU never after it.
  A month closes on the calendar.
- Volume: in the base coin on spot and linear (linear contracts are sized in the coin),
  turnover in the quote. **Inverse is the other way round** (recorded: `volume` 3000332 USD
  contracts, `turnover` 37.11 BTC), so the two are swapped to keep volume in the coin.
- Socket rows carry `confirm`, mapped to `is_final`, and `timestamp`, the envelope's venue
  time. One subscribe frame per topic with the topic as `req_id`: the answer names its pane,
  a refusal fails that pane only (an inverse socket may send data before its ack). A refusal
  without `req_id` is read from the topic in brackets in `ret_msg`. `{"op":"ping"}` after 20
  quiet seconds; the pong is ignored.
- Instruments: `status` `Trading` is active (options: `Trading` and `Delivering`).
  `contractType` `LinearPerpetual`, `LinearFutures`, `InversePerpetual`, `InverseFutures`;
  `optionsType` `Call`, `Put` with `deliveryTime` as expiry. Any other value is logged and
  left out. An inverse contract carries `inverse = true` and its settle coin.
- Fees: Bybit signs a charge positive and a maker rebate negative, which is the canonical
  sign: kept as is, never `abs`. Fee asset from `feeCurrency`.
- Executions: `Trade`, `AdlTrade`, `BustTrade`, `Delivery`, `BlockTrade`, `MovePosition`,
  `FutureSpread` are fills; `Funding` and `Settle` are cash movements and left out; anything
  else is logged and left out. Option fills carry the option class.
- Option fills: the venue answers BTC alone without `baseCoin`, so the coins are read first
  from the option list (public, `baseCoin=All`, paged), then each is walked in turn.
- Inverse: positions and orders are converted from USD contracts to coins (size / entry
  price, order quantity / its price or trigger), quoted in USD; the unrealised profit is in
  the coin and is not carried. **Inverse fills are refused by name**: their profit is
  quantity × (1/entry − 1/exit) in the coin, which the journal does not compute.
- Positions: `side` `Buy` long, `Sell` short (negative quantity); a zero size is skipped; an
  open position with another side is a decode error. Linear currency = the settle coin.
- Balances: `walletBalance` is the total, `walletBalance - locked` the available amount.
- Summary: a classic account (`uta = 0`) is refused with the fix (Bybit retired classic
  accounts); a key that can trade is said so (`readOnly = 0`).

## Enum tables

| Native | Canonical |
| --- | --- |
| side `Buy` / `Sell` | `Side::Buy` / `Side::Sell`, else `Unknown(raw)` |
| contractType `LinearPerpetual` / `InversePerpetual` | `Perp` |
| contractType `LinearFutures` / `InverseFutures` | `Future` |
| optionsType `Call` / `Put` | `OptionRight::Call` / `Put` |
| other contractType / optionsType | left out (warned) |
| execType `Trade` `AdlTrade` `BustTrade` `Delivery` `BlockTrade` `MovePosition` `FutureSpread` | fill |
| execType `Funding` `Settle` | not a fill |

## Auth

`X-BAPI-SIGN` over `timestamp(ms) + api_key + recv_window + query string` (a POST signs its
JSON body instead), plus `X-BAPI-API-KEY`, `X-BAPI-TIMESTAMP`, `X-BAPI-RECV-WINDOW: 5000`.

- System-generated key: HMAC-SHA256, lowercase hex.
- Self-generated key: RSA-SHA256 PKCS#1 v1.5, base64; the private key is pasted as the secret
  in PEM, PKCS#8 (`BEGIN PRIVATE KEY`) or PKCS#1 (`BEGIN RSA PRIVATE KEY`), 2048 to 4096
  bits.

Both checked against Bybit's official SDK (pybit `generate_signature`) on the plain text of
Bybit's documented example: HMAC with a test secret, RSA with a throwaway key generated for
the test (`fixtures/`, allowlisted for gitleaks by path). Bybit publishes the plain text but
not the secret behind its example signatures, so no published vector exists. Secrets:
`api_key`, `api_secret`. Host clock.

## Verification

- Public half, live: candles, day/week anchors, forming bar, stability, symbol search, unknown
  symbol, quotes and the socket on spot, linear, inverse and Bybit EU.
- A made-up key on bybit.com and bybit.eu: HTTP 401 with an empty body, before the
  signature is looked at. It proves the request reaches the venue, not the signature.
- `brokers::net_tests::a_lent_bybit_key_reads_the_whole_account` runs every account read
  against a real key set in the environment (`OTW_BYBIT_KEY`, `OTW_BYBIT_SECRET`, optional
  `OTW_BYBIT_SITE`, `OTW_BYBIT_BROKER`, `OTW_BYBIT_BOOKS`). Until it has run green, account
  reads are verified on mock only.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| `0` | success | none |
| HTTP 403, CloudFront page ("block access from your country") | country refused | Permission |
| HTTP 403 otherwise | IP over 600 requests / 5 s, banned 10 minutes (documented) | RateLimited, retry after 600 s |
| 10006, 10018, 10009 | too many visits, IP limit, IP banned | RateLimited |
| 10002, 10003, 10004, 10007, 10010, 33004 | timestamp, key, signature, auth, IP, key expired | Auth |
| 10005, 10024 | permission, compliance (region) | Permission |
| 10001 naming a symbol, 170121 | the venue does not list it (recorded: `Not supported symbols`) | NotFound ("bad symbol") |
| 10000, 10016 | server timeout, server error | Transient |
| anything else | | InvalidRequest |

## Rate limits

Public market data: 600 requests per 5 seconds per IP (declared as one bucket). Private
endpoints: 10 to 50 requests per second per account and endpoint (`retCode` 10006). Past the
IP limit Bybit answers HTTP 403 for 10 minutes.

## Quirks

- The archive answers HTTP 200 with `retCode` 10001 for an unknown symbol.
- Bybit EU answers candles for some pairs it does not list (`BTCUSDT`); its instrument list
  is the source of truth, and the symbol picker offers only what it lists.
- The fill history covers two years and at most seven days per call.
- Options on a coin that no longer has any listed option are not found by the coin walk.

## Regions and terms

Bybit refuses some jurisdictions (the United States among them) and answers them HTTP 403
from its CDN. Residents of the EEA are served by Bybit EU. Public market data of both
platforms answered from the test machine.

## Live best bid and ask

Topic `orderbook.1.<symbol>` on the candle socket of each market (spot, linear, inverse, EU): a
level-1 snapshot on every change, `b`/`a` as `[price, size]`, stamped `ts` (measured
2026-10-19). A delta naming one side keeps the other; a zero size empties it.
