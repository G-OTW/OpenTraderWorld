# FOREX.com (StoneX)

Adapter version: 0.1.0        Provider API version: TradingAPI (CIAPI) v1        Maturity: M2

Sources:
- API reference: https://docs.labs.gaincapital.com/
- Session: https://docs.labs.gaincapital.com/#Session-LogOn
- Market search and bars: https://docs.labs.gaincapital.com/#Market-ListMarketSearch, https://docs.labs.gaincapital.com/#PriceHistory-GetPriceBarsBetween
- Account: https://docs.labs.gaincapital.com/#AccountInformation-GetClientAndTradingAccount
- Trades, positions, orders: https://docs.labs.gaincapital.com/#TradesAndOrders

M2: mock catalog green (session open, reuse and drop through the driver; chained lookups);
parity with the legacy connector on mock (mid bars on every served timeframe by name and by
id, bid and ask bars, search, ambiguous names, settings; account trades over a paged walk,
positions, holdings, orders, suggestions, connection test, account picking). Real shadow
ready (`OTW_FOREXCOM_USERNAME`, `_PASSWORD`, `_APP_KEY`, `_TRADING_ACCOUNT_ID`).

## Endpoints

Base `https://ciapi.cityindex.com/TradingAPI`.

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| session | `POST /session` (JSON) | opening the session | 1 | none |
| `tree` | `GET /useraccount/ClientAndTradingAccount` | first step of every command but bars by id | 1 | none |
| `find`, `find0`… | `GET /market/search?searchByMarketName&searchByMarketCode&cfdProductType&query&maxResults=20&ClientAccountId` | the market id behind a name | 1 | none |
| `search` | the same, `maxResults=200` | `SearchInstruments` | 1 | none |
| `bars` | `GET /market/{id}/barhistorybetween?interval&span&fromTimestampUTC&toTimestampUTC&maxResults=4000&priceType=MID\|BID\|ASK` | `RequestHistory{Candles}`, three calls (mid, bid, ask) | 1 each | time: 4000 bars |
| `trades` | `GET /order/tradehistory?TradingAccountId&maxResults=200&from` | `QueryAccount{Fills}` | 1 | walk: `from` moves past the newest row |
| `positions` | `GET /order/openpositions?TradingAccountId` | `QueryAccount{Positions}` | 1 | none |
| `orders` | `GET /order/activestoplimitorders?TradingAccountId` | `QueryAccount{OpenOrders}` | 1 | none |

A command runs as a chain (`Mapper::follow_ups`): the tree, then what it names (the client
account for a search, the trading account for a read), then the market id, then the sides.

## Auth

`username`, `password` and `app_key` open a session (`POST /session`, `AppVersion` 1); the
token rides two headers, `UserName` and `Session`, never the query. It is kept for twenty
minutes per login (hash of user and AppKey), shared with every call on it, and dropped on a
401 (a sign-in elsewhere ends it). StoneX has no read-only credential: the login can trade.

## Rate limits

500 requests per 5 seconds. No bucket declared: OTW's worker paces at 200 ms; a bar chunk
is up to five calls (tree, lookup, three sides).

## Symbols

Native id = the market's name as StoneX writes it, in capitals (`EUR/USD`, `UK 100`), or
its numeric market id. A name is looked up among the first twenty search answers and must
match exactly one market; two are refused with both ids, none with what the search did
find. Search lines carry the id (`EUR/USD (market 401484347)`). Discovery is search only.

The trading account is the `trading_account_id` setting, else the login's only one; a login
with several and no setting is refused with the list (`QueryAccount.account`).

## Enum tables

| Interval | `interval`, `span` |
| --- | --- |
| 1m, 5m, 15m | `MINUTE`, 1/5/15 |
| 1h | `HOUR`, 1 |
| 1d | `DAY`, 1 (the session from the New York close) |

| Market name | Market | Journal class |
| --- | --- | --- |
| three letters, slash, three letters (`EUR/USD`) | Forex | forex |
| anything else (index, commodity, share CFD) | Cfd | other |

| Order `Type` | Price |
| --- | --- |
| 1 | stop: `TriggerPrice` is the stop |
| 2 | limit: `TriggerPrice` is the limit |
| other | `type N`, the trigger as a stop |

| Status | Canonical |
| --- | --- |
| sign-in refused | Auth ("check the username, the password and the AppKey") |
| 401 on a call | Auth (session ended, dropped) |
| name with no market | NotFound ("bad symbol") |
| name with several | InvalidRequest, listing the ids |
| 429 | RateLimited |
| 5xx | Transient |

## Units and time

- Times are `/Date(ms)/`, UTC whatever offset follows.
- Numbers come as JSON floats; ids are integers written `401484347.0`.
- Bars are stamped at their open, with no volume (a dealing desk publishes prices, not
  size); the forming bar (`PartialPriceBar`) is not read. A day is the venue's session.
- A trade's quantity is `OriginalQuantity` (before part closures), else `Quantity`; no fee
  (StoneX bills the spread).

## Quirks

- The trade history takes a `from` and no end, at most 200 rows, "changed since": the walk
  moves `from` past the newest row and drops a row already read (by order id); 200 pages at
  most.
- Live prices are Lightstreamer, not a socket: no live candles, as before.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)`; every download carries the bid and ask (stored next to the
  mid).
- Search symbols are in capitals.
- The data connection test proves market data on EUR/USD in the framework's words.
- An unknown market name reads as a symbol fault.

## Regions and terms

FOREX.com operates through regional StoneX entities; the instruments a login can trade
depend on it. The API agreement is signed before an AppKey is issued.
