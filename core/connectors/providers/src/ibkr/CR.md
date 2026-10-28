# IBKR gateway: trading checks

## 2026-10-25, code and unit tests only (no gateway running)

Nothing below ran against IB Gateway: port 4002 was closed. Unit tests only; the paper
catalog compiles and skips without `OTW_IBKR_PAPER`.

Changes:
- T11b: `minSize` and `sizeIncrement` read from contract details with `minTick` (`Rules`,
  kept a day per contract, read again after a 110). A size under the minimum or off the step
  is refused before sending, naming the step and the valid sizes (0.5 MNQZ6: "the smallest
  order is 1"). Also checked on amend.
- Lookup by client id (`orders::lookup`, `Broker::lookup`): `orderRef` among
  `reqAllOpenOrders`, `reqCompletedOrders` and `reqExecutions`; not found only when all three
  answered.
- Unknown outcome (6.1 step 6, 6.3): no answer in 10 s, a socket gone while sending or
  waiting, or 103 is looked up; still unsettled, `place` answers `OrderUnknown` with every leg
  that went out. A lone order is no longer cancelled on a lost answer; a ticket's legs still
  are. Not listed is unknown, never a refusal.
- Codes (spec 8): 100 RateLimited (retry 1 s), 103 Unknown, 504 and 1100 Transient, 161
  read from the open orders, 10148 or a cancel status `Filled` answer done (T22: no error).
  1100 refuses new orders and amends until 1101 / 1102; 1101 and 1300 resubscribe the account
  feed, 1102 says `Resync`.
- Account stream: a filled quantity never shrinks, an order that ended never works again on
  a late message. Executions: a repeated execId counts once, a correction replaces its fill.
- Clippy: the midpoint mark is taken in decimals (`marks.rs`, `float_arithmetic`).

| Test | Where | Result |
| --- | --- | --- |
| T11b size under minimum / off step | `orders.rs` unit | PASS |
| T20 never seen: not found, not refused | `orders.rs` unit (`settle`, `after_silence`) | PASS |
| T21 IB unreachable: unknown, legs named | `orders.rs` + `ibkr.rs` unit | PASS |
| T22 cancel of a filled order is done | `orders.rs` unit | PASS |
| T24 duplicate execution, out of order status, late fee | `orders.rs`, `ibkr.rs`, `account.rs` unit | PASS |
| T25 correction replaces the fill | `orders.rs`, `account.rs` unit | PASS |
| T27 1101 / 1102 handling | `events.rs` unit | PASS |
| T30 pacing (100) | `orders.rs` unit | PASS |
| T22b, T26, T27, T32, T20 on paper | `ibkr_paper_tests.rs` | NOT RUN |
| T03 partial fills | | N/A on paper: the simulator fills a marketable order whole; unit tests cover the sums |
| T28 cancel on disconnect | | N/A: IB has none for API orders here, none armed |
| T29 recvWindow | | N/A (Binance only) |
| T33 position drift | | left to the otw-core reconciliation |

Still unverified until a paper run: that `reqCompletedOrders` lists an order cancelled or
filled today under its `orderRef`; that a reset session lists the same order and the feed says
`Resync` within 15 s; T22 on a real fill; 1100/1101/1102 on a real outage (the daily restart);
the size refusal on a contract with fractional sizes (crypto, stocks with fractions).

## 2026-10-23 (2), after fixes T15, T19, T22

Same command. 2 runs, identical. 36 PASS, 0 FAIL, 1 GAP.

| Test | Result | Detail |
| --- | --- | --- |
| T15a margin known ahead (chart opened the instrument) | PASS | 20000 lots refused before sending, 48 to 56 ms, IB what-if figures |
| T15c ticket that fits | PASS | no extra call, ack 129 to 135 ms |
| T15b margin not known yet (other contract) | PASS | refused with IB's reason (`201`, available funds) |
| T19z same client id twice | PASS | second refused before sending |
| T22a cancel an already cancelled order | PASS | answers done |
| T11b fractional contract | GAP | refused by IB (`10318`) after sending; rule to read from contract details |
| all others (C, T01 to T12, T31, T34, T04, T10, Z) | PASS | unchanged |

Fixes:
- T15: per-unit margin and available funds from an IB what-if, warmed when a chart opens the
  instrument, refreshed every 5 min while used. A rough need that fits passes with no call; one
  that does not is asked of IB exactly and refused only on IB's figures. An order IB answers
  `Inactive` returns IB's reason instead of "accepted".
- T19: client order ids sent per gateway kept in process (seeded once from working orders);
  a reused id is refused. Lost on restart.
- T22: cancel of an order IB names final (`10148`) answers done; filled says so.
- Account feed: what-if echoes are not shown as orders.

Workspace: 1512 passed, 4 failed (histdata golden weekly windows shifted by the date,
unrelated), `otw-connector-engine` tests do not build (pre-existing, `Capabilities.orders`).

## 2026-10-23, IB Gateway paper (127.0.0.1:4002), MNQZ6

`OTW_IBKR_PAPER=127.0.0.1:4002 cargo test -p otw-core ibkr_paper -- --nocapture`
(`otw-core/src/brokers/ibkr_paper_tests.rs`). 2 runs, identical. 31 PASS, 0 FAIL, 4 GAP.

| Test | Result | Detail |
| --- | --- | --- |
| C01 connect | PASS | |
| C02 paper account only | PASS | 1 account, `D…` |
| C05 account stream | PASS | |
| T01 limit rests, cancel | PASS | ack 507 ms, cancel 109 ms, tick 0.25 |
| T06 amend price + qty | PASS | 138 ms, book echoes qty 2 and new price |
| T06 amend off tick, qty 0 | PASS | refused before I/O |
| T11 off tick, qty 0, price ≤ 0, limit without price | PASS | refused before I/O |
| T11 fractional contract (0.5) | GAP | refused by IB (`10318`) after sending, not before |
| T12 GTD without expiry, unknown contract, continuous root | PASS | refused, nothing guessed |
| T11z nothing reached IB on refusals | PASS | |
| T05 stop, stop-limit rest | PASS | |
| T07 bracket linked, cancel entry cancels exits | PASS | |
| T08 OCA pair | PASS | |
| T09 IOC unfilled | PASS | |
| T09 GTD +70 s expires | PASS | gone after 69 s |
| T15 margin (20000 lots) | GAP | `place` answers accepted; IB rejects after; stream says `rejected`, no reason |
| T19z same client id twice | GAP | both accepted |
| T22a cancel an already cancelled order | GAP | error `10148` shown to the user instead of its final state |
| T23a amend a cancelled order | PASS | refused, "not working any more" |
| T31 order from another API client | PASS | listed |
| T34 kill switch | PASS | 3 cancelled (external included) in 509 ms, none left |
| T04 market buy 1 | PASS | ack 131 ms, fee 0.61 USD, mult 2 |
| T10 close position | PASS | 129 ms, flat |
| T04b stream fills + positions | PASS | |
| Z01/Z02 nothing left, position as before | PASS | |

Notes:
- Position `avg_price` 29038.805 is off tick: IB `avgCost` includes the commission (0.61 / 2).
- Fill 3.75 below the quoted ask on both runs: quote source vs paper fill to check.

Not run (no fault injection on a real gateway): T03 partial fills, T19 to T21 lost answer,
T22 cancel/fill race, T24 to T30, T32 restart, T33 drift. Code check `gateway/orders.rs`: on
`ORDER_WAIT` timeout, looks the order up by `orderRef`; not found, answers an error and
cancels the ids it sent; no persisted `Unknown` state.
