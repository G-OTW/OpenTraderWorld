# Deribit (perpetuals, futures, options, spot)

Adapter version: 0.1.0. Provider API version: v2 (JSON-RPC over HTTP and a socket). Maturity:
M2 (mock catalog green; history, the instrument list, quotes, the board, the clock and the
socket checked against the live venue on 2026-10-19; account parity on mock only, no key
lent). Orders on futures and perpetuals at E2D, from the documentation alone ("Trading").

One OTW provider, `deribit`, one adapter, `Deribit::new()`, venue `DERIBIT`. Every kind
answers on the same endpoints and its name says which kind it is, so a connector has no
market or book setting:

| Name | Kind |
| --- | --- |
| `BTC-PERPETUAL`, `ETH-PERPETUAL` | inverse perpetual: one contract is 10 (BTC) or 1 (ETH) USD, settled in the coin |
| `BTC-27DEC26` | inverse dated future |
| `BTC_USDC-PERPETUAL`, `SOL_USDC-27DEC26` | linear perpetual or future, settled in USDC, amounts in the coin |
| `BTC-27DEC26-100000-C` | option settled in the coin, premium in the coin |
| `SOL_USDC-27DEC26-150-P`, `XRP_USDC-25DEC26-1d75-C` | option settled in USDC (`d` marks a fraction of the strike) |
| `BTC_USDT`, `ETH_BTC` | spot pair |

Anything else (`BTC-FS-20OCT26_PERP`, `BTC-PBUT-...`) is a combo, a spread of listed legs,
and is left out of the list. Names are upper case except the strike's `d`; a name typed in
lower case is put back in Deribit's case (the venue refuses `btc-perpetual`, measured).

Sources:
- API reference: https://docs.deribit.com/ (per-method pages and the OpenAPI file
  `https://docs.deribit.com/specifications/deribit_openapi.json`, version 2.1.1, fetched
  2026-10-19).
- Articles: Authentication, Rate Limits, Errors, Using the currency Parameter, Accessing
  Historical Trades and Orders, Connection Management (docs.deribit.com/articles/...).
- Socket channel: https://docs.deribit.com/subscriptions/market-data/charttradesinstrument_nameresolution
- ccxt `python/ccxt/deribit.py` (master, 2026-10-19): the `deri-hmac-sha256` signature
  (`sign`), fill and position parsing, the error table.
- Measured answers of the public endpoints and the socket, 2026-10-19 (recorded in
  `tests.rs`).

## Endpoints

Everything below `https://www.deribit.com/api/v2`. An answer is `{"jsonrpc":"2.0","result":
...}`; a refusal `{"error":{"code","message","data"}}` with HTTP 400.

| Endpoint id | Call | Canonical |
| --- | --- | --- |
| `chart` | `GET public/get_tradingview_chart_data?instrument_name&start_timestamp&end_timestamp&resolution` | `RequestHistory{Candles}` |
| `instrument` | `GET public/get_instrument?instrument_name` | a spot pair's routing, before its candles |
| `instruments` | `GET public/get_instruments?currency=any&expired=false` | `LoadInstruments` |
| `ticker` | `GET public/ticker?instrument_name` | `RequestSnapshot{Quote}` |
| `bookSummary` | `GET public/get_book_summary_by_currency?currency&kind=spot`, for USDC, USDT, BTC, ETH in turn | `RequestBoard` |
| `time` | `GET public/get_time` | `SyncTime` |
| `accountSummaries` | `GET private/get_account_summaries[?extended=true]` (signed) | `Summary`, `Balances` |
| `positions` | `GET private/get_positions?currency=any` (signed) | `Positions` |
| `openOrders` | `GET private/get_open_orders` (signed) | `OpenOrders` |
| `userTrades` | `GET private/get_user_trades_by_currency_and_time?currency&kind&start_timestamp&end_timestamp&count=1000&sorting=asc&historical` (signed) | `Fills` |

Socket: `wss://www.deribit.com/ws/api/v2`.

- `{"jsonrpc":"2.0","id","method":"public/subscribe","params":{"channels":[...]}}` with
  `chart.trades.<instrument>.<resolution>`, the panes joining together in one frame. The
  answer echoes the id and lists the channels taken, in any order; a channel missing from it
  was refused. An unknown instrument, a resolution the socket does not stream and a spot pair
  matched on Coinbase Exchange are all answered that way, never with an error (measured). A
  refused frame (`error` with the id) fails all its panes.
- Updates: `{"method":"subscription","params":{"channel","data":{"tick","open","high","low",
  "close","volume","cost"}}}`, `tick` the bar's open in ms. Several per period; a period with
  no trade gets a filling sample.
- `public/set_heartbeat {"interval":30}` goes out with the first subscription; the venue then
  sends `{"method":"heartbeat","params":{"type":"test_request"}}`, answered with
  `public/test` at the next keepalive (the venue closes a socket that does not answer).
  `public/unsubscribe` with the channel drops one pane.
- A frame is 32 kB at most (docs; a frame of 1000 channels closed the socket, measured):
  subscriptions are split in frames of 16 kB of channel names. One socket took 4000 channels
  without a refusal (measured), so `max_subs_per_connection` is not declared.
  `public/subscribe` costs 3000 credits of a 30,000 pool (about 3.3 frames a second).

## Field mapping and rules

- Resolutions: `1 3 5 10 15 30 60 120 1D` are served as `1m 3m 5m 10m 15m 30m 1h 2h 1d`, on
  the archive and the socket. `180`, `360` and `720` exist but open on the 08:00 UTC grid
  (`02:00`, `05:00`..., measured), off the epoch: they are not declared, the engine folds 3h,
  4h, 6h and 12h from hours. Unsupported tokens (`7`, `1W`) are refused by the venue
  (`-32602`, measured).
- Daily bars open at 08:00 UTC, Deribit's settlement time (measured from the listing on
  2018-08-14 to today). A week is folded from days and stamped with its first day's open
  (`aggregate_stamp = "first_bar"`).
- Archive answer: columns `ticks` (open ms), `open`, `high`, `low`, `close`, `volume`, `cost`,
  `status` (`ok` or `no_data`). `volume` is the base coin on every kind, inverse contracts
  included (BTC-PERPETUAL: 4.67 BTC for 387,020 USD of `cost`, measured); `cost` is the
  quote: USD on an inverse contract, USDC on a linear one, the coin on a coin-settled option
  (premium × amount). They map to `volume` and `quote_volume`; no contract size is involved,
  so a pane needs no instrument list (`live_requires_instruments` is false). A minute with
  no trade is a filling bar at the last close with zero volume (measured, as the venue
  charts it).
- Paging (measured): `start_timestamp` and `end_timestamp` are bar opens, both included, and
  the bar holding an unaligned start comes back too; a window wider than 5001 bars answers
  its newest 5001. Pages ask 5000 bars and walk backwards from the range's end (a dated
  contract's expiry when earlier); the walk stops at the listing, where the window's first
  bar opens after its start, or at a `no_data` page once bars were seen. A `no_data` page
  before any bar walks on (a contract delisted inside the range). No lookback limit: 1m
  bars of 2019 answered (measured). A range wholly after a dated contract's expiry (read
  from its name, 08:00 UTC on its date) is refused as NotFound with the date. Expired
  contracts keep their candles (BTC-27DEC24, measured).
- Closed flags: neither the archive nor the socket carries one. Polled every second through
  ten minute boundaries over five instruments, a closed minute was rewritten up to 1.5 s
  after its close (16 rewrites, measured): an archive bar is final 10 s after its close.
  Socket candles go out as forming (`candles_final_flag = false`).
- Spot pairs matched on Coinbase Exchange (`is_cbe_routed`, 45 of 57 on 2026-10-19,
  BTC_USDC and ETH_USDC among them): Deribit sees its own users' share of their trades only
  (BTC_USDC minutes of 0.0003 BTC against 3000 BTC a day, measured), its docs say the
  candle channel is not supported for them, and the socket refuses them (measured). Their
  history is refused as Unsupported, naming the Coinbase product; a spot pair the session
  does not know yet is looked up once (`get_instrument`) before its candles. They stay in
  the list as inactive, noted `matched on Coinbase Exchange`; their quotes and board prices
  are served.
- List: `get_instruments?currency=any` answers every kind, options included (5238 rows on
  2026-10-19: 57 spot, 144 perpetuals, 67 futures, 4970 options; combos left out). Tick =
  `tick_size`; lot and minimum = `min_trade_amount` (USD on an inverse contract, the coin
  elsewhere); multiplier 1 (amounts are already in the coin, or USD on an inverse
  contract); `inverse` set on a `reversed` future, with a note naming its contract size;
  expiry, strike and right on dated contracts and options; settlement currency as given;
  active when `is_active`, `state` is `open` and the pair is not routed to Coinbase.
- Quotes: `ticker` best bid/ask, stamped with its `timestamp`; sizes are carried in the coin
  and left out on an inverse contract (they are USD).
- Board: the spot summaries under USDC, USDT, BTC and ETH, which together hold every pair
  (a pair sits under its base and its quote, measured); `last`, else `mark_price`.

## Account

- Summary: `get_account_summaries?extended=true`: account type (`main`, `subaccount`), the
  currencies with a balance, total equity in USD when the account reports it (cross
  collateral), each currency's equity otherwise.
- Balances: one per currency with a positive `balance`; available = `available_funds`, held
  between zero and the balance.
- Every account call goes to the host of the account's environment: `env` `live` (or empty)
  reads `https://www.deribit.com`, `env` `test` reads `https://test.deribit.com` ("Test and
  production require separate accounts and API keys",
  [index](https://docs.deribit.com/index); both hosts in the
  [JSON-RPC overview](https://docs.deribit.com/articles/json-rpc-overview), "HTTP REST
  Requests"). Any other value is refused when the account is saved.
- Positions: `get_positions?currency=any`, every kind in one call. `direction` `buy` long,
  `sell` short (negative quantity), `zero` skipped. Linear futures count `size_currency`
  (the coin), options `size` (the coin). Inverse futures stay in USD (`size`), the unit the
  venue sizes them and their orders in, so the panel, a change and a close all count the same
  unit; the label names the coin they are worth (`size_currency`, "Only for futures, position
  size in base currency", [get_positions](https://docs.deribit.com/api-reference/account-management/private-get_positions)).
  Their unrealised profit (in the coin) is not carried, as on Bybit.
- Open orders: `get_open_orders`, every kind, in the venue's unit (USD on an inverse
  contract); a `price` of `"market_price"` (an untriggered market stop) is no price.
- Fills: per (currency, kind) group, not per instrument (`fills_need_symbols = false`). With
  no instrument named the walk reads BTC, ETH and USDC options, USDC futures, and spot under
  USDC, USDT, BTC and ETH. Named instruments narrow it to their groups. Each group is read
  `historical=true` over the period (kept for good, indexed after a short delay) and then
  `historical=false` (the last 24 hours, available at once) where the period reaches them;
  1000 fills a page, oldest first, the next page starting at the page's last millisecond. A
  fill seen twice is handed out once (ids kept for the walk). More than 1000 fills in one
  millisecond cannot be paged and is refused. Fill rows: `amount` (the coin), `price`,
  `fee` in `fee_currency` ("User's fee in units of the specified `fee_currency`": its sign
  is not documented, so it is carried as stated), `direction` the account's side; the quote
  and class are read from the name.
- **Inverse fills are not read**: their profit is quantity × (1/entry − 1/exit) in the coin,
  which the journal does not compute. The default walk never asks for them (BTC and ETH
  `future`), a named inverse contract is refused by name, and an inverse row that comes back
  anyway is logged and left out. Coin-settled options are read: amount and premium are both
  in the coin.
- A spot group refused with 11060 (Coinbase-routed spot, which Deribit documents for public
  trade calls; unverified on the private one without a key) is passed over and logged rather
  than failing the pull.
- History window: unlimited (historical records persist indefinitely), so the broker
  declares `history_days = 0`. Calls for a pull: per group,
  `ceil(fills / 1000)` pages (one at least) in the historical store, plus as many in the
  recent store when the period reaches the last 24 hours: 8 to 16 calls for a quiet
  account over any period with no instrument named, independent of the number of
  instruments.

## Auth

- Stateless per-call signature, no token: lowercase hex HMAC-SHA256, keyed by the Client
  Secret, of `timestamp + "\n" + nonce + "\n" + METHOD + "\n" + URI + "\n" + body + "\n"`,
  the URI being the path and query string as sent, the body empty on a GET. Sent as
  `Authorization: deri-hmac-sha256 id=<Client Id>,ts=<ms>,sig=<hex>,nonce=<nonce>`. The nonce
  is 16 random hex characters per call. Checked against a reference computed with Python's
  `hmac` (Deribit's own `calcSignature`).
- The timestamp is valid for 60 s: `SyncTime` reads the venue's clock (`public/get_time`)
  first. A stale one is refused `13004 invalid_credentials` with `{"invalid":"timestamp"}`
  (measured), reported as a clock problem.
- Secrets: `api_key` = Client Id, `api_secret` = Client Secret. Scopes needed:
  `account:read`, `trade:read` to read; `trade:read_write` to send orders ("**Scope:**
  `trade:read_write`", [private/buy](https://docs.deribit.com/api-reference/trading/private-buy)).
  A read-only key still reads.
- REST calls never use `public/auth`. The private socket logs in with it, `grant_type`
  `client_signature` (see "Trading", "Stream").

## Rate limits

Credit-based, per sub-account (docs). Non-matching-engine calls cost 500 credits of a 50,000
pool refilled at 10,000 a second: 20 calls a second sustained, bursts of 100; declared as
20 per second. `get_instruments` costs 10,000 of a 500,000 pool (one a second, bursts of
50); declared as one a second. `public/subscribe` costs 3000 of 30,000 (about 3.3 a
second). Running out answers `10028 too_many_requests` and may close a socket. 32
connections per IP at most (a 33rd answers HTTP 429). The web platform spends the same
credits.

## Error codes

| Code | Meaning | Canonical |
| --- | --- | --- |
| 10028, 10066, 10047, HTTP 429 | rate limit, too many concurrent requests, connection cap | RateLimited |
| 10000, 13004, 13007, 13009, 12003, 12998 | missing or bad signature, stale timestamp, bad token | Auth (a stale timestamp names the clock) |
| 13021, 11042, 13403, 13012, 13031, 13002, 12005, 11098 | forbidden, scope, subaccount, verification, blocked, banned country | Permission |
| 10020, 13020, 13019, -32602 on `instrument_name` | unknown or closed instrument (measured: `instrument not found`, `wrong format`) | NotFound, naming the name and Deribit's spelling |
| 11060 | Coinbase-routed spot | Unsupported (a spot fill group is passed over) |
| 11051 | system maintenance | ProviderDown |
| 10001, 10040, 10041, 11094, 13025, 13028, 13503, 13888, 11056 | general failure, retry, settlement in progress, internal error, switched off, unavailable, timed out | Transient |
| anything else (-32602 on another parameter, -32601, -32700, -32000, 11029...) | | InvalidRequest |

A body that is not a JSON-RPC refusal falls back to the HTTP status.

## Not covered

- Inverse fills (positions and orders are read).
- Combos (`future_combo`, `option_combo`): left out of the list; their legs are read.
- Candles of spot pairs matched on Coinbase Exchange.
- 3h, 6h and 12h as Deribit draws them (08:00 UTC grid): the engine folds epoch-aligned ones.
- Trades, order books, funding, index and volatility charts.
- Subaccounts other than the key's own, block trades as such.
- Market data (candles, quotes, the list shown on the chart) is read from the live host
  only; an account set to `env` `test` reads and trades on the test host.

## Trading

Execution maturity: E2D (written from the documentation alone; no account, no key, no live
order; the signer has never run against a real key).        Sandbox: testnet
(`https://test.deribit.com`, `wss://test.deribit.com`, its own accounts and keys: account
setting `env` = `test`).

Sources (read 2026-11-12; every page is `https://docs.deribit.com/<path>`, also served as
`<path>.md`; OpenAPI `specifications/deribit_openapi.json` version 2.1.1, sha256
`d06d82d9a42dd9f2...`, last changelog entry "Release 10.11.2026"):
- Orders: `api-reference/trading/private-buy`, `private-sell`, `private-edit`,
  `private-cancel`, `private-cancel_all_by_instrument`, `private-get_open_orders`,
  `private-get_open_orders_by_instrument`, `private-get_order_state`,
  `private-get_order_state_by_label`, `private-get_order_history_by_instrument`,
  `private-get_trigger_order_history`, `private-get_user_trades_by_instrument_and_time`
  (all under `api-reference/trading/`).
- Positions: `api-reference/account-management/private-get_position`, `private-get_positions`.
- Rules: `api-reference/market-data/public-get_instruments`, `public-get_instrument`,
  `public-ticker`.
- Stream: `api-reference/authentication/public-auth`,
  `api-reference/subscription-management/private-subscribe`,
  `api-reference/session-management/public-set_heartbeat`, `api-reference/supporting/public-test`,
  `subscriptions/user/userchangeskindcurrencyinterval`.
- Articles: `articles/json-rpc-overview` (RPC), `articles/errors` (ERR),
  `articles/authentication` (AUTH), `articles/rate-limits` (RL),
  `articles/connection-management-best-practices` (CM),
  `articles/order-management-best-practices` (OM),
  `articles/accessing-historical-trades-orders` (HIST), `articles/currency-parameter` (CUR),
  `changelogs/jsonrpc` (CL).

Books, read from the instrument's name (the `instrument_type` of the list confirms it):

| Book | Example | Size unit | Notes |
| --- | --- | --- | --- |
| Linear (USDC) perpetuals and futures | `BTC_USDC-PERPETUAL`, `SOL_USDC-27DEC26` | base coin | "For options and linear futures it is the underlying base currency coin" (private/buy, `amount`) |
| Inverse perpetuals and futures | `BTC-PERPETUAL`, `ETH-PERPETUAL`, `BTC-27DEC26` | USD | "For perpetual and inverse futures the amount is in USD units" (private/buy); `contract_size` "For inverse (reversed) futures and perpetuals this is USD" (public/get_instrument) |
| Options, combos, spot | `BTC-27DEC26-100000-C`, `BTC_USDT` | | refused by name before I/O (options asked out; spot is "mostly routed" to Coinbase, `articles/spot-trading-venues`) |

An inverse book is quote-sized: the user gives the size in USD (rounded DOWN to the step,
`min_trade_amount`, 10 USD on BTC-PERPETUAL) or in the coin (converted at the last price of
`public/ticker`, then rounded down). A size that rounds to nothing is refused, naming the
smallest size and its coin worth. Positions, orders and fills on it are in USD on the read path
and the order path alike; the position's label names its coin worth (`size_currency`).

All calls are JSON-RPC 2.0 over HTTP (RPC, "HTTP REST Requests": "Both GET and POST are
supported"): orders as `POST /api/v2/private/<method>` with the JSON-RPC body (the doc's own
cURL examples), one `Content-Type: application/json`; reads as `GET` with the query string.
Numbers go out as JSON numbers written from exact decimals. The JSON-RPC id of a place is
the order's label.

### Order types and flags

| Canonical | Native | Notes |
| --- | --- | --- |
| Market | `type=market` | sized as above |
| Limit | `type=limit`, `price` | |
| Stop | `type=stop_market`, `trigger_price`, `trigger` | |
| StopLimit | `type=stop_limit`, `price`, `trigger_price`, `trigger` | |
| Mit | `type=take_market`, `trigger_price`, `trigger` | |
| Lit | `type=take_limit`, `price`, `trigger_price`, `trigger` | |
| trigger Last / Mark / Index | `trigger=last_price` / `mark_price` / `index_price` | "Required for Stop-Loss, Take-Profit and Trailing trigger orders" (private/buy): the venue has no default, so a stop with no trigger kind is refused before I/O with the fix |
| reduce-only | `reduce_only=true` | |
| post-only | `post_only=true` with `reject_post_only=true` | alone, "the price will be changed to be just below the spread" (private/buy; OM: "place your order at $9,999.50"); with the reject flag "the order is put to the order book unmodified or the request is rejected". "Only valid for non-market, non-advanced orders with time_in_force=good_til_cancelled or good_til_day": refused otherwise before I/O |
| amend | `private/edit` (same `order_id`) | always resends `amount` (the new total: OM, "the filled 100 remains") and `reduce_only` (the edit page does not say an omitted one is kept); on a post-only order both post-only flags (OM: "In standard post-only mode, the system will adjust the price on edit"). A time-in-force change, a trigger price on a limit order ("you cannot change a limit order into a stop order via edit", OM) and a limit price on a market-type order are refused before I/O |
| cancel | `private/cancel` (`order_id`) | |
| cancel all | `private/cancel_all_by_instrument` (`instrument_name`, `type=all`), per instrument with working orders | covers trigger orders and orders placed outside OTW, options included (the account's whole working list, `get_open_orders`) |
| close position | reduce-only market order for the size read from `private/get_position` | `private/close_position` takes no label (CP): not used |

Left out: `trailing_stop` (absolute offset only), `market_limit` (no JSON-RPC definition; FIX
only), iceberg (`display_amount`), linked orders (OTO, OCO, OTOCO: the secondary orders' ids
and lifecycle are not documented, and OM's example contradicts itself), `valid_until`,
cancel on disconnect (WebSocket only, off by default, CM), the venue's dry run (none:
`private/get_margins` checks margin only), options, combos, spot.

One position per instrument: `direction` `buy`, `sell` or `zero` (get_position); the docs have
no hedge mode (`position_mode = "net"`). Leverage cannot be set by any method: the position's
`leverage` ("Current available leverage for a future position") is shown, read only; an order
carries none. Margin: one account-wide `margin_model`, nothing per order.

### Time in force

| Canonical | Native | Notes |
| --- | --- | --- |
| GTC | `good_til_cancelled` | the default; not sent on a market order |
| Day | `good_til_day` | "until the end of the session at 8 UTC ... maximum lifetime up to 24 hrs" (CL); refused on a market order |
| IOC | `immediate_or_cancel` | |
| FOK | `fill_or_kill` | |

### Client order id

`label`: "user defined label for the order (maximum 64 characters)" (private/buy). The charset
is not documented: OTW's keys keep to `[A-Za-z0-9_-]`, 1 to 64. Labels are not unique at the
venue ("you might label all orders from a particular strategy", OM; `13032
non_unique_order_label`): OTW's keys are. A trigger order and the order it spawns share the
label ("the same `label` was carried over between them", get_order_state_by_label).

Lookup by label (spec 6.3), asked in turn until one answers it:
1. `private/get_open_orders_by_instrument` (`type=all`): the working orders, trigger orders
   included.
2. `private/get_order_state_by_label` (`currency`, `label`): "only returns orders that closed
   within the last 30 minutes". The currency: the coin of an inverse contract; for a linear
   one the doc does not say whether labels are filed under USDC or the base coin, so both
   are asked (an empty answer is no error, CUR).
3. `private/get_order_history_by_instrument` (`historical=true`, `include_unfilled=true`,
   `include_old=true`, 1000 a page, two pages by `offset`), searched by label.

Lookup by id (a cancel reconciled, a change, a streamed trigger): `private/get_order_state`
(a working order, "status (open, filled, cancelled)", or one that "closed within the last 30
minutes"), the instrument's working orders, `private/get_trigger_order_history` (`currency`,
`instrument_name`), then the history (`historical=true`).

**A trigger order fires as a new order.** "Id of the trigger order that created the order"
(`trigger_order_id`, order schema); the trigger history's `trigger:order` entry names both
(`trigger_order_id: SLIS-103`, `order_id: '671473'`). A list that holds both is merged: the
spawned order first, the trigger naming it as its successor, which the trader follows for the
state, the fills and the cancel. Read by id, a fired trigger whose successor the answer does
not hold is passed over, so the trigger history is read next. A fired trigger read alone stays
`triggered`, unknown (it alerts) rather than guessed.

### Order status

| Native (`order_state`) | Canonical |
| --- | --- |
| `open` | New; PartiallyFilled once `filled_amount` is above zero |
| `untriggered` | New, trigger pending |
| `filled` | Filled |
| `cancelled` | Cancelled (with any fill it had: "partially executed and the remaining quantity was removed", OM) |
| `rejected` | Rejected |
| `triggered` | the trigger fired as a new order: followed to it; alone, unknown |
| anything else (`archive` of trade rows, `new` of the trigger history) | unknown, alerts |

### Instrument trading rules

`public/get_instruments?currency=<coin, or USDC for a linear contract>&kind=future` ("Every
instrument whose name involves the currency", CUR), `currency=any` for the whole list;
`public/get_instrument` for one listed in another state (expired, archived). Held a day.

- tick = `tick_size`. `tick_size_steps` (a coarser tick above a price) is not checked before
  I/O: a price off the coarser tick is refused by the venue (`10043 price_wrong_tick`,
  "rejected instead of being truncated", CL).
- step and minimum = `min_trade_amount` ("Minimum amount for trading and the quantity
  increment (step size) for order amounts"), in the book's unit (USD on an inverse contract).
- status: `state` `open` trades; any other word (`settlement`, `delivered`, `inactive`,
  `locked`, `halted`, `archivized`) refuses new orders by name ("only `state` `open` accepts
  new orders", public/get_instrument).
- No maximum size, no minimum notional in the list (`10018`, `10032` exist: the venue checks
  alone).

### Errors

Only the complete table of ERR ("Complete RPC Error Codes Reference") is used; its "Common
Error Codes" table contradicts it (10001, 10002, 10004, 11000 to 11009, 13009 with other
words), so 11000 to 11009 stay unmapped. `10050 lock_time_exceeded` is from OM. A refusal
says "Deribit refused it: <message> (code <code>)"; a code not listed says "Deribit answered
code X (message), which OpenTraderWorld does not map" and is unknown. `-32602 Invalid params`
naming `instrument_name` is the venue not listing the instrument: Rejected(not found). A
timeout, a closed connection, a 5xx without a refusal and an unreadable body are Unknown. The
read path (`errors.rs`) maps 10001, 10040, 11094, 13028 and 13888 as transient; here they are
Unknown, looked up, never resent.

| Code | Meaning | Canonical | Action |
| --- | --- | --- | --- |
| `10001`, `10040`, `10050`, `11031`, `11094`, `13008`, `13028`, `13777`, `13888` | error, retry, lock_time_exceeded, other_error, internal_server_error, request_failed, temporarily_unavailable, replaced, timed_out | Unknown | looked up by label, never sent again |
| `10009`, `10039` | not_enough_funds, not_enough_funds_in_currency | Rejected(funds) | refused, nothing placed |
| `10002`, `10021`, `10026`, `10027`, `10032`, `10043`, `10044` | qty_too_low, invalid_amount, price_precision_exceeded, non_integer_contract_amount, whole_cost_too_low, price_wrong_tick, trigger_price_wrong_tick | Rejected(venue_rule) | refused; the instrument rules are read again |
| `10005`, `10006`, `10007`, `10011`, `10034`, `10035`, `11036` | price_too_low, price_too_low4idx, price_too_high, price_not_allowed, trigger_price_too_high, trigger_price_too_low, invalid_trigger_price | Rejected(price) | refused, nothing placed |
| `11021`, `11054` | post_only_price_modification_not_possible, post_only_reject | Rejected(post only) | refused, nothing placed |
| `10012`, `10019`, `10041`, `11037`, `11046`, `11051`, `13019` | book_closed, locked_by_admin, settlement_in_progress, outdated_instrument_for_IV_order, outdated_instrument, system_maintenance, orderbook_closed | Rejected(market closed) | refused, nothing placed |
| `10004`, `10020`, `10029`, `13020` | order_not_found, invalid_or_unsupported_instrument, not_owner_of_order, not_found | Rejected(not found) | a cancel or a lookup reconciles the order; a lookup moves to its next call |
| `10010`, `11008`, `11044` | already_closed, already_filled, not_open_order | Rejected(final) | a cancel or a change reconciles the order and answers its final state |
| `10003`, `10013`, `10014`, `10015`, `10016`, `10017`, `10018`, `10037`, `10038`, `10045`, `10046`, `10072`, `11013`, `11022`, `11030`, `11035`, `11056`, `13025`, `13030`, `13035`, `13036`, `13043`, `13503`, `13666`, `13793`, `13901`, `13905`, `13906`, `13907`, `13910`, `13911`, `13917`, `13918` | order_overlap, pme_max_total_open_orders, pme_max_future_open_orders, pme_max_option_open_orders, pme_max_future_open_orders_size, pme_max_option_open_orders_size, non_pme_max_future_position_size, non_pme_total_short_options_positions_size, pme_max_risk_reducing_orders, can_not_cancel_liquidation_order, can_not_edit_liquidation_order, disabled_while_position_lock, max_spot_open_orders, max_spot_order_quantity, other_reject, no_more_triggers, unauthenticated_public_requests_temporarily_disabled, method_switched_off_by_admin, mmp_trigger, active_combo_limit_reached, unavailable_for_combo_books, quotes_frozen, unavailable, request_cancelled_by_user, KYC_data_inaccessible, no_more_oto_orders, too_many_quotes_per_block_rfq, too_many_quotes_per_block_rfq_side, not_fully_filled / too_many_open_block_rfqs, quote_crossed, max_broker_client_count, account_quote_limit_crossed, inverse_future_cross_trading | Rejected(other) | refused, nothing placed |
| `10028`, `10047`, `10066`, `13780`, `13903` | too_many_requests, matching_engine_queue_full, too_many_concurrent_requests, move_positions_over_limit, too_many_quotes | RateLimited | refused; held for the `Retry-After` wait where the answer carries one (none is documented) |
| `10000`, `12003`, `12998`, `13004`, `13005`, `13006`, `13007`, `13009`, `13778` | authorization_required, login_over_limit, security_key_authorization_over_limit, invalid_credentials, pwd_match_error, security_error, user_not_found, unauthorized, raw_subscriptions_not_available_for_unauthorized | Auth | refused; the key or the clock is named |
| `11042`, `11098`, `12005`, `13012`, `13021`, `13031`, `13037`, `13040`, `13403`, `13792`, `13902`, `13904` | permission_denied, account_locked, country_is_banned, unavailable_in_subaccount, forbidden, verification_required, incomplete_KYC_data, mmp_required, scope_exceeded, incomplete_KYC_data, mass_quotes_disabled, security_key_setup_required | Permission | refused; the scope is named |
| `10022`, `10023`, `10024`, `10025`, `10030`, `10031`, `10033`, `10036`, `10048`, `10049`, `11029`, `11038`, `11039`, `11041`, `11043`, `11045`, `11047`, `11048`, `11049`, `11050`, `11052`, `11053`, `11055`, `11059`, `11060`, `11090`, `11091`, `11092`, `11093`, `11095`, `11096`, `11097`, `12001`, `12002`, `12004`, `12100`, `13010`, `13011`, `13013`, `13014`, `13015`, `13016`, `13018`, `13032`, `13034`, `13042`, `13781`, `13791`, `13912`, `13913`, `13914`, `13915`, `13916`, `13919`, `13922`, `-32602`, `-32600`, `-32601`, `-32700`, `-32000` | invalid_quantity, invalid_price, invalid_max_show, invalid_order_id, must_be_websocket_request, invalid_args_for_instrument, not_implemented, invalid_max_show_amount, not_on_this_server, cancel_on_disconnect_failed, invalid_arguments, no_adv_for_futures, no_adv_postonly, not_adv_order, bad_argument, invalid_event, unsupported_arg_combination, wrong_max_show_for_option, bad_arguments, bad_request, subscribe_error_unsubscribed, transfer_not_found, post_only_not_allowed, iceberg_not_allowed, not_supported_for_coinbase_routed_spot, invalid_addr, invalid_transfer_address, address_already_exist, max_addr_count_exceeded, disabled_deposit_address_creation, address_belongs_to_user, no_deposit_address, too_many_subaccounts, wrong_subaccount_name, registration_over_limit, transfer_not_allowed, value_required, value_too_short, invalid_phone_number, cannot_send_sms, invalid_sms_code, invalid_input, invalid_content_type, non_unique_order_label, no_more_security_keys_allowed, cod_not_enabled, coupon_already_used, KYC_transfer_already_initiated, broker_cannot_be_client, broker_already_linked, user_is_a_broker_client, user_is_not_a_broker, app_registered_to_broker, client_of_main_account, linked_order_type_not_supported_for_csr_spot, Invalid params, request entity too large, Method not found, Parse error, Missing params | InvalidRequest | refused (a bug of OTW or a wrong setting) |

### Rate limits for orders

Matching-engine calls (`private/buy`, `sell`, `edit`, `cancel`, `cancel_all_by_instrument`)
draw from a per-sub-account limit by tier, recalculated hourly: from 5 requests a second
(bursts of 20, tier 4) to 30 (bursts of 100, tier 1) (RL, "Matching Engine Requests"). Running
out answers `10028 too_many_requests` "and terminate[s] the session" (RL); no wait is stated.
`10047 matching_engine_queue_full`: "The Portfolio Queue can hold up to 8 waiting processes"
(OM). Production and testnet "operate on separate, independently-tracked rate-limit pools"
(RL). Reads keep the declared 20 a second.

### Stream

`wss://www.deribit.com/ws/api/v2` (`wss://test.deribit.com/ws/api/v2` on the test
environment), JSON-RPC:
1. `public/auth` with `grant_type=client_signature`, `client_id`, `timestamp`, `nonce`,
   `data` empty and `signature` = hex HMAC-SHA256 of `timestamp + "\n" + nonce + "\n" + data`
   (AUTH, "Client Signature (WebSocket)"; its worked example `56590594f979...` is a byte
   test). "Over WebSocket, a successful call also authenticates the current connection"
   (public/auth). JSON-RPC id 1.
2. `private/subscribe` to `user.changes.future.any.raw` (id 2): one frame per change with the
   instrument's `orders`, `trades` and `positions` ("raw ... available only to authorized
   users"). The answer must list the channel.
3. `public/set_heartbeat` with `interval` 30 (id 3). "If your client sends no message for
   `(2 * interval) + 500 ms` ... the API server closes the WebSocket with close code `4000`.
   Any JSON text message from the client resets this timer" (CM): `public/test` (id 4) goes
   out every 15 s, and answers the venue's `test_request` too.

A fired trigger in a frame whose new order is not in it is read again by its id and followed.
A fill from the stream moves nothing by itself: its order's own row in the frame says how
far the order went. Only a fill of the account's own liquidation has the account read
again, as a position moved without an order of OTW: `order_type` `liquidation`, or
`liquidation` ("`M` when maker side of trade was under liquidation, `T` when taker side")
naming the side `liquidity` gives the account ("what was role of users order"). A
counterparty's liquidation sets `liquidation` too: an order of the account filled against it
is an ordinary fill.

Every fill is read from the REST stores by execution id
(`trade_id`, "Unique (per currency) trade identifier": fills are read per instrument). Fills:
`private/get_user_trades_by_instrument_and_time`, `historical=false` where the window reaches
the last 24 hours ("Recent trades: Available for 24 hours", HIST) and `historical=true` where
it starts before them ("Recent data is not included"), an hour of overlap, a fill read twice
dropped by its id; 1000 a page, the next page from the last fill's millisecond. The fee is
carried as stated (its sign is not documented).

### Disconnect behaviour

Nothing: cancel on disconnect is off by default and WebSocket only (CM, "Cancel on
Disconnect"); OTW does not enable it. Working orders stay working; the account is reconciled
on reconnect.

### Dry-run

None. `private/get_margins` "Calculates margin requirements for a hypothetical order"
(instrument, amount, price only): it checks no type, time in force, flag or rule, so it is not
declared.

### Quirks

- Doc gaps: the label's charset; which currency a linear contract's label and trigger
  history are filed under (both asked); whether
  `get_order_state_by_label` lists working orders (its page names closed ones only: the
  working list is asked first); whether
  an edit that omits `reduce_only` clears it (always resent); a market order's time in force
  and unfilled remainder on derivatives; the reduce-only refusal's code (none in the table;
  the trigger history shows `rejected:reduce_direction`); the fee's sign; the widest fills
  window per call; the `with_continuation` answer's field names (paged by `offset` instead);
  which instruments carry `tick_size_steps`; the maximum order size and minimum notional; a
  linear position's `size` unit ("in quote currency" next to `size_currency` "in base
  currency": `size_currency` is read); whether the trigger history lists a trigger that fired
  long ago within its 1000 entries; the `new` state of the trigger history and the missing
  `triggered` in the state prose; the contradicting "Common Error Codes" table; which channels
  send a snapshot after subscribing; whether a 5xx can carry a JSON-RPC refusal.
- No `user.changes` notification example is documented: the stream fixture is built from the
  channel's payload schema.
- Every answer carries `testnet` (`false` for production, RPC); the host is chosen by the
  account's `env`, the flag is not checked.
- Unproven until a test-host run: real fills and their order, real error bodies on the order
  path, ack timing, the signer against a real key (REST and socket).

## Regions and terms

Deribit does not serve some jurisdictions (the United States among them); public market data
answered from the test machine.

## Live best bid and ask

Channel `quote.<instrument>`, joined and answered like a candle channel: `{timestamp,
best_bid_price, best_ask_price, best_bid_amount, best_ask_amount}` on every change (measured
2026-10-19). An inverse contract's amounts are USD and are not carried.
