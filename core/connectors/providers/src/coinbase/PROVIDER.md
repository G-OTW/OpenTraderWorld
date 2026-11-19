# Coinbase

Adapter version: 0.1.0        Provider API version: Exchange REST + feed, Advanced Trade v3        Maturity: M2

Sources:
- Exchange candles: https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproductcandles
- Exchange products and ticker: https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproducts
- Exchange feed, `matches`: https://docs.cdp.coinbase.com/exchange/docs/websocket-channels
- Advanced Trade REST: https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/rest-api
  and its OpenAPI document https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/advanced-trade-spec.yaml
- Advanced Trade JWT auth: https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication
- Rate limits: the Advanced Trade REST rate-limit page no longer exists (its old address
  answers HTTP 308 to https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/overview,
  read 2026-11-12); only https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-rate-limits remains.
- Orders: see [Trading](#trading).

M2: mock catalog green; shadow against the live venue (history, product list, feed)
identical to the legacy connector; account parity on mock, real shadow ready with a key.

## Endpoints

| Endpoint id | Method + path / WS channel | Used for | Weight | Paging |
| --- | --- | --- | --- | --- |
| `candles` | `GET /products/{id}/candles?granularity&start&end` (Exchange) | `RequestHistory{Candles}` | 1 | time: 300 steps a call, both ends counted, the end bar opens the next page |
| `products` | `GET /products` (Exchange) | `LoadInstruments` | 1 | none |
| `ticker` | `GET /products/{id}/ticker` (Exchange) | `RequestSnapshot{Quote}` | 1 | none |
| `board`, `productQuotes` | `GET /api/v3/brokerage/market/products?product_type=SPOT` (public) | `RequestBoard`; product quote currencies kept one hour | 1 | none |
| `accounts` | `GET /api/v3/brokerage/accounts?limit=250` (JWT) | `QueryAccount{Summary}`, `{Balances}` | 1 | none |
| `ordersOpen` | `GET /api/v3/brokerage/orders/historical/batch?order_status=OPEN&order_status=PENDING&order_status=QUEUED&order_status=CANCEL_QUEUED&order_status=EDIT_QUEUED&limit=100&order_placement_source=RETAIL_ADVANCED` then `RETAIL_SIMPLE`, `RETAIL_ADMIN`, `RETAIL_RAISE`, `UNKNOWN_PLACEMENT_SOURCE` (JWT); `proof_token_required: true` is an error, never an empty list | `QueryAccount{OpenOrders}` | 1 | cursor `<source>:<venue cursor>`: every page (`has_next`, `cursor`) of each placement source |
| `fills` | `GET /api/v3/brokerage/orders/historical/fills?limit=100&start_sequence_timestamp&end_sequence_timestamp&product_types=SPOT[&product_ids][&cursor]` (JWT) | `QueryAccount{Fills}` | 1 | cursor `<pages>:<venue cursor>`, 500 pages max |
| feed `matches` | `wss://ws-feed.exchange.coinbase.com`, `{"type":"subscribe","product_ids":[..],"channels":["matches"]}` | `Subscribe{Trades}` | | |

## Auth

A JWT per request, valid 120 s: header `{alg: EdDSA | ES256, typ: JWT, kid: <key name>, nonce}`,
payload `{sub: <key name>, iss: cdp, nbf, exp, uri: "GET api.coinbase.com<path>"}` (query
string excluded), sent as `Authorization: Bearer`. The key's form picks the algorithm: an
Ed25519 key (64 base64 bytes, or a 32-byte seed) signs EdDSA; an ECDSA P-256 PEM (`EC PRIVATE
KEY` as Coinbase issues it, or PKCS#8 `PRIVATE KEY`, escaped line breaks accepted) signs
ES256. Secret `api_private_key`; setting
`key_name` (`organizations/<org>/apiKeys/<key>`, or a CDP Ed25519 key's bare id, a UUID, as
the [CDP API keys](https://docs.cdp.coinbase.com/get-started/authentication/cdp-api-keys) page
puts it in `kid` and `sub`; checked when saved). Host clock, no clock
call. EdDSA checked against RFC 8032 vector 1; ES256 against an OpenSSL-made key (its public
point and OpenSSL's signature), each JWT verified with the public key. The docs give no worked
example (no fixed key, time, nonce and token), so no byte-for-byte vector exists.

A socket subscribe frame carries its own JWT in `jwt`, built the same way without `uri`
("WebSocket JWTs (vs those for REST API) are not built with a request method or request path",
[WebSocket authentication](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-authentication);
SDK `build_ws_jwt`), a new one per frame. Orders need a key with the `trade` scope; `view`
reads ([Scopes](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/rest-scopes)).

## Rate limits

- Exchange public REST: about 10 requests per second per IP; OTW's worker paces history at
  200 ms.
- Advanced Trade private REST: no current source. The page that stated a per-second limit
  is gone (see Sources), and the "10,000 requests per hour" of
  https://docs.cdp.coinbase.com/coinbase-app/api-architecture/rate-limiting is the Coinbase App
  v2 API's, not stated for `/api/v3/brokerage`. Doc gap.
- Over the limit: HTTP 429 (`RateLimited`, `Retry-After` honoured when sent).

## Symbols

Native id = the product id (`BTC-USD`), used in URL paths, so only letters, digits and `-`
are accepted. Display symbol = `display_name` (`NEON/USD` for some products, else the id).
Base and quote from `base_currency` / `quote_currency`. Account rows use the product id;
their quote currency comes from the public product list, never from cutting on the dash.
A holding is valued on `<asset>-<quote>` for quote in USD, USDC, USDT, EUR.

## Enum tables

| Coinbase | Canonical |
| --- | --- |
| side `BUY` / `SELL` (account) | `Side::Buy` / `Side::Sell`; anything else `Unknown(raw)` |
| feed `side` `buy` / `sell` (the maker's) | aggressor `Sell` / `Buy`; anything else `Unknown(raw)` |
| granularity 60, 300, 900, 3600, 86400 | `1m 5m 15m 1h 1d` (21600 exists, not declared) |
| wallet `type` containing `FIAT` | fiat wallet (reported as forex) |
| `trading_disabled: true` | inactive |
| order type, order configuration | kept verbatim; prices read from whatever configuration holds them |

Errors: the HTTP status decides (Exchange `{"message":..}`, Advanced Trade
`{"error":..,"message":..}`): 429 RateLimited, 401 Auth, 403 Permission, 404 NotFound ("bad
symbol"), other 4xx InvalidRequest, 5xx Transient. A feed `error` naming a carried product
fails that pane; any other is reported as an error event, not fatal.

## Units and time

- Spot quantities in base units. A fill with `size_in_quote` is sized in money: quantity =
  size / price (exact decimals); at price 0 it is refused with a sentence.
- Candle row `[time, low, high, open, close, volume]`, newest first, JSON numbers (read
  from their text), `time` in seconds, candle open time, UTC epoch aligned.
- No forming flag on candles (`history.candles.final_flag = false`): every bar goes out
  forming and the engine finalizes at the period's end.
- Fill `trade_time`, order `created_time`, feed `time`: RFC 3339, kept to the millisecond.
- Fees as stated (`commission`, "Fee amount for fill"; its sign is doc silent, so a negative
  one stays a rebate and is never made a cost), in the quote currency (the fill names none:
  doc silent, taken from the product list).
- An adjusted fill (`trade_type` `REVERSAL`, `CORRECTION`, `SYNTHETIC`) refuses the pull with a
  sentence naming it: how one undoes or replaces a fill is doc silent.
- An order placed in the quote currency (`size_in_quote`, `quote_size`) states no base size
  until it fills: its size is what it filled, its type reads `LIMIT (50 in the quote currency)`.
- Wallet total = available + hold.

## Quirks

- A candle window counts both ends: 300 steps answer 301 bars, 301 steps are refused
  ("Count of aggregations requested exceeds 300"). An unaligned start begins at the next
  bar (checked against the live venue).
- Fills and orders carry no quote currency; the public product list does.
- The public product list carries a price per product; an unpriced product is left off the
  board.
- The feed streams trades only: every live timeframe is folded by OTW from them.

Deliberate differences from the legacy connector (each pinned by a test):

- History answers `[from, to)` with committed bars only.
- A venue outage reads as the provider's in the chart's notice (legacy blamed the symbol).
- Fill and order times keep the millisecond (legacy kept microseconds).
- Wallet totals and quote-sized quantities are exact decimals.
- Suggestions count an asset held entirely by an open order, and add products with a
  working order.
- The price board is the public product list: valuing holdings needs no signed call.
- The feed subscribes a product once whatever the number of panes on it.
- A fill pull longer than 500 pages is an error asking for a shorter period.

## Regions and terms

Advanced Trade accounts follow Coinbase's jurisdictions; the Exchange market data answered
from the test machine. Coinbase offers a sandbox for the Exchange API, not used here.

## Live best bid and ask

Channel `level2_batch` (answered as `level2_50`) on the trade socket: a `snapshot` of the whole
book (about 1 MB on BTC-USD) then `l2update` changes every 50 ms, size 0 removing a level; the
top of the book is the quote (measured 2026-10-19). `ticker` carries a best bid/ask only when a
trade prints and is not used.

## Trading

Execution maturity: **E2D**, spot book only, built from the documentation alone: no account, no
key, no canary. Coinbase has no test account for Advanced Trade: its sandbox is static ("All
responses are static and pre-defined", [Sandbox](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/sandbox)),
so `is_sandbox` is always false and the sandbox served only as a source of fixtures. Code:
`trade.rs` (mapper), `trade_errors.rs` (error table); the order path is the shared trader's
(`connectors/engine/src/trading.rs`).

Doc snapshot, read 2026-11-12 (base `https://docs.cdp.coinbase.com`): the OpenAPI document
every REST reference page embeds (`advanced-trade-spec.yaml`, sha256 `e1edecbc619a5058...`), the
AsyncAPI document (`advanced-trade-asyncapi.json`, sha256 `d60f9b4cc0711256...`) and these pages:

| Page | Used for |
| --- | --- |
| [Create Order](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order) | body, `client_order_id`, `order_configuration` keys and fields, `stop_direction`, `post_only`, `end_time`, the answer `{success, success_response, error_response}` |
| [Cancel Orders](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/cancel-order) | `POST /orders/batch_cancel {order_ids}`, `results[]` per id |
| [Edit Order](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/edit-order) | `order_id`, `price`, `size` (all three required), `{success, errors[]}` |
| [Get Order](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/get-order) | lookup by venue id; `client_order_id` query "(Deprecated)" |
| [List Orders](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-orders) | open orders, lookup by client id (list and match), `has_next`, `cursor`, `start_date`, `order_placement_source` |
| [List Fills](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-fills) | fills, `entry_id`, `trade_type`, `commission`, `product_types`, `limit` "default is 100" |
| [Preview Orders](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/preview-orders) | dry run, `errs[]` |
| [List Public Products](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/public/list-public-products) | trading rules and states, `pagination` |
| [REST API](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/rest-api) | host, "Public responses are cached for 1 second" |
| [Orders guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/orders) | order types, "To buy a product, provide a `quote_size` or `base_size`", stops "based on the movement of the last trade price", `market_market_fok` "(Perpetuals only)" |
| [FAQ](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/faq) | `success: true` with `UNKNOWN_FAILURE_REASON` is an accepted order; `UNKNOWN` in `warning` is no warning |
| [Scopes](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/rest-scopes) | `trade` for place, cancel, edit; `view` for preview, lists, fills |
| [Sandbox](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/sandbox) | static answers, `X-Sandbox` error variants (fixtures) |
| [API key authentication](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication), [JWT authentication](https://docs.cdp.coinbase.com/get-started/authentication/jwt-authentication) | REST JWT (see [Auth](#auth)) |
| [WebSocket overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-overview), [endpoints](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-endpoints), [authentication](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-authentication), [rate limits](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-rate-limits), [guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/websocket) | user socket host, a JWT per subscribe frame, one channel per subscribe, heartbeats |
| [User channel](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/websocket/user), [Heartbeats](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/websocket/heartbeats) | private stream frames |
| [Overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/overview), [Perpetuals guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/perpetual), [Futures guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/futures) | books left out (see [Left out](#left-out)) |

The official SDK (github.com/coinbase/coinbase-advanced-py, `coinbase/jwt_generator.py` and
`coinbase/websocket/websocket_base.py` at `master`, read 2026-11-12) is used only where the
docs are silent and is marked "(SDK)".

### Order types and flags

| Canonical | Native (`order_configuration` key) | Notes |
| --- | --- | --- |
| `market` | `market_market_ioc {base_size}` | a buy as a sell, sized in the base coin: "To buy a product, provide a `quote_size` or `base_size`; to sell, provide a `base_size`" ([Orders guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/orders)). A market order asked FOK is refused before I/O (`market_market_fok` is "(Perpetuals only)"), and one asked GTD too. |
| `limit` GTC | `limit_limit_gtc {base_size, limit_price[, post_only]}` | |
| `limit` GTD | `limit_limit_gtd {base_size, limit_price, end_time[, post_only]}` | `end_time` RFC 3339, "The time at which the order will be cancelled if it is not Filled" |
| `limit` IOC | `sor_limit_ioc {base_size, limit_price}` | "any remaining quantity is canceled" |
| `limit` FOK | `limit_limit_fok {base_size, limit_price}` | "immediately and completely Fill" |
| `stop_limit` GTC / GTD | `stop_limit_stop_limit_gtc` / `_gtd {base_size, limit_price, stop_price, stop_direction[, end_time]}` | a buy `STOP_DIRECTION_STOP_UP`, a sell `STOP_DIRECTION_STOP_DOWN`: "If Up, then the Order will trigger when the last trade price goes above the stop_price. If Down, ... below" ([Create Order](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order)). Last trade price only: no mark or index trigger. IOC and FOK refused before I/O. |
| `lit` (limit if touched) GTC / GTD | the same configuration, the other direction | a buy `STOP_DOWN`, a sell `STOP_UP` |
| `post_only` | `post_only: true` on `limit_limit_gtc` / `_gtd` | "Orders that will be posted as a Taker Order will be rejected" |

Not offered (refused before I/O): `stop` (stop-market: no configuration has one, the
`STOP` order type exists only in the read enum), `mit`, `trailing_stop`, `Day` (`MARKET_GFD`
and `LIMIT_GFD` are equity only), iceberg (`ICEBERG_ORDERS_NOT_SUPPORTED`), reduce-only (no
request field; `REDUCE_ONLY_NOT_ALLOWED_ON_SPOT_PRODUCTS`), attached take profit and stop loss
([Left out](#left-out)), TWAP, scaled, OCA. Neither `leverage` nor `margin_type` is ever sent: a
spot order carries none.

**Change in place** (`amend`): `POST /orders/edit {order_id, price, size}` on a working limit
order sized in the coin; the venue id is kept. `size` is the order's whole size (whether it is
the total or what remains is doc silent; `CANNOT_EDIT_TO_BELOW_FILLED_SIZE` reads as the total).
A stop-limit, an order placed in the quote currency (`SIZE_IN_QUOTE_EDITS_NOT_ALLOWED`), a new
stop price or time in force are refused before I/O (`ONLY_LIMIT_ORDER_EDITS_SUPPORTED`; the
`stop_price` field is "Only applicable for editing TP/SL or SL orders"). Whether a change keeps
the order's place in the book is doc silent (`replace_accept_timestamp` in `edit_history`
suggests a replace): not declared.

**Cancel**: `POST /orders/batch_cancel {order_ids: [id]}`, the only cancel. The kill switch
lists every working order (every working status of every placement source, every page) and cancels them in batches of
at most 50 ids (no maximum is documented: a small batch, [Doc gaps](#doc-gaps)); a row that
failed is cancelled again on its own, and what is still listed after is walked one by one.

### Time in force

`gtc`, `gtd`, `ioc`, `fok`, each by its own configuration key (table above). A market order is
always immediate-or-cancel: asked GTC it goes out as `market_market_ioc`, asked FOK or GTD it is
refused. A stop-limit is GTC or GTD only.

### Client order id

`client_order_id`, required on create: "A unique ID provided for the order (used for
identification purposes). If the ID provided is not unique, the order will not be created and
the order corresponding with that ID will be returned instead" ([Create Order](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order)).
Its charset and length are doc silent (the example is `0000-00000-000000`; the SDK sends a
32-character hex UUID): OTW sends its key unchanged under the commonest rule, at most 36
letters, digits, `-` and `_`. A preview carries none (the sandbox answers `unknown field
"client_order_id"` to one that does), so the venue never sees the live order's id before the
order.

**Lookup by client id.** No call takes one: Get Order's `client_order_id` query is
"(Deprecated)" and its path still wants the venue's id, and List Orders has no client id
filter. A lost answer is therefore looked up by listing the product's orders (every state, from
`start_date` one minute before the order was sent when that time is held) and matching
`client_order_id`, following `cursor` for at most 5 pages. Not found there: the order stays
unknown and is looked up again by the reconciliation cycle, never resent.

### Order status

`OrderExecutionStatus` ([List Orders](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-orders)):

| Coinbase | Canonical |
| --- | --- |
| `PENDING` ("not yet open"), `QUEUED` | `PendingNew` |
| `OPEN` | `New`; `PartiallyFilled` when `filled_size` is above zero; `PendingCancel` when `pending_cancel` is true |
| `FILLED` | `Filled` |
| `CANCELLED` | `Cancelled` (with its fill, `filled_size`, when partly filled) |
| `EXPIRED` | `Expired` |
| `FAILED` ("could not be placed") | `Rejected` |
| `CANCEL_QUEUED` | `PendingCancel` |
| `EDIT_QUEUED` | `PendingReplace` |
| `UNKNOWN_ORDER_STATUS`, any other word | unknown, alerts |

A stop-limit waiting for its trigger is `OPEN` with `trigger_status` `STOP_PENDING`: `New`,
trigger pending. `filled_size` is "(in base currency)". An order placed in the quote currency
(`size_in_quote`, placed outside OTW) states no base size until it fills: its size is what it
filled (never one computed from the quote amount), and its type reads `LIMIT (50 in the quote
currency)`, on the order path as on the account read.

### Instrument trading rules

From [List Public Products](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/public/list-public-products)
(`GET /api/v3/brokerage/market/products?product_type=SPOT&product_ids=<id>`, no key, "cached for
1 second"), paged by `pagination.next_cursor` while `has_next`:

| OTW | Coinbase |
| --- | --- |
| tick | `price_increment` "Minimum amount price can be increased or decreased at once" |
| lot step | `base_increment` |
| quote step | `quote_increment` |
| min / max qty | `base_min_size` / `base_max_size` |
| min notional | `quote_min_size` ("Minimum size that can be represented of quote currency", worded as `base_min_size`, the min size; `PREVIEW_INVALID_QUOTE_SIZE_TOO_SMALL`); `quote_max_size` not read |
| status | below |

| Coinbase | OTW |
| --- | --- |
| `trading_disabled` ("disabled for trading for all market participants"), `is_disabled`, `view_only` | closed (the flag's name) |
| `status` other than `online` (seen: `online`, `delisted`; the words are doc silent) | closed (the word) |
| `cancel_only` ("can only be cancelled, not placed or edited") | closed (`cancel_only`): reduce-only cannot be sent anyway |
| `post_only` ("can only be posted") | post-only limits only (`TradingStatus::PostOnly`) |
| `limit_only` ("can only be limit orders, not market orders") | orders with a limit price only (`TradingStatus::LimitOnly`) |
| `auction_mode` | not read: what it allows is doc silent, the venue decides |

A product not trading now is read again on the next order. An order on a `-USDC` product whose
`alias` is the `-USD` one (`BTC-USDC` alias `BTC-USD`) goes out under the id the user chose;
which id the venue reports it under is doc silent, and the id the venue answers is kept.

### Errors

Refusals travel inside HTTP 200 bodies, by call: a place `success: false` with
`error_response` (`new_order_failure_reason`, else the deprecated `error`, else the deprecated
`preview_failure_reason`, as the sandbox's insufficient-funds answer carries only those two), a
preview `errs[]`, a cancel `results[]` row with `success: false` and its `failure_reason`, an
edit `success: false` with `errors[]`. `success` decides, never the reason field (FAQ). The
user sees `Coinbase refused it: <message> (code <reason>)`. A reason the table does not list is
unknown and alerts: `Coinbase answered code <reason> (<message>), which OpenTraderWorld does not
map`.

Outside a 200, the gRPC gateway's body (`{error, code, message, details}`, OpenAPI `default`
response; its `code` values are doc silent) under an HTTP status, which decides (what each status
means for a key is doc silent): 401 Auth, 403 Permission, 429 RateLimited (`Retry-After` honoured), 404 on Get Order not found, any
other 4xx InvalidRequest, 5xx, a timeout or a dropped connection unknown (looked up, never
resent). `success: false` with no reason at all is unknown too.

Every value of the four failure enums of the OpenAPI document as mapped in `trade_errors.rs` (a
test asserts each row maps as listed, in the create and preview shapes, and that an unlisted
one is unknown and alerts):

| Canonical | Reasons | Enum | Action |
| --- | --- | --- | --- |
| Unknown | `UNKNOWN_FAILURE_REASON`, `COMMANDER_REJECTED_NEW_ORDER`, `TOO_MANY_PENDING_REPLACES`, `MODIFY_POSITION_WORKFLOW_ALREADY_RUNNING`, `DUPLICATE_CLIENT_ORDER_ID` | new order | looked up by its client id (an order) or read again (a cancel, a change), never resent |
| Unknown | `UNKNOWN_PREVIEW_FAILURE_REASON`, `PREVIEW_RISK_PROXY_FAILURE`, `PREVIEW_TOO_MANY_PENDING_REPLACES` | preview | looked up by its client id (an order) or read again (a cancel, a change), never resent |
| Unknown | `UNKNOWN_CANCEL_FAILURE_REASON`, `INVALID_CANCEL_REQUEST`, `UNKNOWN_CANCEL_ORDER`, `COMMANDER_REJECTED_CANCEL_ORDER`, `DUPLICATE_CANCEL_REQUEST`, `ORDER_IS_BEING_REPLACED` | cancel | looked up by its client id (an order) or read again (a cancel, a change), never resent |
| Unknown | `UNKNOWN_EDIT_ORDER_FAILURE_REASON`, `COMMANDER_REJECTED_EDIT_ORDER`, `ORDER_IS_ALREADY_BEING_REPLACED` | edit | looked up by its client id (an order) or read again (a cancel, a change), never resent |
| RateLimited | `RFQ_RATE_LIMITED` | new order | refused, new orders held |
| Rejected(funds) | `INSUFFICIENT_FUND`, `INVALID_LEDGER_BALANCE`, `INSUFFICIENT_FUNDS`, `IN_LIQUIDATION`, `UBO_HIGH_LEVERAGE_QUANTITY_BREACHED`, `BREACHED_RISK_LIMIT` | new order | refused |
| Rejected(funds) | `PREVIEW_INVALID_LEDGER_BALANCE`, `PREVIEW_INSUFFICIENT_LEDGER_BALANCE`, `PREVIEW_INSUFFICIENT_FUND`, `PREVIEW_INSUFFICIENT_FUNDS_FOR_FUTURES`, `PREVIEW_INVALID_MARGIN_HEALTH`, `PREVIEW_IN_LIQUIDATION`, `PREVIEW_UBO_HIGH_LEVERAGE_QUANTITY_BREACHED`, `PREVIEW_ECOSYSTEM_LEVERAGE_UTILIZATION_BREACHED`, `PREVIEW_UBO_HIGH_LEVERAGE_NOTIONAL_BREACHED`, `PREVIEW_BREACHED_RISK_LIMIT` | preview | refused |
| Rejected(venue_rule) | `INVALID_SIZE_PRECISION`, `INVALID_PRICE_PRECISION`, `INVALID_ATTACHED_TAKE_PROFIT_PRICE_PRECISION`, `INVALID_ATTACHED_STOP_LOSS_PRICE_PRECISION`, `INVALID_ATTACHED_TAKE_PROFIT_SIZE_BELOW_MIN`, `BUCKET_SIZE_SMALLER_THAN_QUOTE_MIN`, `BUCKET_SIZE_SMALLER_THAN_BASE_MIN`, `INVALID_RFQ_BASE_SIZE_TOO_SMALL`, `INVALID_RFQ_BASE_SIZE_TOO_LARGE`, `INVALID_RFQ_QUOTE_SIZE_TOO_SMALL`, `INVALID_RFQ_QUOTE_SIZE_TOO_LARGE`, `FRACTIONAL_ORDERS_NOT_ALLOWED_FOR_PRODUCT`, `QUOTE_ORDERS_NOT_ALLOWED_FOR_PRODUCT`, `NOTIONAL_SIZE_BREACHES_FRACTIONAL_MINIMUM` | new order | refused, rules read again |
| Rejected(venue_rule) | `PREVIEW_INVALID_SIZE_PRECISION`, `PREVIEW_INVALID_PRICE_PRECISION`, `PREVIEW_INVALID_BASE_SIZE_TOO_LARGE`, `PREVIEW_INVALID_BASE_SIZE_TOO_SMALL`, `PREVIEW_INVALID_QUOTE_SIZE_PRECISION`, `PREVIEW_INVALID_QUOTE_SIZE_TOO_LARGE`, `PREVIEW_INVALID_QUOTE_SIZE_TOO_SMALL`, `PREVIEW_INVALID_STOP_PRICE_PRECISION`, `PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_PRECISION`, `PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE_PRECISION`, `PREVIEW_INVALID_LIMIT_PRICE_PRECISION`, `PREVIEW_INVALID_STOP_TRIGGER_PRICE_PRECISION`, `PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_SIZE_BELOW_MIN`, `PREVIEW_BELOW_MIN_SIZE_FOR_DURATION`, `PREVIEW_BUCKET_SIZE_SMALLER_THAN_QUOTE_MIN`, `PREVIEW_BUCKET_SIZE_SMALLER_THAN_BASE_MIN`, `PREVIEW_INVALID_RFQ_BASE_SIZE_TOO_SMALL`, `PREVIEW_INVALID_RFQ_BASE_SIZE_TOO_LARGE`, `PREVIEW_INVALID_RFQ_QUOTE_SIZE_TOO_SMALL`, `PREVIEW_INVALID_RFQ_QUOTE_SIZE_TOO_LARGE`, `PREVIEW_FRACTIONAL_ORDERS_NOT_ALLOWED_FOR_PRODUCT`, `PREVIEW_QUOTE_ORDERS_NOT_ALLOWED_FOR_PRODUCT`, `PREVIEW_NOTIONAL_SIZE_BREACHES_FRACTIONAL_MINIMUM`, `PREVIEW_INVALID_BRACKET_LIMIT_PRICE_PRECISION`, `PREVIEW_PREDICTIONS_QUOTE_SIZE_BELOW_MIN_CONTRACT_PRICE` | preview | refused, rules read again |
| Rejected(venue_rule) | `INVALID_EDITED_SIZE` | edit | refused, rules read again |
| Rejected(price) | `INVALID_LIMIT_PRICE`, `INVALID_BRACKET_PRICES`, `INVALID_BRACKET_LIMIT_PRICE`, `INVALID_BRACKET_STOP_TRIGGER_PRICE`, `BRACKET_LIMIT_PRICE_OUT_OF_BOUNDS`, `STOP_TRIGGER_PRICE_OUT_OF_BOUNDS`, `LIMIT_PRICE_TOO_FAR_FROM_MARKET`, `INVALID_ATTACHED_TAKE_PROFIT_PRICE`, `INVALID_ATTACHED_STOP_LOSS_PRICE`, `INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS`, `INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS`, `INVALID_ATTACHED_TAKE_PROFIT_PRICE_EXCEEDS_MAX_DISTANCE_FROM_ORIGINATING_PRICE`, `ATTACHED_ORDER_MUST_HAVE_POSITIVE_PRICES`, `LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_BUY`, `LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_SELL`, `INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER`, `INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER`, `STOP_ALREADY_TRIGGERED`, `INVALID_NBBO_BID_PRICE`, `INVALID_NBBO_ASK_PRICE`, `STOP_LOSS_PRICE_TOO_LOW`, `ATTACHED_STOP_LOSS_PRICE_TOO_LOW`, `STOP_LOSS_PRICE_TOO_HIGH`, `ATTACHED_STOP_LOSS_PRICE_TOO_HIGH`, `TAKE_PROFIT_PRICE_TOO_HIGH`, `ATTACHED_TAKE_PROFIT_PRICE_TOO_HIGH`, `TAKE_PROFIT_PRICE_TOO_LOW`, `ATTACHED_TAKE_PROFIT_PRICE_TOO_LOW` | new order | refused |
| Rejected(price) | `PREVIEW_INVALID_LIMIT_PRICE`, `PREVIEW_INVALID_STOP_PRICE`, `PREVIEW_INVALID_PRICE_TOO_LARGE`, `PREVIEW_BREACHED_PRICE_LIMIT`, `PREVIEW_INVALID_BRACKET_PRICES`, `PREVIEW_INVALID_BRACKET_LIMIT_PRICE`, `PREVIEW_INVALID_BRACKET_STOP_TRIGGER_PRICE`, `PREVIEW_BRACKET_LIMIT_PRICE_OUT_OF_BOUNDS`, `PREVIEW_STOP_TRIGGER_PRICE_OUT_OF_BOUNDS`, `PREVIEW_STOP_PRICE_ABOVE_LIMIT_PRICE`, `PREVIEW_STOP_PRICE_BELOW_LIMIT_PRICE`, `PREVIEW_STOP_PRICE_ABOVE_LAST_TRADE_PRICE`, `PREVIEW_STOP_PRICE_BELOW_LAST_TRADE_PRICE`, `PREVIEW_LIMIT_PRICE_TOO_FAR_FROM_MARKET`, `PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE`, `PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE`, `PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS`, `PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS`, `PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_EXCEEDS_MAX_DISTANCE_FROM_ORIGINATING_PRICE`, `PREVIEW_ATTACHED_ORDER_MUST_HAVE_POSITIVE_PRICES`, `PREVIEW_LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_BUY`, `PREVIEW_LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_SELL`, `PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER`, `PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER`, `PREVIEW_STOP_TRIGGERED`, `PREVIEW_INVALID_NBBO_BID_PRICE`, `PREVIEW_INVALID_NBBO_ASK_PRICE`, `PREVIEW_STOP_LOSS_PRICE_TOO_LOW`, `PREVIEW_PREDICTIONS_HIGH_PRICE_CONTRACTS_BLOCKED`, `PREVIEW_ATTACHED_STOP_LOSS_PRICE_TOO_LOW`, `PREVIEW_STOP_LOSS_PRICE_TOO_HIGH`, `PREVIEW_ATTACHED_STOP_LOSS_PRICE_TOO_HIGH`, `PREVIEW_TAKE_PROFIT_PRICE_TOO_HIGH`, `PREVIEW_ATTACHED_TAKE_PROFIT_PRICE_TOO_HIGH`, `PREVIEW_TAKE_PROFIT_PRICE_TOO_LOW`, `PREVIEW_ATTACHED_TAKE_PROFIT_PRICE_TOO_LOW` | preview | refused |
| Rejected(price) | `INVALID_EDITED_PRICE`, `INVALID_EDITED_STOP_PRICE`, `INVALID_EDITED_STOP_LIMIT_PRICE`, `INVALID_EDITED_TAKE_PROFIT_PRICE`, `INVALID_EDITED_STOP_LOSS_PRICE` | edit | refused |
| Rejected(post_only) | `INVALID_LIMIT_PRICE_POST_ONLY` | new order | refused |
| Rejected(post_only) | `PREVIEW_INVALID_LIMIT_PRICE_POST_ONLY` | preview | refused |
| Rejected(reduce_only) | `POSITION_SIZE_INCREASE_REJECT`, `ASSET_BALANCE_INCREASE_REJECT`, `REDUCE_ONLY_INCREASED_POSITION_SIZE` | new order | refused |
| Rejected(reduce_only) | `PREVIEW_POSITION_SIZE_INCREASE_REJECT`, `PREVIEW_ASSET_BALANCE_INCREASE_REJECT`, `PREVIEW_REDUCE_ONLY_INCREASED_POSITION_SIZE` | preview | refused |
| Rejected(market closed) | `ORDER_ENTRY_DISABLED`, `UNTRADABLE_PRODUCT`, `INVALID_FCM_TRADING_SESSION`, `FUTURES_AFTER_HOUR_INVALID_ORDER_TYPE`, `FUTURES_AFTER_HOUR_INVALID_TIME_IN_FORCE`, `MARKET_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION`, `NOTIONAL_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION`, `INVALID_EQUITY_TRADING_SESSION`, `PRODUCT_TRADING_HALTED`, `TRADING_DISABLED` | new order | refused |
| Rejected(market closed) | `PREVIEW_MISSING_PRODUCT_PRICE_BOOK`, `PREVIEW_UNTRADABLE_PRODUCT`, `PREVIEW_INVALID_FCM_TRADING_SESSION`, `PREVIEW_NOT_ALLOWED_BY_MARKET_STATE`, `PREVIEW_CLOSE_ONLY_FAILURE`, `PREVIEW_FUTURES_AFTER_HOUR_INVALID_ORDER_TYPE`, `PREVIEW_FUTURES_AFTER_HOUR_INVALID_TIME_IN_FORCE`, `PREVIEW_MARKET_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION`, `PREVIEW_NOTIONAL_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION`, `PREVIEW_INVALID_EQUITY_TRADING_SESSION`, `PREVIEW_PRODUCT_TRADING_HALTED`, `PREVIEW_TRADING_DISABLED` | preview | refused |
| Rejected(market closed) | `INVALID_CANCEL_FCM_TRADING_SESSION` | cancel | refused |
| Permission | `INELIGIBLE_PAIR`, `GEOFENCING_RESTRICTION` | new order | refused |
| Permission | `PREVIEW_UNTRADABLE_FCM_ACCOUNT_STATUS`, `PREVIEW_GEOFENCING_RESTRICTION` | preview | refused |
| Permission | `NOT_ALLOWED_TO_CANCEL` | cancel | refused |
| Permission | `CALLER_ID_MISMATCH`, `PORTFOLIO_NOT_ALLOWED_FOR_EDITS` | edit | refused |
| Rejected(final) | `ORDER_IS_PENDING_CANCEL` | new order | a cancel is reconciled: no error when it ended |
| Rejected(final) | `PREVIEW_ORDER_IS_PENDING_CANCEL` | preview | a cancel is reconciled: no error when it ended |
| Rejected(final) | `ORDER_IS_FULLY_FILLED` | cancel | a cancel is reconciled: no error when it ended |
| Rejected(final) | `ONLY_OPEN_ORDERS_CAN_BE_EDITED`, `CANNOT_EDIT_ORDER_PENDING_CANCEL` | edit | a cancel is reconciled: no error when it ended |
| Rejected(not found) | `ORDER_NOT_FOUND`, `FUTURES_VENUE_ORDER_NOT_FOUND` | edit | reconciled by Get Order |
| Rejected(other) | `INVALID_NO_LIQUIDITY`, `MISSING_MARKET_TRADE_DATA`, `OPEN_BRACKET_ORDERS`, `SCALED_PARAM_INFEASIBLE`, `SCALED_MIN_ORDER_VIOLATION`, `SCALED_MAX_ORDER_VIOLATION`, `MAX_NOTIONAL_PER_ORDER_BREACHED_15C35_CHECK`, `MAX_SHARES_PER_ORDER_BREACHED_15C35_CHECK`, `CANNOT_CLOSE_ZERO_POSITION`, `SCALED_PARAM_DISCREPANCY`, `RFQ_QUOTE_EXPIRED`, `RFQ_TOO_MANY_OPEN_QUOTES`, `RFQ_VALIDATION_FAILURE`, `RFQ_VENUE_REJECTED` | new order | refused, the venue's words shown |
| Rejected(other) | `PREVIEW_INVALID_NO_LIQUIDITY`, `PREVIEW_BREACHED_ACCOUNT_POSITION_LIMIT`, `PREVIEW_BREACHED_COMPANY_POSITION_LIMIT`, `PREVIEW_BREACHED_OPEN_INTEREST_LIMIT`, `PREVIEW_OPPOSITE_MARGIN_TYPE_EXISTS`, `PREVIEW_MISSING_MARKET_TRADE_DATA`, `PREVIEW_BRACKET_ORDER_SIZE_EXCEEDS_POSITION`, `PREVIEW_ORDER_SIZE_EXCEEDS_BRACKETED_POSITION`, `PREVIEW_MAX_DAILY_VOLUME_NOTIONAL_BREACHED`, `PREVIEW_COMPLIANCE_PURCHASE_LIMIT_EXCEEDED`, `PREVIEW_SCALED_PARAM_INFEASIBLE`, `PREVIEW_SCALED_MIN_ORDER_VIOLATION`, `PREVIEW_SCALED_MAX_ORDER_VIOLATION`, `PREVIEW_MAX_NOTIONAL_PER_ORDER_BREACHED_15C35_CHECK`, `PREVIEW_MAX_SHARES_PER_ORDER_BREACHED_15C35_CHECK`, `PREVIEW_SCALED_PARAM_DISCREPANCY` | preview | refused, the venue's words shown |
| Rejected(other) | `CANNOT_EDIT_TO_BELOW_FILLED_SIZE`, `NOT_STARTED`, `CANNOT_EDIT_FUTURES_ORDER`, `CANNOT_EDIT_STOP_PRICE_FOR_TRIGGERED_ORDER`, `INSUFFICIENT_TIME_TO_EDIT`, `EXCEEDED_MAX_ALLOWED_EDIT_REQUEST_COUNT`, `CANNOT_CANCEL_ACTIVE_ATTACHED_ORDER`, `CANNOT_EDIT_TRIGGERED_ORDER`, `CANNOT_EDIT_ATTACHED_ORDER_CONFIGURATION_AFTER_CREATION`, `CANNOT_EDIT_ATTACHED_ORDER_SIZE_WITH_OPEN_ORIGINATING_ORDER`, `CANNOT_EDIT_ATTACHED_SL_ORDER_PRICE`, `CANNOT_MODIFY_ATTACHED_CONFIG_OF_NON_NEW_ORDER` | edit | refused, the venue's words shown |
| InvalidRequest | `UNSUPPORTED_ORDER_CONFIGURATION`, `INVALID_SIDE`, `INVALID_PRODUCT_ID`, `INVALID_REQUEST`, `INVALID_MARGIN_TYPE`, `INVALID_LEVERAGE`, `QUOTE_SIZE_NOT_ALLOWED_FOR_BRACKET`, `BRACKET_ORDER_NOT_SUPPORTED`, `FOK_DISABLED`, `FOK_ONLY_ALLOWED_ON_LIMIT_ORDERS`, `POST_ONLY_NOT_ALLOWED_WITH_FOK`, `END_TIME_TOO_FAR_IN_FUTURE`, `ATTACHED_ORDER_SIZE_MUST_BE_NIL`, `INVALID_SETTLEMENT_CURRENCY`, `DURATION_TOO_SMALL`, `INTX_FOK_ONLY_ALLOWED_ON_LIMIT_AND_MARKET_ORDERS`, `END_TIME_AFTER_CONTRACT_EXPIRATION`, `START_TIME_MUST_BE_SPECIFIED`, `ICEBERG_ORDERS_NOT_SUPPORTED`, `END_TIME_IS_IN_THE_PAST`, `GTD_ORDERS_MUST_HAVE_END_TIME`, `INVALID_ORDER_SIDE_FOR_ATTACHED_TPSL`, `ATTACHED_ORDERS_ONLY_ALLOWED_ON_MARKET_LIMIT`, `INVALID_ORDER_TYPE_FOR_ATTACHED`, `PRICE_NOT_ALLOWED_FOR_MARKET_ORDERS`, `REDUCE_ONLY_NOT_ALLOWED_ON_VENUE`, `DURATION_TOO_LARGE`, `REDUCE_ONLY_NOT_ALLOWED_ON_SPOT_PRODUCTS`, `REPLACE_NOT_SUPPORTED`, `INVALID_UNSUPPORTED_INSTRUMENT`, `POST_ONLY_NOT_ALLOWED_WITH_PEG`, `INVALID_PEG_OFFSET`, `INVALID_PEG_WIG_LEVEL`, `INVALID_PEG_VENUE_OPTIONS`, `PEG_INVALID_ORDER_TYPE`, `SINGLE_LEGGED_TPSL_NOT_ALLOWED`, `NBBO_NOT_PROVIDED`, `RFQ_INVALID_COMBO_LEGS` | new order | refused (a bug signal) |
| InvalidRequest | `PREVIEW_MISSING_COMMISSION_RATE`, `PREVIEW_INVALID_SIDE`, `PREVIEW_INVALID_ORDER_CONFIG`, `PREVIEW_INVALID_PRODUCT_ID`, `PREVIEW_INVALID_COMMISSION_CONFIGURATION`, `PREVIEW_INVALID_MARGIN_TYPE`, `PREVIEW_INVALID_LEVERAGE`, `PREVIEW_INVALID_END_TIME`, `PREVIEW_QUOTE_SIZE_NOT_ALLOWED_FOR_BRACKET`, `PREVIEW_BRACKET_ORDER_NOT_SUPPORTED`, `PREVIEW_FOK_DISABLED`, `PREVIEW_FOK_ONLY_ALLOWED_ON_LIMIT_ORDERS`, `PREVIEW_POST_ONLY_NOT_ALLOWED_WITH_FOK`, `PREVIEW_END_TIME_TOO_FAR_IN_FUTURE`, `PREVIEW_INVALID_BRACKET_ORDER_SIDE`, `PREVIEW_ATTACHED_ORDER_SIZE_MUST_BE_NIL`, `PREVIEW_INVALID_SETTLEMENT_CURRENCY`, `PREVIEW_DURATION_TOO_SMALL`, `PREVIEW_INTX_FOK_ONLY_ALLOWED_ON_LIMIT_AND_MARKET_ORDERS`, `PREVIEW_END_TIME_AFTER_CONTRACT_EXPIRATION`, `PREVIEW_START_TIME_MUST_BE_SPECIFIED`, `PREVIEW_ICEBERG_ORDERS_NOT_SUPPORTED`, `PREVIEW_END_TIME_IS_IN_THE_PAST`, `PREVIEW_GTD_ORDERS_MUST_HAVE_END_TIME`, `PREVIEW_INVALID_ORDER_SIDE_FOR_ATTACHED_TPSL`, `PREVIEW_ATTACHED_ORDERS_ONLY_ALLOWED_ON_MARKET_LIMIT`, `PREVIEW_INVALID_ORDER_TYPE_FOR_ATTACHED`, `PREVIEW_PRICE_NOT_ALLOWED_FOR_MARKET_ORDERS`, `PREVIEW_REDUCE_ONLY_NOT_ALLOWED_ON_VENUE`, `PREVIEW_NON_NUMERIC_ORDER_SIZE`, `PREVIEW_INVALID_INTX_CLIENT_ORDER_ID`, `PREVIEW_DURATION_TOO_LARGE`, `PREVIEW_REDUCE_ONLY_NOT_ALLOWED_ON_SPOT_PRODUCTS`, `PREVIEW_REPLACE_NOT_SUPPORTED`, `PREVIEW_POST_ONLY_NOT_ALLOWED_WITH_PEG`, `PREVIEW_INVALID_PEG_OFFSET`, `PREVIEW_INVALID_PEG_WIG_LEVEL`, `PREVIEW_INVALID_PEG_VENUE_OPTIONS`, `PREVIEW_PEG_INVALID_ORDER_TYPE`, `PREVIEW_SINGLE_LEGGED_TPSL_NOT_ALLOWED`, `PREVIEW_NBBO_NOT_PROVIDED`, `PREVIEW_INVALID_UNSUPPORTED_INSTRUMENT` | preview | refused (a bug signal) |
| InvalidRequest | `INVALID_CANCEL_PRODUCT_ID` | cancel | refused (a bug signal) |
| InvalidRequest | `ONLY_LIMIT_ORDER_EDITS_SUPPORTED`, `INVALID_ORIGINAL_SIZE`, `INVALID_ORIGINAL_PRICE`, `EDIT_REQUEST_EQUAL_TO_ORIGINAL_REQUEST`, `SIZE_IN_QUOTE_EDITS_NOT_ALLOWED`, `STRATEGY_NOT_SUPPORTED`, `FIELD_STOPPRICE_NOT_SUPPORTED`, `FIELD_DISPLAYSIZE_NOT_SUPPORTED`, `FIELD_ENDTIME_NOT_SUPPORTED`, `INVALID_ORIGINAL_TIME`, `INVALID_NEW_ENDTIME`, `ORDER_EDIT_INVALID_ATTACHED_ORDER_REQUEST`, `ORDER_EDIT_ORDER_TYPE_NOT_SUPPORTED`, `INVALID_ORIGINAL_STOP_PRICE`, `INVALID_ORIGINAL_STOP_LIMIT_PRICE`, `INVALID_ORIGINAL_ATTACHED_ORDER_CONFIG`, `CANNOT_CONVERT_SIZE_ASSET`, `CANNOT_CONVERT_DISPLAY_SIZE_ASSET`, `INVALID_EDITED_ATTACHED_ORDER_CONFIGURATION`, `CANNOT_ADD_ATTACHED_ORDER_TO_ORDER_TYPE`, `CANNOT_CHANGE_SIDE_OF_ORDER`, `CANNOT_CHANGE_ORDER_TYPE_OF_ORDER`, `CANNOT_CHANGE_PRODUCT_ID_OF_ORDER`, `PREV_CLIENT_ORDER_ID_REQUIRED_WHEN_CLIENT_ORDER_ID_PROVIDED`, `CLIENT_ORDER_ID_REQUIRED_WHEN_PREV_CLIENT_ORDER_ID_PROVIDED`, `CLIENT_ORDER_ID_MUST_BE_DIFFERENT_FROM_PREV_CLIENT_ORDER_ID`, `PREV_CLIENT_ORDER_ID_MUST_MATCH_CURRENT_CLIENT_ORDER_ID`, `INVALID_EDITED_ATTACHED_ORDER_CONFIGURATION_NON_MODIFIABLE_VALUE_EDIT`, `ATTACHED_CONFIGURATION_IS_EQUAL`, `CANNOT_ADD_LEG_TO_TPSL_ORDER`, `CANNOT_REMOVE_LEG_FROM_TPSL_ORDER`, `SINGLE_LEGGED_BRACKET_ORDER_NOT_ALLOWED`, `SINGLE_LEGGED_ATTACHED_ORDER_CONFIGURATION_NOT_ALLOWED`, `FIELD_PEG_OFFSET_NOT_SUPPORTED`, `INVALID_EDITED_PEG_WIG_LEVEL`, `INVALID_EDITED_PEG_OFFSET`, `FIELD_PEG_WIG_LEVEL_NOT_SUPPORTED` | edit | refused (a bug signal) |

### Rate limits for orders

No current source for Advanced Trade REST (see [Rate limits](#rate-limits)): no order bucket is
declared. A 429 refuses the order with its wait (`Retry-After` when sent) and holds new orders
until then. An order row's `reject_reason` can also be `RATE_LIMIT_EXCEEDED`. The SDK reads
`x-ratelimit-limit`, `x-ratelimit-remaining` and `x-ratelimit-reset` headers (SDK
`constants.py`), whose units are doc silent: not read. Sockets: "WebSocket connections and
unauthenticated messages are each limited to 8 per second per IP" ([WebSocket rate
limits](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-rate-limits)).

### Stream

`wss://advanced-trade-ws-user.coinbase.com` ("User Order Data", [WebSocket
overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-overview)).
No login frame: each subscribe frame carries its own JWT in `jwt`, "a different JWT for each
websocket message sent", built like the REST one without `uri` ("WebSocket JWTs ... are not
built with a request method or request path", [WebSocket
authentication](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-authentication);
SDK `build_ws_jwt`). One channel per subscribe frame, sent within 5 seconds of connecting:

1. `{"type":"subscribe","channel":"user","jwt":...}`, `product_ids` omitted ("omit it for all
   products").
2. `{"type":"subscribe","channel":"heartbeats","jwt":...}`: "Real-time server pings (every
   second)"; it keeps the socket open ("Most channels close within 60-90 seconds when no updates
   arrive. Subscribe to `heartbeats` to keep the connection open"). Sent second, so that no
   heartbeat can be taken for the `user` subscribe's answer.

No client heartbeat is documented: OTW sends none, and drops the socket after 20 s without a
frame (heartbeats come every second), then connects again and reads the book (`Resync`). The
answer to a subscribe is doc silent ("You receive a `subscriptions` message as a response to an
`unsubscribe` message"): the `user` subscribe is answered by its `snapshot` or by a
`subscriptions` frame listing `user`, the `heartbeats` subscribe by a heartbeat; a frame `type:
error` is a refusal (its shape doc silent). Frames carry `sequence_num`, and the WebSocket overview
reads a jump of more than one as a dropped message: on such a jump the book is read again
(`Resync`), and a lower number (out of order) is passed over. The count is per connection, as
the channel pages state it ("Per-connection message sequence number"), across the `user` and
`heartbeats` channels of the one socket, counted from the first frame of each connection. The
overview says instead "increasing integer values for each product" (doc conflict; the channel
pages are followed), and no page says where the count starts.

A `user` frame's `update` event lists orders whose remaining size (`leaves_quantity`) is "in the
same currency the order was placed in (quote or base)" without saying which, so each order it
names is read again by its venue id (Get Order) rather than from the frame. The `snapshot`
event (the open orders at connect, "batched by 50") is the book the account already read, and
is not read again. The channel carries no fills and no execution ids: fills are read by REST
(`entry_id`). `positions` is futures only ("beta and empty by default").

### Disconnect behaviour

No cancel on disconnect is documented. A dropped or silent socket is opened again with backoff
and the account read again; between two connections the account is reconciled by polling.

### Dry-run

`POST /orders/preview` ([Preview Orders](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/preview-orders),
scope `view`): the order's own body without `client_order_id`. `errs[]` ("potential failure
reasons were this order to be submitted") non-empty is a refusal under the same table;
otherwise the answer is summed up as `order total, commission, best bid, best ask` and its
warnings (`UNKNOWN` "can therefore be seen as no warning", FAQ). The request schema lists
`commission_rate` as required but never defines it (doc defect): it is not sent, and the
sandbox answers without it. On an account never tested against the venue, the first order of
each session goes through it first (E2D).

### Quirks

- `success: true` with `failure_reason: UNKNOWN_FAILURE_REASON` is an accepted order (FAQ); the
  sandbox's cancel answers `success: true` with `UNKNOWN_CANCEL_FAILURE_REASON` the same way.
- A duplicate `client_order_id` "will not be created and the order corresponding with that ID
  will be returned instead", while `DUPLICATE_CLIENT_ORDER_ID` is also a failure reason: it is
  unknown (looked up), never resent.
- The four cancel reasons titled "Internal error when placing cancel request"
  (`UNKNOWN_CANCEL_FAILURE_REASON`, `INVALID_CANCEL_REQUEST`, `UNKNOWN_CANCEL_ORDER`,
  `COMMANDER_REJECTED_CANCEL_ORDER`) and `DUPLICATE_CANCEL_REQUEST` are unknown: the order is
  read again, and a cancel of an order that ended is no error. Cancel has no not-found reason.
- The sandbox lists an attached TP/SL order with `order_type: TAKE_PROFIT_STOP_LOSS`, a word the
  `OrderType` enum does not list: type words are kept verbatim. Its `trigger_bracket_gtc` states
  the stop as `stop_trigger_price`, read as its stop; a working entry's
  `attached_order_configuration` is said in its type words (`LIMIT with take profit 99990 and
  stop loss 99800 attached`).
- List Orders and List Fills answer `proof_token_required` ("a valid proof token is required to
  access this data (EU SCA compliance)"): OTW sends no `proof_token`, and such a list is an
  error naming it, never read as empty.
- Order ids land in URL paths (Get Order): only letters, digits and `-` are accepted.
- The account read and the order path read sizes, fees and adjusted fills the same way; both
  name a fill's fee currency as the product's quote currency (the order path through the
  trader, from the product list).

### Left out

- **US futures (CFM)**: "CFTC-regulated futures for eligible US clients"
  ([Overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/overview)); whether
  `base_size` counts contracts is doc silent; `leverage` rides on each order ("default is 1.0")
  with no account value to read, and OTW never chooses a leverage.
- **Global Derivatives**: another API (JSON-RPC on `drb.coinbase.com`,
  [REST API](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/rest-api)).
- **INTX perpetuals**: "INTX perpetuals trading through the Advanced Trade API ended on October
  1, 2026" ([Perpetuals guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/perpetual)).
- **Attached take profit and stop loss** (`attached_order_configuration`, `trigger_bracket_gtc`):
  both legs are required (`SINGLE_LEGGED_TPSL_NOT_ALLOWED`), the stop leg is a stop-limit "5%
  higher for BUYS and 5% lower for SELLS", when the attached order appears and its states while
  waiting are doc silent, and a change goes through the entry's order id, which the shared
  exit change (keyed by position) cannot express.
- **Stop-market, trailing, MIT, TWAP, scaled, close position, equities, predictions**: not
  offered on spot, or not orders OTW sends.
- **Key permission check** (`GET /key_permissions`, `can_trade`): no hook reads it before an
  order; a key without the `trade` scope is refused by the venue, in words doc silent.

### Doc gaps

1. Client id charset, length and uniqueness window; the "returned instead" sentence against
   `DUPLICATE_CLIENT_ORDER_ID`.
2. Lookup by client id: only a deprecated query. The sort order of List Orders ("Default is to
   sort by creation time", direction unsaid), its largest `limit`, and whether an API order is
   `RETAIL_ADVANCED` (the sandbox's are).
3. Maximum ids per batch cancel; no not-found cancel reason.
4. Advanced Trade REST rate limits (page removed); `x-ratelimit-*` units (SDK only).
5. Product `status` words; `auction_mode`; `quote_min_size` worded as a representable size,
   read as the order value bound.
6. Fill fee sign and currency; fill `size` unit when `size_in_quote`; how a `REVERSAL`,
   `CORRECTION` or `SYNTHETIC` row undoes or replaces a fill.
7. Fills: largest `limit`, window, history depth, end inclusivity; whether a page that is not
   full can still carry a `cursor` (a next page is asked only after a full one).
8. Edit: total or remaining `size`; how an edit treats the client id; whether it keeps queue
   priority.
9. Whether `order_status=OPEN` lists a stop-limit waiting for its trigger; whether a GTD order
   ends `EXPIRED` or `CANCELLED` (the stream's glossary says `EXPIRED` is a TWAP's).
10. HTTP 404 on Get Order (read as not found) and the gRPC `code` values.
11. The answer to a socket subscribe and the error frame's shape; the user channel's host (the
    AsyncAPI names the market data host, the endpoints page the user host).
12. The product id an order on a `-USDC` alias is reported under.
13. Key type: the API key page says ECDSA is required, the JWT page and the SDK say Ed25519
    works; the signer takes both (see [Auth](#auth)).

### What stays unproven

Everything above was read, not run: no Coinbase account placed, changed or cancelled an
order from OTW. The public product list and the venue's refusal of a key it never issued are
checked against the live venue (`OTW_NET_TEST=1`; 2026-11-12: 925 spot products, every one with
sound rules, 44 not plainly trading; a create order signed with a key Coinbase never issued, and
one not signed, both answered HTTP 401 with the plain text `Unauthorized`, read as Auth); every
other answer is the static sandbox's
or built on its shape. The first live orders settle the doc gaps above.

### Fixtures

`fixtures/trade/`: the sandbox's answers (create, insufficient funds, cancel, cancel failure,
edit, edit failure, preview, preview insufficient funds, list orders, open orders, get order,
an invalid id), the FAQ's accepted-with-`UNKNOWN_FAILURE_REASON` example verbatim, the live
public product list for `BTC-USD` and `BTC-USDC`, and two **synthetic** socket frames (a `user`
update built on the AsyncAPI field list, which has no example; a heartbeat with the doc's
placeholders filled in). Sources per file in `trade_tests.rs`.
