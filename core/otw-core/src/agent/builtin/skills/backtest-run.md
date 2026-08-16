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
  "funding": {"annual_rate_pct": 0, "interval_hours": 8}
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
- `{"kind": "custom_indicator", "id": "<id from /api/backtest/indicators>"}`

`op` is one of: `above`, `below`, `crosses_above`, `crosses_below`, `cross`, `rising`,
`falling`, `closing_above`, `closing_below`, `opening_above`, `opening_below`.

`"kind": "grid"` is a different strategy family and needs a `grid` object; leave it `null`
unless that is what you were asked for.

## Units — the single biggest source of wrong answers

The `_pct` suffix does **not** tell you the scale. Two families:

- **Fractions** (`0.01` = 1%): `long/short.stop_loss_pct`, `take_profit_pct`, `slippage.value`
  when `kind="pct"`, `spread_pct`, `oos_split_pct`, `pyramiding.min_distance_pct`.
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
3. **Read the echoed settings back** before believing anything. The response carries the
   settings actually used; check slippage, fees and sizing against what you intended.
4. Check `bars` in the response is the span you expected.
5. Name a run worth keeping: `POST /api/backtest/runs` with the `run_id` and a name, or the
   auto-history cap will eventually evict it.

## Reading the result

`stats` carries `return_pct`, `trades`, `win_rate`, `profit_factor`, `max_drawdown_pct`,
`sharpe`, `sortino`, `expectancy_pct`, `total_fees`, `final_equity`, `exit_reasons`, and
`buy_hold_return_pct`. There are also `all` / `long` / `short` sub-blocks.

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

Before reporting: confirm the echoed slippage/fees/sizing are what you meant, the bar count
matches the intended window, and the trade count supports the claim you are about to make.

Then read `avg_bars_held` and `exit_reasons` and check they match the strategy you described:

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
