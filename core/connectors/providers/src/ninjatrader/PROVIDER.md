# NinjaTrader

Adapter version: 0.1.0        Provider API version: Trader API v1 (tradovateapi)        Maturity: M2

Sources:
- API reference: https://docs.ninjatrader.com/api
- The platform's entity API (the same hosts and paths): https://api.tradovate.com/

M2: mock catalog green (token minted once, reused and dropped on a 401 through the driver;
the chained walk: accounts, orders, fees, fills, then contract, maturity, product and
currency once per contract); parity with the legacy connector on mock (fills over several
accounts and both hosts, symbol filter, fees, point values, positions, holdings, working
orders and their latest version, suggestions, connection test, account picking, settings,
refusals). Real shadow ready (`OTW_NINJATRADER_USERNAME`, `_PASSWORD`, `_CID`, `_SEC`,
`_ENV`, `_ACCOUNT_ID`).

## Endpoints

Base `https://live.tradovateapi.com/v1` (demo `https://demo.tradovateapi.com/v1`).

| Endpoint id | Method + path | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| token | `POST /auth/accesstokenrequest` (JSON) | the bearer token | 1 | none |
| `accounts` | `GET /account/list` | first step of every read; `QueryAccount{Summary}` | 1 | none |
| `orders` | `GET /order/list` | the account behind a fill; `QueryAccount{OpenOrders}` | 1 | none |
| `fees` | `GET /fillFee/list` | the cost of each fill | 1 | none |
| `fills` | `GET /fill/list` | `QueryAccount{Fills}` | 1 | none: the whole list, filtered here |
| `positions` | `GET /position/list` | `QueryAccount{Positions}` | 1 | none |
| `versions` | `GET /orderVersion/list` | a working order's type, size and prices | 1 | none |
| `contract` | `GET /contract/item?id` | a contract's name and maturity | 1 | none |
| `maturity` | `GET /contractMaturity/item?id` | the maturity's product | 1 | none |
| `product` | `GET /product/item?id` | type, `valuePerPoint`, currency id | 1 | none |
| `currency` | `GET /currency/item?id` | the currency's name | 1 | none |

A read runs as a chain (`Mapper::follow_ups`); each contract the kept rows name is walked
once per read. A fee, a version or a contract step that does not answer leaves a gap (no
fee, no version, a contract nobody could identify) rather than failing the read, as before.

## Auth

`username`, `password` and the API key pair `cid` (a number) and `sec` are exchanged for a
bearer token (`appId` OpenTraderWorld, `appVersion` the build's version). The token is kept
until the answer's `expirationTime` (ninety minutes when absent) per login and environment
(hash of the credentials), shared by every call, and dropped on a 401. A 200 with no token
is a refusal (`errorText`: terms to accept, a captcha). There is no read-only key: the same
credentials can trade. Nothing here places, changes or cancels an order.

**Two sessions per login**: a third closes the oldest, which is why the token is shared.

## Rate limits

Not declared: the platform publishes none per endpoint. A read is at most 4 list calls plus
4 per contract.

## Symbols

Native id = the contract's name as the platform writes it (`ESZ5`). Discovery none: the
contracts are the account rows' own, asked by id. An unknown contract is a row with no
name, filed as other, one per point.

The account is the `account_id` setting, else the login's only open one; a login with
several and no setting is refused with the list, a closed account is never offered.

## Enum tables

| `productType` | Market | Journal class |
| --- | --- | --- |
| Futures, Continuous | Future | future |
| CommonStock | Equity | stock |
| Cryptocurrency | Spot | crypto |
| Options | Option | option |
| MarketInternals | Index | other |
| Spread | Other | other |
| anything else | Other (logged) | other |

| `action` | Side |
| --- | --- |
| Buy | buy |
| Sell | sell |
| anything else | `Unknown(raw)` (logged) |

| `ordStatus` | Read as |
| --- | --- |
| Working, PendingNew, PendingReplace | open order |
| anything else | history, left out |

| Status | Canonical |
| --- | --- |
| sign-in refused | Auth ("check the username, the password and the API key pair") |
| 401, 403 on a call | Auth (session ended, two sessions per login) |
| 404 | NotFound |
| 429 | RateLimited |
| 5xx | Transient |

## Units and time

- Times are ISO-8601 UTC (`timestamp`); a fill's period is `[from, to]`.
- Numbers are read from their JSON text. A fill's quantity is in contracts; its point value
  is the product's `valuePerPoint` (1 when the product has none), carried on the row
  (`Fill.multiplier`, `Position.multiplier`).
- A fill's cost is every fee line added up as a positive cost: `clearingFee`,
  `exchangeFee`, `nfaFee`, `brokerageFee`, `ipFee`, `commission`, `orderRoutingFee`.
- A position's sign is its side (`netPos`); its average price is `netPrice` when positive.
- A working order's type, quantity, limit and stop come from its highest numbered version.

## Quirks

- A fill names its order, not its account: the orders are read first.
- How far back `fill/list` reaches is the platform's business and is not published.

Deliberate differences from the legacy connector:

- Fees are added up as decimals (legacy's float sum is a bit off at times).
- An unknown `action` is kept as the platform wrote it, not read as a buy.
- A 403 ends the read as Auth; the token is dropped on a 401 only (the driver's rule).

## Regions and terms

NinjaTrader's API access is issued with a developer key pair on a funded or simulation
login; the simulation platform is its own host with its own account numbers.
