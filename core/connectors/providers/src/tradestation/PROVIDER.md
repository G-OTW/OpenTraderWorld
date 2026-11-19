# TradeStation

Adapter version: 0.1.0        Provider API version: v3        Maturity: M2

Sources:
- Authentication: https://api.tradestation.com/docs/fundamentals/authentication/refresh-tokens
- Bar charts: https://api.tradestation.com/docs/specification#tag/MarketData/operation/GetBars
- Quotes, symbol details: https://api.tradestation.com/docs/specification#tag/MarketData
- Brokerage: https://api.tradestation.com/docs/specification#tag/Brokerage
- Rate limits: https://api.tradestation.com/docs/fundamentals/rate-limiting
- Environments: https://api.tradestation.com/docs/fundamentals/sim-vs-live

M2: mock catalog green (OAuth mint, reuse and refusal through the driver included); parity
with the legacy connector on mock (bars on every served timeframe for stocks, futures and
options, search, connection test, settings; account fills over paged historical orders,
positions, holdings, orders, quotes, both environments). Real shadow ready
(`OTW_TRADESTATION_CLIENT_ID`, `_CLIENT_SECRET`, `_REFRESH_TOKEN`, `_ACCOUNT_ID`, `_ENV`).

## Endpoints

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| token | `POST /oauth/token` (sign-in host), `grant_type=refresh_token` | minting the access token | 1 | none |
| `bars` | `GET /v3/marketdata/barcharts/{symbol}?unit&interval&firstdate&lastdate` | `RequestHistory{Candles}` | 1 | time: up to 57,600 bars |
| `quotes` | `GET /v3/marketdata/quotes/{symbol}` | `RequestSnapshot{Quote}` (bid/ask), `RequestSnapshot{Trades}` (last) | 1 | none |
| `symbols` | `GET /v3/marketdata/symbols/{a,b,…}` | `SearchInstruments` (the symbol typed), `ResolveInstruments` (50 a call) | 1 | none |
| `balances` | `GET /v3/brokerage/accounts/{account}/balances` | `QueryAccount{Summary}` | 1 | none |
| `positions` | `GET /v3/brokerage/accounts/{account}/positions` | `QueryAccount{Positions}` | 1 | none |
| `orders` | `GET /v3/brokerage/accounts/{account}/orders` | `QueryAccount{OpenOrders}` | 1 | none |
| `fills` | `GET /v3/brokerage/accounts/{account}/historicalorders?since&pageSize=600[&nextToken]` | `QueryAccount{Fills}` | 1 | cursor (`NextToken`), 200 pages at most |

## Auth

OAuth 2 refresh-token grant (SDK `OAuthRefresh`): `client_id`, `client_secret` and the
`refresh_token` from a one-time sign-in are posted to `signin.tradestation.com/oauth/token`;
the access token (twenty minutes, or the answer's `expires_in`) is kept for the process,
keyed by a hash of the credential, spent a minute early, shared by every call on the same
key, and dropped on a 401. `Authorization: Bearer`. The signer also writes the
`account_id` setting into brokerage paths. Scopes to grant: `ReadAccount`, `MarketData`,
`openid`, `offline_access`; never `Trade`. Live and simulated trading are different API hosts
(`env`: `live` or `sim`); the sign-in host is shared.

## Rate limits

Bar charts: 500 requests per 5 minutes plus a history credit allowance; accounts 250 per 5
minutes. No bucket declared: OTW's worker paces at 600 ms.

## Symbols

Native id = TradeStation's symbol, uppercased: `AAPL`, `@ES` (continuous), `ESH26`,
`$SPX.X`, options `AAPL 251219C150` (root, space, date, right, strike). An OCC option is
rewritten that way; TradeStation's own spelling is accepted as typed
(`own_option_symbols`). Paths are percent-encoded. No free-text search: the details of the
symbol typed confirm it. Venue `TRADESTATION`; the exchange comes from the details.

## Enum tables

| Interval | `unit`, `interval` |
| --- | --- |
| 1m, 5m, 15m | `Minute`, 1/5/15 |
| 1d | `Daily`, 1 |
| 1h (folded from 15m by the engine) | not asked: TradeStation's hour starts at the 9:30 open |

| `AssetType` | Market | Journal class |
| --- | --- | --- |
| `STOCK` | Equity | stock |
| `STOCKOPTION`, `INDEXOPTION`, `FUTUREOPTION` | Option | option |
| `FUTURE` | Future | future |
| `INDEX` | Index | other |
| `FOREX` | Forex | forex |
| `CRYPTO` | Spot | crypto |
| anything else | not mapped | other |

| `BuyOrSell` | Side |
| --- | --- |
| `Buy`, `BuyToCover`, `BuyToOpen`, `BuyToClose` | Buy |
| `Sell`, `SellShort`, `SellToOpen`, `SellToClose` | Sell |

| Status | Canonical |
| --- | --- |
| 401 | Auth (token dropped) |
| 404 | NotFound ("bad symbol") |
| `Errors` for the account inside a 200 | Permission |
| 429 | RateLimited |
| 5xx | Transient |

## Units and time

- Prices and quantities as decimal strings.
- A bar is stamped at its close: an intraday bar opened one interval earlier, a daily bar is
  its session's date at midnight UTC. `BarStatus: Open` is the forming bar
  (`final_flag = true`).
- A contract's point value is `PriceFormat.PointValue` (an option is not assumed to be a
  hundred, a future never one); its currency is `Currency`.
- A fill is an executed order leg, id `<OrderID>-<leg index>`, stamped at the order's close;
  the order's commission rides its first leg.
- A holding is valued at the last trade.

## Quirks

- The fill archive is 90 days, and `since` takes a date with no end: the period's end
  filters here.
- The intraday hour, four hours and week are anchored on the session open, so they are not
  asked; the engine folds an hour from fifteen minutes on the clock.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)`; an hour is folded on the clock hour (legacy refused it).
- A short sale (`SellShort`, `SellToOpen`, `SellToClose`) is a sell (legacy filed it as a buy).
- A future option is an option (legacy: other); an asset type not mapped is not offered.
- The connection test reads the account's balances (legacy listed every account).
- The latest bid/ask is served (legacy had none on the data side).

## Regions and terms

US brokerage; the API agreement forbids redistributing market data. The refresh token is a
long-lived credential; a sign-in that grants `Trade` makes it a trading credential.
