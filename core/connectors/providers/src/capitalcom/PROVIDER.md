# Capital.com

Adapter version: 0.1.0        Provider API version: Open API v1, streaming        Maturity: M2

Sources:
- API reference: https://open-api.capital.com/
- Session: https://open-api.capital.com/#tag/Session
- Markets and prices: https://open-api.capital.com/#tag/Markets-Info, https://open-api.capital.com/#tag/Prices
- Accounts and activity: https://open-api.capital.com/#tag/Accounts
- Positions and working orders: https://open-api.capital.com/#tag/Trading-greater-Positions, https://open-api.capital.com/#tag/Trading-greater-Orders
- Streaming: https://open-api.capital.com/#section/WebSocket-API

M2: mock catalog green (two-step sign-in through the driver, signed socket frames, day-by-day
activity walk); parity with the legacy connector on mock (mid candles and both sides on
every timeframe, live and demo hosts, best price, search, refusals, settings; account
summary, fills, positions, holdings, orders, quotes, suggestions, account switch; socket
frames, mid candles, pane and socket refusals). Real shadow ready (`OTW_CAPITALCOM_API_KEY`,
`_IDENTIFIER`, `_PASSWORD`, `_ENV`, `_ACCOUNT_ID`).

## Endpoints

Base `https://api-capital.backend-capital.com` (demo `https://demo-api-capital.backend-capital.com`),
socket `wss://api-streaming-capital.backend-capital.com/connect` for both.

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| token | `POST /api/v1/session` (JSON) | signing in | 1 | none |
| token | `PUT /api/v1/session` (JSON `accountId`) | moving the session to the `account_id` setting | 1 | none |
| `prices` | `GET /api/v1/prices/{epic}?resolution&from&to&max=1000` | `RequestHistory{Candles}` | 1 | time: 1000 candles |
| `market` | `GET /api/v1/markets/{epic}` | `RequestSnapshot{Quote}` | 1 | none |
| `search` | `GET /api/v1/markets?searchTerm` | `SearchInstruments` | 1 | none |
| `details` | `GET /api/v1/markets?epics=A,B` | `ResolveInstruments` (currency and name) | 1 | 50 epics |
| `accounts` | `GET /api/v1/accounts` | `QueryAccount{Summary}` | 1 | none |
| `activity` | `GET /api/v1/history/activity?from&to&detailed=true&filter=type==POSITION;status==ACCEPTED` | `QueryAccount{Fills}` | 1 | walk: 24 hours per call |
| `positions` | `GET /api/v1/positions` | `QueryAccount{Positions}` | 1 | none |
| `orders` | `GET /api/v1/workingorders` | `QueryAccount{OpenOrders}` | 1 | none |
| socket | `OHLCMarketData.subscribe` / `.unsubscribe`, `ping` | `Subscribe{Candles}` | | |

## Auth

`api_key`, `identifier` (the login) and `api_password` (the custom password set on the key)
sign in; the answer's `CST` and `X-SECURITY-TOKEN` headers are the session, and every call
carries both with `X-CAP-API-KEY`. They live ten minutes from their last use: each signed
call or frame extends the kept copy. With `account_id` set, the sign-in has a second step
(`PUT /session`) that moves the session to that financial account and answers a fresh
security token; naming the account the session already starts on is not a refusal. A 401
drops the session and the next call signs in again. Two-factor authentication must be on
to create a key; no key is graded read-only, so this login can trade.

The socket has no handshake: the two tokens ride on every frame, written by the signer
(`Signer::sign_frame`), so the live bridge signs in over REST before opening it.

## Rate limits

10 requests a second per user (`scope = "account"`), one sign-in a second (`token`). The
activity walk is one call per day of the period; a period wider than 400 days is refused.
A session carries 40 socket instruments at most.

## Symbols

Native id = the epic, Capital.com's own name for a market (`GOLD`, `EURUSD`, `US500`,
`OIL_CRUDE`), in capitals; anything but letters, digits, `_`, `.` and `-` is refused.
Discovery is search only; a search line carries the market's name and its type.

The financial account is the `account_id` setting, else the preferred one (Capital.com opens
one per currency).

## Enum tables

| Timeframe | `resolution` |
| --- | --- |
| 1m, 5m, 15m | `MINUTE`, `MINUTE_5`, `MINUTE_15` |
| 1h, 4h | `HOUR`, `HOUR_4` |
| 1d, 1w | `DAY`, `WEEK` (the venue's session, not the UTC day) |

| `instrumentType` | Search line | Journal class |
| --- | --- | --- |
| `CURRENCIES` | fx | forex |
| `INDICES` | index | other |
| `CRYPTOCURRENCIES` | crypto | other |
| anything else (`SHARES`, `COMMODITIES`) | equity | other |

| `orderType` | Price |
| --- | --- |
| `LIMIT` | `orderLevel` is the limit |
| other (`STOP`) | `orderLevel` is the stop |

| Status | Canonical |
| --- | --- |
| sign-in refused | Auth ("check the API key, the login and the custom password") |
| account switch refused | InvalidRequest, naming the account |
| 401 on a call | Auth (session ended, dropped) |
| 404 `error.prices.not-found` | an empty chunk |
| 404 on prices | NotFound ("bad symbol") |
| 404 on a market's price | no quote |
| 429 | RateLimited |
| 5xx | Transient |
| socket `errorCode` naming a token | Auth (terminal) |
| subscription not `PROCESSED` | that pane's NotFound |

## Units and time

- `…UTC` fields are ISO times with no zone, in UTC; query `from`/`to` are written the same
  way, in UTC.
- A candle is the mid of its bid and ask, one side alone when only one is there; both sides
  ride along when all eight prices are there. The last candle is forming and unflagged: its
  period's end decides. `lastTradedVolume` is the volume.
- A streamed candle arrives once per side (`priceType` bid, ask or offer): it is sent once
  both sides of its period are in, at their mid, with no volume. Both sides ride on it
  (`live.candle_sides`): the forming candle's two closes are the chart's live bid and ask, with
  no subscription of their own.
- A fill's identity is deal id, time and direction (the deal id names the position); no fee
  (Capital.com bills the spread and overnight funding). A position's `contractSize` is its
  multiplier.

## Quirks

- Every market is a CFD: a share CFD is not the share, so only a currency pair keeps a
  class of its own.
- The activity log answers 24 hours at a time and needs `detailed=true` to carry size, level
  and currency.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` and leaves the forming candle out; a window with no prices
  is empty instead of an error.
- Naming the account the session already starts on is not a refusal.
- Account rows carry the `account_id` setting; holdings carry the market's name.
- The data connection test proves market data on EURUSD in the framework's words.

## Regions and terms

Capital.com operates through regional entities (UK, EU, Australia, Seychelles, Bahamas);
the markets a login can trade depend on it. The demo platform is a separate host with its
own key.
