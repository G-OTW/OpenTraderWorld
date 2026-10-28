# OANDA

Adapter version: 0.1.0        Provider API version: v20 REST v3        Maturity: M2

Sources:
- Candles: https://developer.oanda.com/rest-live-v20/instrument-ep/ (account-scoped variant
  under https://developer.oanda.com/rest-live-v20/account-ep/)
- Pricing: https://developer.oanda.com/rest-live-v20/pricing-ep/
- Instruments: https://developer.oanda.com/rest-live-v20/account-ep/
- Positions, orders: https://developer.oanda.com/rest-live-v20/position-ep/, https://developer.oanda.com/rest-live-v20/order-ep/
- Transactions: https://developer.oanda.com/rest-live-v20/transaction-ep/
- Best practices and limits: https://developer.oanda.com/rest-live-v20/best-practices/
- Environments: https://developer.oanda.com/rest-live-v20/development-guide/

M2: mock catalog green; parity with the legacy connector on mock (candles with bid and ask
on every timeframe, pricing, instrument search, refusals, settings; account fills over a
three-year walk, positions, holdings, orders, quotes, connection test, both environments).
Real shadows ready with a token (`OTW_OANDA_API_TOKEN`, `OTW_OANDA_ACCOUNT_ID`,
`OTW_OANDA_ENV`).

## Endpoints

Every path starts `/v3/accounts/{account}`; the signer writes the account id.

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `candles` | `GET …/instruments/{inst}/candles?price=MBA&granularity&from&to&dailyAlignment=0&alignmentTimezone=UTC&weeklyAlignment=Monday` | `RequestHistory{Candles}` | 1 | time: up to 5000 candles, the planner sizes the window |
| `pricing` | `GET …/pricing?instruments` | `RequestSnapshot{Quote}` | 1 | none |
| `instruments` | `GET …/instruments[?instruments=A,B]` | `LoadInstruments`, `ResolveInstruments` | 1 | none |
| `summary` | `GET …/summary` | `QueryAccount{Summary}` | 1 | none |
| `positions` | `GET …/openPositions` | `QueryAccount{Positions}` | 1 | none |
| `orders` | `GET …/pendingOrders` | `QueryAccount{OpenOrders}` | 1 | none |
| `transactions` | `GET …/transactions?from&to&pageSize=1000&type=ORDER_FILL` | `QueryAccount{Fills}`: the page index of one slice | 1 | cursor: 365-day slices |
| `idrange` | `GET …/transactions/idrange?from&to&type=ORDER_FILL` | `QueryAccount{Fills}`: one page | 1 | cursor |

## Auth

`Authorization: Bearer <api_token>` and `Accept-Datetime-Format: RFC3339`, added by the
signer, which also writes the `account_id` setting into the path (four dash-separated groups
of digits, checked before any call). OANDA issues no read-only token: the same token can
trade. fxTrade and fxPractice are different hosts with different tokens and account ids; the
connector's `env` (`live` or `practice`) picks the host.

## Rate limits

120 requests a second per token; the instrument list and the transaction id-range pages
30 a second. No bucket declared: OTW's worker paces at 100 ms, well under both.

## Symbols

Native id = OANDA's own name, `BASE_QUOTE` (`EUR_USD`, `SPX500_USD`, `XAU_USD`), venue
`OANDA`. The quote currency is read off the name (the venue's notation). Discovery is the
account's own instrument list (which instruments exist depends on the account's division);
it is cached per account.

## Enum tables

| Interval | `granularity` |
| --- | --- |
| 1m, 5m, 15m | `M1`, `M5`, `M15` |
| 1h, 4h | `H1`, `H4` |
| 1d, 1w | `D`, `W` (UTC midnight, Monday weeks) |

| Instrument `type` | Market | Journal class |
| --- | --- | --- |
| `CURRENCY` | Forex | forex |
| `CFD` (and any type OANDA adds) | Cfd | other |
| `METAL` | Commodity | other |

| Order `type` | `price` is |
| --- | --- |
| `LIMIT` | the limit |
| `STOP`, `MARKET_IF_TOUCHED`, any `*STOP*` | the trigger |

| Status | Canonical |
| --- | --- |
| 401 | Auth |
| 404, or 400 naming the instrument | NotFound ("bad symbol") |
| other 400 | InvalidRequest |
| 429 | RateLimited |
| 5xx | Transient |

## Units and time

- Prices and units as decimal strings; a negative unit count is a sell (or a short leg).
- Times RFC 3339 with nanoseconds (asked for by header); account rows are kept to the
  millisecond.
- Candle `volume` is the number of prices made in the period (a dealing desk's only volume).
- `complete` flags the closed candle (`history.candles.final_flag = true`).
- Bid and ask candles go to `Candle.sides` (`history.candles.sides = true`): OTW stores them
  next to the mid for fills priced on the side that deals.
- The latest price is the closeout pair (`closeoutBid`, `closeoutAsk`): what OANDA would
  actually fill at. A holding is valued at its mid.
- A fill's price is `fullVWAP` (the deprecated `price` when absent); its fee is
  `|commission| + |guaranteedExecutionFee|`, billed in the account's currency.

## Quirks

- The transaction index answers page URLs, not rows; only the id pair is taken from each
  (digits only), and the call is rebuilt against the configured host.
- The index accepts at most 365 days: the fill walk goes a year at a time, at most 500 pages.
- A take-profit or stop-loss order hangs off a trade and names no instrument: left out.
- Positions come one row per side (a hedging account may hold both).
- No live candles: OANDA streams prices over a long-lived HTTP response, not a socket.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)`.
- A fill's fee is added exactly (legacy added two floats, one bit off at times).
- A holding carries the instrument's display name.
- The account's symbol suggestions are what it is positioned in or working (legacy listed
  every instrument it may trade).
- The data connection test reports the instrument list in the framework's words.
- An unknown instrument reads as a symbol fault.

## Regions and terms

OANDA operates through regional divisions (US, Canada, Europe, UK, Asia Pacific, Japan,
Australia, Singapore); which instruments an account may trade depends on its division, and
CFDs are not offered to US accounts. The token is a full-access credential.
