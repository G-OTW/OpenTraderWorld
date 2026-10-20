# Interactive Brokers

Two adapters, one venue: the Flex Web Service (the account's past, over HTTP) and the local
IB Gateway or TWS socket (market data, over the TWS API).

## Flex Web Service (`ibkr_flex`)

Adapter version: 0.1.0        Provider API version: Flex Web Service v3        Maturity: M2

Sources:
- Flex Web Service: https://www.interactivebrokers.com/campus/ibkr-api-page/flex-web-service/
- Activity Flex Query reference: https://www.ibkrguides.com/reportingreference/reportguide/activity%20flex%20query%20reference.htm

M2: mock catalog green (request, collection while the statement is built, twelve tries at
most, refusals by code, statement read); parity with the legacy connector on mock (fills
over three zone offsets and a symbol filter, every level of detail, cancellations, options,
positions, holdings, connection test, refusals, settings). Real shadow ready
(`OTW_IBKR_FLEX_TOKEN`, `_QUERY_ID`, `_TZ_OFFSET`).

### Endpoints

Base `https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService`.

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `send` | `GET /SendRequest?t&q&v=3` | asking for the statement of query `q` | 1 | none |
| `statement` | `GET /GetStatement?t&q=<reference>&v=3` | collecting it; asked again while code 1019 | 1 | none |

Every read (fills, positions, the connection test) asks for its own statement. The answer's
`Url` names the same host and is not followed: the collection goes to the configured base,
which is what lets a test point the adapter at a mock server.

### Auth

A Flex Web Service token, sent as the `t` parameter by the signer (`ApiKeyQuerySigner`). It
reads reports only, cannot trade, and expires (IBKR mails a reminder).

### Rate limits

One statement per query at a time: a second request while one is being built is refused
(1018). The collection is paced by a declared bucket (`statement`, one call per five seconds)
on the shared driver; twelve collections at most (sixty seconds), then a transient refusal
("try again in a minute").

### Symbols

Native id = the statement's `symbol` (else `underlyingSymbol`, else `description`); an option
loses its OSI padding (`SPY   251219C00650000` to `SPY251219C00650000`). Discovery none:
every row names its own class, exchange, currency and point value (`rows_carry_terms`).

The read is chosen by the `query_id` setting and the statement's zone by `tz_offset`
(minutes off UTC), both carried in `QueryAccount.account` as `query|offset`.

### Enum tables

| `assetCategory` | Market | Journal class |
| --- | --- | --- |
| STK | Equity | stock |
| ETF, FUND | Etf | etf |
| OPT, FOP, WAR | Option | option |
| FUT | Future | future |
| CFD | Cfd | other |
| CASH | Forex | forex |
| CRYPTO | Spot | crypto |
| BOND, BILL | Bond | other |
| CMDTY | Commodity | other |
| anything else | Other (logged) | other |

| Side | Read as |
| --- | --- |
| negative `quantity` | sell (a cancellation `BUY (Ca.)` undoes its buy) |
| `buySell` starting SELL | sell |
| `buySell` starting BUY, or blank | buy |
| anything else | `Unknown(raw)` (logged) |

| `levelOfDetail` | Kept |
| --- | --- |
| EXECUTION | yes |
| ORDER | only when the statement has no execution rows |
| CLOSED_LOT, LOT | never (would double every trade) |

| Error code | Canonical |
| --- | --- |
| 1001, 1004 to 1009, 1019, 1021 | Transient (not ready, under load, being built) |
| 1018 | RateLimited |
| 1011, 1012, 1013, 1015, 1016 | Auth (inactive, expired, IP restriction, invalid token or account) |
| 1010, 1014, 1017, 1020 | InvalidRequest (legacy query, invalid query, reference or request) |
| 1003 | NotFound (statement not available) |
| other | Unknown (logged) |

### Units and time

- A time is `yyyyMMdd;HHmmss` (or with separators, or `HHmm`, or a bare date) in the query's
  zone, which the statement never names: `tz_offset` says it. An unreadable one refuses the
  read with the format to set. The period is `[from, to]` and only filters the statement.
- Numbers may carry thousands separators. A quantity is absolute (the side says the
  direction); a commission is a cost whatever its sign; a point value below one is one.
- A position's average price is `costBasisPrice` when positive; its name is `description`.

### Quirks

- The period belongs to the query (Last 365 Calendar Days, Year to Date...), not to the call.

Deliberate differences from the legacy connector (pinned by tests):

- A CFD is filed as other (legacy: future); BOND, BILL, CMDTY and unknown categories as other
  (legacy: stock).
- Positions and holdings come sorted by symbol; the open positions are offered as symbol
  suggestions (legacy offered none).
- Refusals carry a kind by Flex code (an expired token is Auth, a busy service RateLimited).

### Regions and terms

Flex queries are configured in Account Management; the token is issued under Settings, Flex
Web Service.

## IB Gateway / TWS (`ibkr`)

Adapter version: 0.1.0        Provider API version: TWS API (server version 200 seen), `ibapi` 2.12        Maturity: M2

Sources:
- TWS API: https://interactivebrokers.github.io/tws-api/ (historical bars, contract details,
  matching symbols, tick-by-tick, real-time bars, message codes)
- `ibapi` crate 2.12: https://github.com/wboayue/rust-ibapi (the transport and its types)

M2: contract building, durations, bar sizes, windows, refusals and the live classification
tested offline; real shadow against a live IB Gateway (port 4001, data only) identical to the
legacy connector: 990 MNQ December bars over five timeframes (dated and local-symbol
spellings), 25 bid/ask bars, futures, equity and forex searches, the unknown-contract
refusal. The live line opens on the same contract; its prints are to compare in trading
hours.

### Shape

An adapter, not a mapper: the transport is the `ibapi` client over one socket per gateway,
with its own pacing. Driven by method calls in canonical terms (`history`, `quote`, `search`,
`stream`, `test`); what the gateway answers is translated by the pure `mapper`. OTW's
derivatives tools (futures curves, option chains) read the same `session`, `contract` and
`resolve`, so IB's per-account pacing is counted once.

### Endpoints

| Call | TWS request | Used for | Paging |
| --- | --- | --- | --- |
| connect | handshake, client id from 3100 (16 tried) | every call; `test` re-handshakes | none |
| `contract_details` | `reqContractDetails` | a future's listing before use; the futures search | none |
| `historical_data` | `reqHistoricalData` (end + duration) | candles, bid and ask series | windows per bar size, 24 at most |
| `matching_symbols` | `reqMatchingSymbols` | search (not futures) | none |
| `tick_by_tick_bid_ask` | `reqTickByTickData(BidAsk)` | the market now: first tick within 5 s | none |
| `tick_by_tick_last` / `_midpoint`, `realtime_bars` | `reqTickByTickData`, `reqRealTimeBars` | live | stream |

### Auth

None: an address (`host`, `port`), stored in the clear. The gateway's API must be enabled;
from a container the host is `host.docker.internal` and the container's address must be in
Trusted IPs.

### Rate limits

60 historical requests per rolling ten minutes per account: the session paces 55, at least
1.5 s apart; one connection per gateway, whatever the number of readers. Live subscriptions
are not paced (they are not historical requests) but count against the account's
market-data lines (100 by default) and tick-by-tick streams (its market-depth lines).

### Symbols

Native id = the OTW ticker in capitals; the market rides the venue id (`IBKR`,
`IBKR_CRYPTO`, `IBKR_FX`, `IBKR_INDEX`, `IBKR_FUTURE`, `IBKR_OPTION`). The ticker carries
`@EXCHANGE` and `:CURRENCY` when the defaults (SMART, USD; PAXOS for crypto, IDEALPRO for
forex) are not right; a future is `ROOT.YYYYMM[DD][@EXCHANGE]`, IB's local symbol (`MNQZ6`) or
the continuous `ROOT@EXCHANGE`, always resolved against the gateway (exactly one listing, or
the candidates listed); an option is OCC. Resolved contracts are cached per gateway.

### Enum tables

| Market | History series | Live source | Hours |
| --- | --- | --- | --- |
| equity, etf | TRADES | tick-by-tick last | regular |
| future | TRADES | tick-by-tick last | extended |
| fx, crypto | MIDPOINT | tick-by-tick midpoint (no size) | extended |
| option, index | TRADES | 5 s real-time bars | regular |

| Code | Canonical (history) | Canonical (live) |
| --- | --- | --- |
| 200 | NotFound (no contract matching) | NotFound |
| 354, 10089, 10090 (10091 live) | Permission (no market-data subscription) | Permission |
| 10197, 10167 | Permission | 10197: RateLimited (another session holds the seat) |
| 101, 322, 420 | Unknown | RateLimited (lines exhausted, refusing requests) |
| tick-by-tick words | | RateLimited (ceiling) or Unsupported (none for this contract) |
| 162 | Unknown (pacing or no data in the window) | Transient |
| a dropped socket | Transient after one retry | Transient, code `dropped` (ends the session) |
| gateway unreachable, handshake silent, ids exhausted | ProviderDown | |
| allow-list refusal | Permission | |

### Units and time

- Bars keep IB's stamp (a daily bar is midnight of its trading date); IB flags no bar as
  forming, so the period's end decides. A volume of -1 (no tape) is zero.
- `ibapi` reads TWS text into floats; a price crosses to `Decimal` through its shortest
  round-trip text, so OTW stores the float `ibapi` read, bit for bit.
- A window is asked as an end plus a duration in seconds up to a day (bars up to 30 min),
  else days, else years; at least 30 bars' worth (IB refuses shorter windows, error 321).

### Quirks

- IB answers a contract once per trading class: deduplicated by contract id.
- `reqMatchingSymbols` answers underlyings, never dated futures: futures search the contract
  database instead.
- The gateway restarts itself once a day: a dropped socket is retried once, and a live line
  ends its session so the hub reconnects.

Deliberate differences from the legacy connector:

- Refusals carry a kind (a missing subscription is Permission, a gateway down ProviderDown
  and retried by the worker), shown as the framework writes them.
- An asset type IB is not asked for is refused before any request.

### Regions and terms

Market data is whatever the IB account subscribes to; delayed data is not a live feed. One
live market-data session per account.

## Live best bid and ask

`reqMktData` (ticks 1 bid, 2 ask; `Price` or `PriceSize`), one request per instrument apart
from its tick-by-tick stream, never a tick-by-tick slot. A side at -1 or 0 is empty; delayed
ticks (66, 67) are not a live quote and are ignored; a refusal (354, no market-data
subscription) fails the quote line alone. No time on these ticks: stamped on receipt. Indices
have no bid or ask. To measure on an account: whether the request costs a market-data line on
top of the instrument's own.
