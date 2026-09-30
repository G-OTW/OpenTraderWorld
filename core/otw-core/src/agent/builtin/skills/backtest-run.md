# Running a backtest

## When to use

Any time you simulate a strategy. Deciding whether the *result* means anything is a separate
job — `backtest-validate` covers that, and a positive verdict requires it.

## Preconditions

A dataset (`data-acquire`) and a settings object.

**Do not compose settings by trial and error.** The API reports one missing field per attempt,
so guessing costs a round trip per field and you will exhaust your tool budget before the shape
is complete. Start from the working example below, or from a saved strategy
(`GET /api/backtest/strategies` → `GET /api/backtest/strategies/{id}`) when one exists.

## A complete, valid settings object

This runs as-is. Change the indicators, periods, `op`, and the cost fields; keep every key.

```json
{
  "kind": "signals", "grid": null, "mode": "long",
  "long": {
    "entry": {"logic": "all", "conditions": [
      {"left":  {"kind": "indicator", "indicator": "ema", "period": 20},
       "op":    "crosses_above",
       "right": {"kind": "indicator", "indicator": "ema", "period": 50}}]},
    "exit": {"logic": "all", "conditions": [
      {"left":  {"kind": "indicator", "indicator": "ema", "period": 20},
       "op":    "crosses_below",
       "right": {"kind": "indicator", "indicator": "ema", "period": 50}}]},
    "stop_loss_pct": 0.05, "take_profit_pct": 0.10, "exit_on_reverse": false
  },
  "short": {
    "entry": {"logic": "all", "conditions": [
      {"left":  {"kind": "indicator", "indicator": "ema", "period": 20},
       "op":    "crosses_below",
       "right": {"kind": "indicator", "indicator": "ema", "period": 50}}]},
    "exit": {"logic": "all", "conditions": [
      {"left":  {"kind": "indicator", "indicator": "ema", "period": 20},
       "op":    "crosses_above",
       "right": {"kind": "indicator", "indicator": "ema", "period": 50}}]},
    "stop_loss_pct": 0.05, "take_profit_pct": 0.10, "exit_on_reverse": false
  },
  "stop_and_reverse": false, "pyramiding": 1,
  "sizing": {"mode": "percent_equity", "percent": 100},
  "starting_capital": 10000, "leverage": 1, "spread_pct": 0,
  "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.1},
  "risk": {},
  "pyramid_steps": {"scale": [], "min_distance_pct": 0, "after_add_sl": "none"},
  "instrument": {"multiplier": 1, "lot_step": 0, "min_qty": 0},
  "slippage": {"kind": "pct", "value": 0.0005, "tick_size": 0},
  "oos_split_pct": 0,
  "funding": {"annual_rate_pct": 0, "interval_hours": 8},
  "filters": {},
  "indicators": {}
}
```

**`exit_on_reverse` is not "exit on the opposite signal".** It means *exit as soon as the
entry group stops being true*. With an event operator — `crosses_above`, `crosses_below`,
`cross` — the entry group is true only on the bar the cross happens, so the position is closed
on the very next bar and **every trade lasts one bar**. The run still looks plausible (trades,
a win rate, a positive return) and is meaningless: on BTCUSDT daily, EMA20/50 with
`exit_on_reverse: true` returned +3 743 while the same rules with an explicit cross-down exit
returned +32 979.

So: with a `crosses_*` entry, write the exit as its own condition and leave
`exit_on_reverse: false` (the example above). Reserve `exit_on_reverse: true` for **state**
entries — `above`, `below`, `rising`, `falling` — where "the entry condition no longer holds"
is a real exit.

**Two fields are both called `mode` and they are unrelated:** top-level `mode` is
`"long" | "short" | "both"` (which directions may trade); `sizing.mode` is
`"percent_equity" | "fixed_qty" | "risk" | "equity_tiers" | "kelly"`. Mixing them up produces
`unknown variant` errors that look like a typo and are not.

Supply **both** `long` and `short` blocks even for a long-only run — `mode` decides what
actually trades.

A condition side (`left` / `right`) is one of exactly four kinds:

- `{"kind": "price", "field": "close"}` — open/high/low/close/volume
- `{"kind": "const", "value": 30}` — a fixed number. **It is `const`, not `value`**; `"value"`
  is the field *inside* it. This is the single most common rejection.
- `{"kind": "indicator", "indicator": "rsi", "period": 14}` — plus `fast`, `slow`, `mult`,
  `signal_period` where that indicator uses them
- `{"kind": "custom_indicator", "id": "<id>"}` — and **the definition must be embedded** in
  `settings.indicators` as `{"<id>": {…def…}}` (`GET /api/backtest/indicators/{id}`). The engine
  never reads the library at run time, so an id missing from that map is an undefined series on
  every bar: the signal never fires, with no error and no warning.

`op` is one of: `above`, `below`, `crosses_above`, `crosses_below`, `cross`, `rising`,
`falling`, `closing_above`, `closing_below`, `opening_above`, `opening_below`.

`kind` is one of three families: `"signals"` (everything above), `"grid"` (needs a `grid`
object) and `"dca"` (a savings plan: needs a `dca` plan, ignores sizing / pyramiding / SL-TP,
and **refuses** a non-zero `oos_split_pct`). Leave `grid` and `dca` `null` unless that is what
you were asked for.

## Stops: three layers, the object form wins

`stop_loss_pct` / `take_profit_pct` are the legacy fractions. `stop_loss` / `take_profit` are
`{"kind": "pct"|"atr", "value": 0.05, "period": 14}` and **override** the legacy fields when
present. `trailing_stop` is independent of both:
`{"kind": "pct"|"abs", "value", "activate_pct", "breakeven_pct"}` (a percent of the extreme or a
raw price distance, and nothing else: a distance that reads an indicator is an indicator), re-computed
at each closed candle from that candle's favorable extreme; when a fixed stop is also set, the
level the market reaches first is the one that fires and names the exit reason
(`trailing_stop`).

`activate_pct` arms the trail only once the trade is that far in profit (below it the fixed
stop holds the position alone); `breakeven_pct` moves the stop to the average entry. Either
alone is a legal rule, so `value: 0` with a `breakeven_pct` is "go to breakeven, never trail".

Risk-based sizing needs a stop price *at entry*: a fixed `stop_loss`, or a trail with
`value > 0` **and** `activate_pct: 0`. A trail that waits for activation does not satisfy it.

## Trading window

`filters` gates when an entry may open: `tz_offset_min` (a fixed offset, no DST), `weekdays`
(1 = Monday … 7 = Sunday), `sessions` (OR-ed intraday windows), `include_dates` /
`exclude_dates`, `on_window_end` (`"hold"`, the default, or `"flat"` = close at the open of the
first bar outside the window, exit reason `session_end`) and `block_adds` (whether a closed
window also blocks pyramiding adds). All empty = always open. The response reports what it
blocked in `filtered_bars`.

## Units — the single biggest source of wrong answers

The `_pct` suffix does **not** tell you the scale. Two families:

- **Fractions** (`0.01` = 1%): `long/short.stop_loss_pct`, `take_profit_pct`, the `value` of a
  `stop_loss` / `take_profit` / `trailing_stop` of `kind="pct"`, `trailing_stop.activate_pct`
  and `breakeven_pct`, `slippage.value` when `kind="pct"`, `spread_pct`, `oos_split_pct`,
  `pyramid_steps.min_distance_pct`.
- **Percents** (`10` = 10%): `sizing.percent`, `sizing.risk_pct`, `risk.max_*_pct`,
  `funding.annual_rate_pct`, and `fees.amount` when `amount_kind="pct"`.

So `slippage: {kind:"pct", value:0.05}` is **5% per fill** and will destroy the account, while
`fees: {amount_kind:"pct", per:"trade", amount:0.1}` is **0.1% per trade**. Realistic crypto
values: slippage `0.0005`, fees `0.1`.

## Procedure

1. `POST /api/backtest/run` with `dataset_ids`, `settings`, and **`view:"summary"`** — the
   default `"full"` returns every trade and the whole equity curve and will blow your context
   on a long dataset. The run still lands in history either way, reachable by `run_id`.
2. Optionally restrict the span with `from`/`to` (`"2024-01-01"` or RFC3339; a plain end date
   covers that whole day). Leave `limit` alone — it keeps the *most recent* bars of the range,
   so setting it small silently truncates to the tail.
3. **Read `warnings`.** The engine flags legal settings that measure something other than the
   stated idea (the `exit_on_reverse` trap above, a DCA weight naming no loaded ticker). A
   warning is not a rejection: the run happened, and reporting its numbers without acting on
   the warning is reporting a different strategy.
4. Check `bars` in the response is the span you expected. Limits: at most 8 datasets per run,
   `limit` defaults to 50 000 bars per dataset (clamped to 200 000), and the whole run is capped
   at 400 000 simulated points, Σ (assets × bars).
5. Name a run worth keeping: `POST /api/backtest/runs` with the `run_id` and a name, or the
   auto-history cap will eventually evict it.

## Reading the result

`stats` carries `return_pct`, `net_pnl`, `trades`, `wins`, `losses`, `win_rate`,
`profit_factor`, `avg_trade`, `max_drawdown_pct`, `max_drawdown` (currency), `sharpe`,
`sortino`, `expectancy_pct`, `total_fees`, `final_equity`, `exit_reasons`, `buy_hold_return_pct`
and `engine_version`. The `all` / `long` / `short` sub-blocks hold the per-direction breakdown,
and that is where **`avg_bars_held`** and the streak counters live (`stats.all.avg_bars_held`,
never top level).

Beside `stats`, a run reports `warnings`, `warmup_bars` and `trading_start_ts` (the first bar
the strategy could act on), `alignment` (the merged clock for a multi-asset run), `per_asset`,
`oos` when `oos_split_pct > 0`, `filtered_bars` (blocked by `filters`), `halted_bars` (blocked
by a `risk` circuit breaker), `skipped_min_size` / `skipped_margin` (entries refused by
`instrument.min_qty` or by margin), `total_funding`, and `grid` / `dca` for those kinds. A
non-zero skip or halt count means the run traded less than the rules asked for: say so.

Exit reasons in use: `signal`, `stop_loss`, `take_profit`, `trailing_stop`, `reverse`,
`session_end`, `grid_reset`, `end` (open at the last bar).

Two habits:

- **Always compare against `buy_hold_return_pct`.** A strategy returning 30% where buy-and-hold
  returned 2208% is not a good strategy; it is an expensive way to avoid owning the asset. Say
  that plainly.
- **Read `exit_reasons`.** A strategy whose exits are nearly all `stop_loss` behaves very
  differently from one exiting on `signal`, whatever the headline return says.

## Pitfalls

- **Fees or slippage left at zero** makes every result fiction. If the user did not specify,
  use realistic values and *say which you used*.
- **A handful of trades proves nothing.** Report the trade count next to every performance
  figure, always.
- **`view:"full"` on a long dataset** wastes the context you need later in the task.
- **Datasets must share a timeframe** in a multi-asset run, and there is a bar budget — a
  rejected run says which limit it hit.

## Verification

Before reporting: confirm `warnings` is empty (or that you acted on what it said), the bar
count matches the intended window, and the trade count supports the claim you are about to
make. **The response does not echo the settings**, so the slippage, fees and sizing you report
are the ones you posted; when in doubt, read the run back from `GET /api/backtest/runs`, whose
rows carry the stored `settings`.

Then read `stats.all.avg_bars_held` and `exit_reasons` and check they match the strategy you
described:

- `avg_bars_held` around 1 on anything but an explicitly intraday-flat rule means the position
  is being closed the bar after it opens — almost always `exit_on_reverse` on a `crosses_*`
  entry. **Fix the configuration and re-run; do not report the numbers.**
- `exit_reasons` dominated by `stop_loss` when you meant to exit on signal (or by `signal`
  when you set a tight stop) means the run tested something other than the idea.

Say what the exits actually were, next to the return.

## Report shape

Dataset identity and window, the settings that matter (fees, slippage, sizing), then the stats
with `trades` beside them, the buy-and-hold comparison, and the `run_id`. Never a return figure
on its own.
