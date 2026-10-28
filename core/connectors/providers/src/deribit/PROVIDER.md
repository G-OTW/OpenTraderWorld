# Deribit (perpetuals, futures, options, spot)

Adapter version: 0.1.0. Provider API version: v2 (JSON-RPC over HTTP and a socket). Maturity:
M2 (mock catalog green; history, the instrument list, quotes, the board, the clock and the
socket checked against the live venue on 2026-10-19; account parity on mock only, no key
lent).

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
- Positions: `get_positions?currency=any`, every kind in one call. `direction` `buy` long,
  `sell` short (negative quantity), `zero` skipped. Linear futures count `size_currency`
  (the coin), options `size` (the coin). Inverse futures are converted from USD contracts to
  coins at the entry price (`size / average_price`), quoted in USD, and their unrealised
  profit (in the coin) is not carried, as on Bybit.
- Open orders: `get_open_orders`, every kind. Inverse amounts to coins at the order's price,
  else its trigger price; a `price` of `"market_price"` (an untriggered market stop) is no
  price.
- Fills: per (currency, kind) group, not per instrument (`fills_need_symbols = false`). With
  no instrument named the walk reads BTC, ETH and USDC options, USDC futures, and spot under
  USDC, USDT, BTC and ETH. Named instruments narrow it to their groups. Each group is read
  `historical=true` over the period (kept for good, indexed after a short delay) and then
  `historical=false` (the last 24 hours, available at once) where the period reaches them;
  1000 fills a page, oldest first, the next page starting at the page's last millisecond. A
  fill seen twice is handed out once (ids kept for the walk). More than 1000 fills in one
  millisecond cannot be paged and is refused. Fill rows: `amount` (the coin), `price`,
  `fee` (positive a cost, negative a rebate) in `fee_currency`, `direction` the account's
  side; the quote and class are read from the name.
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
  `account:read`, `trade:read`.
- `public/auth` (client credentials or client signature, then a bearer token) is not used.

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
- The test environment (`https://test.deribit.com`, its own keys) is not wired: the same
  adapter would serve it with its base URL moved.

## Regions and terms

Deribit does not serve some jurisdictions (the United States among them); public market data
answered from the test machine.

## Live best bid and ask

Channel `quote.<instrument>`, joined and answered like a candle channel: `{timestamp,
best_bid_price, best_ask_price, best_bid_amount, best_ask_amount}` on every change (measured
2026-10-19). An inverse contract's amounts are USD and are not carried.
