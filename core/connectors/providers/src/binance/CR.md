# Binance: trading checks

## 2026-10-25 evening, through the app (`testbook/brokers.sh`)

The catalog now runs through the app's own routes (grant, lock, live gate, risk gate, order
store, audit log in the path), logged in as the stack's user, on the accounts set to
`env = testnet`. Suite: `testbook/part1-api/src/scenarios/brokers_net.rs`. Reports in
`testbook/reports/20261025-180720/` (with `--restart`) and `20261025-183114/` (with `--stress`).

- Two runs agree, 113 PASS each, 0 FAIL; 2 WARN: `order/test` does not check funds (D02).
- T07 resting bracket on a flat USD-M account: refused before I/O (was the T20 `-2022` FAIL);
  with a position held it rests, and cancelling the entry takes its exits; a market entry's
  exits work (filled exit cancels the other).
- T28 the core stopped 150 s with a futures order working: Binance cancelled it (countdown).
- T32 restart with orders on both accounts: listed after, nothing resent (audit count equal).
- E01 to E04 stream: order and done events under 1 s.
- Dry run (`POST /api/brokers/{id}/dry-run`): accepted by the test route, nothing placed;
  off-tick refused before sending.
- Differential paper vs testnet (same script, BTCUSDT): outcomes equal step by step.
- Stress: 20 orders at once per account all placed and listed; kill switch cancels 20 in
  0.8 s (spot) and 1.4 s (futures).
- T13 / T14 through the routes: size limit and stale-price collar refuse, trading off refuses
  a place and lets a cancel through; settings put back.
- Core log: no signature or key header during the runs.

Fixed on the way: a lost answer that could not be looked up was stored as refused, now unknown
(T21, mock test); a 429 / 418 refusal names the wait (T30, mock test); a fill counted twice
by the drift check raised a false drift (overlap window), now a move is drift only when
neither window explains it.

## 2026-10-25, same accounts, after filters, stream, stop swap, countdown, store

Same command, 1 run. Catalog 69 PASS, 1 FAIL; account stream 6 PASS.

| Test | Spot | Futures | Detail |
| --- | --- | --- | --- |
| E01 stream opens | PASS | PASS | WS API `subscribe.signature` 2.1 s (HMAC key) / listen key 1.6 s |
| E02 order event | PASS | PASS | far limit pushed whole, id `SYMBOL:client id` |
| E03 done event on cancel | PASS | PASS | `cancelled`, 258 / 288 ms |
| T06 move a stop, one stop working | PASS | PASS | futures: new stop `….r1` placed then old cancelled, 1.8 s |
| T11 off tick / off lot | PASS | PASS | refusal names both nearest grid values |
| T15, T19z, T22a, T23a, T34, T04, T10, C01 to C03 | PASS | PASS | unchanged |
| T20 bracket entry and both exits rest | | FAIL | `-2022` ReduceOnly Order is rejected: a reduce-only take-profit limit cannot rest while the account is flat |
| T21 filled exit cancels the other | | PASS | |

Open: T20 needs a design choice (exits held until the entry fills, or not offered on a flat
entry). Not run: countdown firing on the venue (T28), resume after a real restart (T32).

## 2026-10-23, Spot Demo Mode (demo-api.binance.com) + USDⓈ-M futures testnet, BTCUSDT

`OTW_BINANCE_TESTNET=<spot account id>,<futures account id> OTW_SECRET_KEY=... cargo test -p otw-core binance_testnet -- --nocapture`
(`otw-core/src/brokers/binance_testnet_tests.rs`, accounts "Binance Spot Testnet" and "Binance
Futures Testnet"). 2 runs, identical. 68 PASS, 0 FAIL.

| Test | Spot | Futures | Detail |
| --- | --- | --- | --- |
| C01 connect | PASS | PASS | Testnet in the answer |
| C02 live account refused | PASS | PASS | before sending, "testnet only until M4" |
| C03 chart opens the instrument | PASS | PASS | clock + terms read: 3.8 s spot, 1.7 s futures |
| T01 limit rests, listed, cancel | PASS | PASS | ack 260 / 240 ms after C03 (9.6 s cold on spot) |
| T06 amend price + qty, same id | PASS | PASS | spot cancel-replace 780 ms, futures `PUT` 1050 ms |
| T06 amend off tick | PASS | PASS | refused before I/O |
| T06 tif change | PASS | PASS | spot rides the replace; futures refused (no such call) |
| T05 stop, stop-limit rest, listed | PASS | PASS | futures in the algo book |
| T06 move a stop, same id | PASS | PASS | futures: cancel + new, 1.3 s |
| T05 buy stop below market | PASS | PASS | venue refuses (`-2010` / `-2021`) |
| T11 off tick, off lot, qty 0, no price, day, id > 36 | PASS | PASS | refused before I/O |
| T11 gtd on spot | PASS | | refused |
| T12 unknown symbol, `BTC/USDT` | PASS | PASS | refused, nothing guessed |
| T11z nothing reached the venue | PASS | PASS | |
| T09 IOC far from market | PASS | PASS | not working after |
| T09 GTD +15 min | | PASS | |
| T15 beyond balance / margin | PASS | PASS | venue, synchronous: `-2010` 262 ms / `-2019` 240 ms |
| T19z same client id twice | PASS | PASS | refused before sending |
| T22a cancel a cancelled order | PASS | PASS | answers done |
| T23a amend a cancelled order | PASS | PASS | "not working any more" |
| T34 kill switch | PASS | PASS | 2 cancelled, 760 ms / 2.0 s, none left |
| T04 market buy | PASS | PASS | ack 263 / 244 ms |
| T04 sell back / position long | PASS | PASS | spot fee taken in BTC: sell size minus fee |
| T10 close position, close when flat | PASS (n/a) | PASS | reduce-only market, 960 ms; flat refused |
| account figures | | PASS | equity, available, margin |
| Z01 nothing left working | PASS | PASS | |

Gaps (not built, not run):
- No user data stream: book read on a cycle. Spot listen key answers `410`, needs the WS API.
- Sent client ids are lost on restart.
- T03 partial fills, T20/T21 lost answer, T22 cancel/fill race, T32 restart: not run.

## Since 2026-10-23: built, unit tested on fixtures, not yet run on testnet

- Spot order lists (`orderList/oco`, `oto`, `otoco`), a market entry's exits after its fill;
  USDⓈ-M brackets and OCO pairs linked by OTW, taken back up after a restart (`resume`).
- USDⓈ-M hedge mode (`positionSide`, no `reduceOnly`); a close-position exit sends no
  `reduceOnly` (Binance refuses the pair).
- Filters before sending: price range, max size, `MARKET_LOT_SIZE`, `NOTIONAL` /
  `MIN_NOTIONAL`, `PERCENT_PRICE(_BY_SIDE)` against the reference / mark price,
  `MAX_NUM_ORDERS`, `MAX_NUM_ALGO_ORDERS`.
- Futures stop change: new stop first under `<id>.rN`, then the old one cancelled (no gap; the
  id changes). T06 "move a stop, same id" expects the old id on futures and must look for
  `<key>.r1` there.
- Cancel of an order that filled first answers done.
- Order buckets from `X-MBX-ORDER-COUNT-*`, refused not queued; 429 / 418 holds.
- USDⓈ-M `countdownCancelAll` heartbeat (120 s, every 30 s); positions carry leverage and
  margin mode (`symbolConfig`).
- Spot account figures: USDT cash and available; equity from the wallet valuation on live.
- Live gates (`apiRestrictions`, `order/test`): live only, unreachable below M4.
- User data stream (`Broker::events`): spot `userDataStream.subscribe.signature` on the
  WebSocket API, USDⓈ-M listen key on `/private`, kept alive every 30 min; order, fill, done,
  position events, resync on every reconnect. Testnet check: `binance_testnet_account_stream`
  (E01 opens, E02 order event, E03 done on cancel), not yet run.

Workspace: `otw-core` 1150 passed, 4 failed (histdata golden weekly windows shifted by the
date, unrelated); providers 355 passed; frontend `histviz-orders` 20 passed.
