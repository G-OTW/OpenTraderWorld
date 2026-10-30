# Alpaca

Adapter version: 0.1.0        Provider API version: market data v2 (stocks), v1beta3 (crypto), v1beta1 (options); trading v2        Maturity: M2

Sources:
- Stock bars and quotes: https://docs.alpaca.markets/reference/stockbars
- Crypto bars and quotes: https://docs.alpaca.markets/reference/cryptobars-1
- Option bars and quotes: https://docs.alpaca.markets/reference/optionbars
- Streaming: https://docs.alpaca.markets/docs/streaming-market-data
- Assets: https://docs.alpaca.markets/reference/get-v2-assets
- Option contracts: https://docs.alpaca.markets/reference/getoptioncontractsymbolorid
- Account, positions, orders, activities: https://docs.alpaca.markets/reference/getaccount-1

M2: mock catalog green; parity with the legacy connector on mock (bars incl. folded weeks,
quotes, search, socket handshake and bars, account on live and paper). Real shadows ready
with a key (`OTW_ALPACA_API_KEY`, `OTW_ALPACA_API_SECRET`, `OTW_ALPACA_ENV`).

## Endpoints

| Endpoint id | Method + path / WS channel | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `bars` | `GET /v2/stocks/{sym}/bars`, `/v1beta3/crypto/us/bars?symbols`, `/v1beta1/options/bars?symbols` (data host) `timeframe&start&end&limit=10000` | `RequestHistory{Candles}` | 1 | cursor `page_token` |
| `quote` | `GET /v2/stocks/{sym}/quotes/latest`, `/v1beta3/crypto/us/latest/quotes?symbols`, `/v1beta1/options/quotes/latest?symbols` | `RequestSnapshot{Quote}` | 1 | none |
| `quote` | `GET /v2/stocks/{sym}/quotes`, `/v1beta3/crypto/us/quotes?symbols`, `end&sort=desc&limit=1` or `start&sort=asc&limit=1` | `RequestQuoteAt` | 1 | none |
| `assetsEquity`, `assetsCrypto` | `GET /v2/assets?status=active&asset_class=us_equity\|crypto` (trading host) | `LoadInstruments` | 1 each | none |
| `currency`, `asset{n}`, `contract{n}` | `GET /v2/account`, `/v2/assets/{sym}`, `/v2/options/contracts/{sym}` | `ResolveInstruments` (32 per lookup) | 1 each | none |
| `account` | `GET /v2/account` | `QueryAccount{Summary}` | 1 | none |
| `positions` | `GET /v2/positions` | `QueryAccount{Positions}` | 1 | none |
| `orders` | `GET /v2/orders?status=open&limit=500&direction=desc` | `QueryAccount{OpenOrders}` | 1 | none |
| `fills` | `GET /v2/account/activities/FILL?after&until&direction=asc&page_size=100[&page_token]` | `QueryAccount{Fills}` | 1 | last row id, 500 pages max |
| socket | `wss://stream.data.alpaca.markets` `/v2/iex`, `/v2/sip`, `/v1beta3/crypto/us`, `/v1beta1/indicative`, `/v1beta1/opra`; `{"action":"subscribe","bars":[..]}` | `Subscribe{Candles 1m}` | | one socket per market |

## Auth

REST: headers `APCA-API-KEY-ID` and `APCA-API-SECRET-KEY` (`AlpacaSigner`). Socket: the
`{"action":"auth","key","secret"}` frame, written by the signer on the venue's
`success/connected` (`WsLogin`); `subscribe` is sent only after `success/authenticated`.
Secrets: `api_key`, `api_secret`. Settings: `feed` (`iex` free, `sip` paid: picks the live
socket), `env` (`live`, `paper`: paper trading is another host, `paper-api.alpaca.markets`,
with its own key).

## Rate limits

Free key: 200 requests a minute (declared as an account bucket). OTW's worker paces at
350 ms. Over the limit: HTTP 429.

## Symbols

Native id = Alpaca's symbol (`AAPL`, `BTC/USD`, OCC `SPY251219C00650000`), the market riding
the id's venue (`ALPACA`, `ALPACA-CRYPTO`, `ALPACA-OPTIONS`) from OTW's asset type; with none
named, Alpaca's notation decides (a slash is a pair, an OCC symbol an option). An option is
first written as OTW's canonical OCC symbol and must parse as one. The asset list covers
stocks and crypto; options are not listed (OCC symbol typed).

## Enum tables

| Alpaca | Canonical |
| --- | --- |
| `1Min 5Min 15Min 1Hour 1Day` | `1m 5m 15m 1h 1d`; `1w` folded by the engine from `1d`, stamped with the week's first session bar (`aggregate_stamp = "first_bar"`) |
| asset class `us_equity`, `global_equity` / `us_option` / `crypto`, `crypto_perp` | Equity / Option / Spot; any other class is no instrument ("other" on a row) |
| side `buy` / `sell`, `sell_short` | Buy / Sell |
| order `type` (else `order_type`) | kept verbatim |

Socket errors:

| Code | Canonical | Sentence |
| --- | --- | --- |
| 401, 404 | Auth | login not accepted |
| 402 | Auth | key rejected (live versus paper keys) |
| 403 | Transient | |
| 405 | Unsupported | subscription refused |
| 406 | RateLimited (one connection per account) | close the other program |
| 408, 409 | Permission | streaming not enabled / plan lacks the feed |
| 410 | Unsupported | feed does not carry the instrument |

REST: 401 Auth, 403 Permission (a plan or a key of the other environment, named in the
sentence), 404 and 422 NotFound ("bad symbol"), 429 RateLimited, 5xx Transient.

## Units and time

- Prices and sizes as JSON numbers or strings, read from their text.
- Bar `t` RFC 3339, the period's start; a stock day is stamped at New York's midnight.
- No forming flag on REST bars (`final_flag = false`): the engine finalizes. Socket minute
  bars are final.
- A stock or option settles in the account's currency (read once per session), a crypto
  pair in its quote. An option's multiplier is its contract's, never assumed.
- Fills carry no commission (crypto and option fees arrive as separate activities).
- Positions carry Alpaca's own mark (`current_price`): holdings are the positions with their
  cost basis, and a holding is valued at that mark.

## Quirks

- Stocks answer `{bars:[..]}`, crypto and options `{bars:{SYM:[..]}}` keyed by the symbol as
  asked (a pair with its slash).
- Options keep no past quotes.
- `u` (bar correction) and `d` (daily bar) socket messages are ignored on purpose.
- The options socket is documented as msgpack-only; frames are read as JSON, as before.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` with closed bars only.
- A plan refusal on the socket reads as an entitlement fault (legacy: plan); both name the fix.
- An empty search query lists the market; an option search answers nothing (legacy refused
  with a sentence).
- Holdings carry Alpaca's name for every instrument; orders name their exchange.
- Fill and order times keep the millisecond.
- A crypto window with no bar answers an empty chunk (legacy failed it as a bad symbol).

## Regions and terms

Alpaca serves US and international accounts by its own eligibility rules; market data
entitlements follow the plan (IEX and indicative free, SIP and OPRA paid).

## Live best bid and ask

`{"action":"subscribe","quotes":[..]}` on the market's socket after the login: `{T:"q", S, bp,
bs, ap, as, t}` (Alpaca's streaming docs, not measured: no key). The free stock feed is IEX's
book, not the NBBO, and the chart says so. Stock sizes are round lots and option sizes
contracts: not carried. The options socket speaks MessagePack only (412 on a JSON connection)
and has no `bars` channel (410): the live transport asks for MessagePack (`Content-Type:
application/msgpack`) and transcodes both ways, and an option's candles fold from its prints
(`trades`, `{T:"t", S, p, s, t}`).
