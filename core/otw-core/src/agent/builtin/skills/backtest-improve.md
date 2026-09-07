# Improving a strategy that already ran

## When to use

A run exists and the question is "make it better". `backtest-run` is how you run one,
`backtest-validate` is how you decide whether a result means anything, and this is what happens
between two runs: reading the result as a *behaviour*, naming why it behaved that way, and
changing the one thing that follows from it.

## The stance

**A change is a hypothesis about the result, never a knob turn.** "Raise the stop and see" is
not an iteration; "the stop sits inside the noise the winners routinely take, so it should be
measured off ATR" is.

Aim every change at making the rules describe the market behaviour they claim to trade, not at
raising a number. That does not make the strategy profitable. It makes the result more likely
to survive data you have not seen, because it is no longer fitted to the accidents of this
window. A number raised by a change you cannot explain is a worse result than the one you had,
even when it is bigger.

Every variant still counts in the trial ledger of `backtest-validate`. Improving is the part
that inflates trial counts fastest, and it is the part most likely to be reported without one.

## Read the behaviour first

From one `view:"summary"` run: `stats.trades`, `stats.all.avg_bars_held`,
`stats.all.expectancy_pct`, `stats.all.payoff_ratio`, `stats.win_rate`, `stats.exit_reasons`,
`stats.max_drawdown_pct`, `stats.buy_hold_return_pct`. When the diagnosis needs excursions
(`mae` / `mfe`, per trade) take one `view:"full"` run over a *narrow* window rather than the
whole history.

Then compute the cost floor, because half of all "bad strategies" are just expensive ones:
one round trip costs roughly `2 × (fees.amount% + spread_pct/2 + slippage.value)` in percent of
notional. If `expectancy_pct` is at or below that, the strategy is a fee generator and no
parameter fixes it: only fewer, larger trades do.

## Diagnosis → lever

**Many trades, expectancy at the cost floor.** The rule is paying the spread to stay busy.
Lengthen the signal (bigger `period`, slower `fast`/`slow`), add a confirmation condition to the
entry group, or let the winners last longer (a real exit group instead of a tight
`take_profit_pct`). Fewer, bigger trades is the direction; verify the trade count stays above
the ~30 floor.

**`avg_bars_held` ≈ 1.** Not a finding, a configuration bug: `exit_on_reverse` with a
`crosses_*` entry (the response says so in `warnings`). Fix and rerun; the old numbers measured
nothing.

**Exits mostly `take_profit`, weak total return, `payoff_ratio` < 1.** The take-profit caps the
winners while the losers run to a wide stop. Look at the `mfe` of the take-profit trades: when
it routinely exceeds the TP distance, you are cutting winners at the point they start paying.
Widen or drop the TP and carry the trade with `trailing_stop` (a percent trail plus a
`breakeven_pct` step), or tighten the stop so the geometry stops being upside-down.

**Exits mostly `stop_loss`, low win rate.** The stop is inside the instrument's noise. The
measured answer is in the `mae` of the *winning* trades: the stop has to sit beyond the adverse
excursion winners routinely take. Prefer `stop_loss: {"kind":"atr","value":2,"period":14}` over
a flat percent, which means a different thing at every volatility level. If widening the stop is
what the data says, size it back down (`sizing.risk_pct` keeps risk per trade constant while the
stop moves).

**Good return, drawdown the user cannot hold.** A sizing and risk problem, not a signal problem:
`sizing.percent` down, `sizing.risk_pct`, `risk.max_exposure_pct`, `risk.max_drawdown_pct`,
`risk.max_daily_loss_pct`. Do not touch the entry rule for a drawdown complaint.

**Zero or near-zero trades.** Diagnose before loosening anything: an indicator name that does
not exist resolves to an undefined series and simply never fires, and so does a
`custom_indicator` id absent from `settings.indicators` (both are silent). Then check the
warm-up (`warmup_bars`, `trading_start_ts`), `filtered_bars` (the `filters` window),
`halted_bars` (a `risk` breaker), `skipped_min_size` / `skipped_margin`, and an `all` group
whose conditions cannot hold on the same bar.

**Strong in sample, dead out of sample.** Not an improvement problem. Stop; that is
`backtest-validate`'s verdict, and more tuning only burns the split.

**Loses to buy-and-hold.** Ask what the strategy is for before changing it. If the only honest
answer is a smaller drawdown, then the drawdown *is* the product: report it that way instead of
tuning the return upward.

## Coherence: entry and exit must describe one behaviour

The most common broken strategy is not badly tuned, it is self-contradictory, and no parameter
search fixes that. A momentum entry with a reversal exit buys strength and sells at the first
pause: the exit fires exactly when the premise is working.

- **Momentum / trend continuation.** Entries: moving-average crosses (`ema`, `sma`, `hma`),
  `macd` / `macd_hist`, `roc` or `momentum` above 0, `supertrend`, `donchian_upper` breaks,
  gated by `adx` above ~25. Coherent exits: a `trailing_stop`, the opposite trend event
  (cross back, `supertrend` flip, `donchian_lower`), a wide ATR stop. **Not** an
  `rsi`/`stoch_k`/`willr` overbought exit and not a tight fixed TP: both cut the trend that the
  entry paid to catch.
- **Mean reversion.** Entries: `rsi` / `willr` / `cci` / `stoch_k` at an extreme, `bb_lower` or
  `keltner_lower` touched, price stretched below a moving average. Coherent exits: the return to
  the middle (`bb_mid`, the moving average, `rsi` back through 50), a fixed take-profit, a time
  stop. **Not** a trailing stop, which gives the reversion back, and never "let it run".
- **Breakout / volatility expansion.** Entries: `donchian_upper`, `bb_upper` or
  `keltner_upper` with rising `atr` / `stddev`. Coherent exits: trail behind the channel
  (`donchian_mid`, `donchian_lower`) or an ATR trail. Not a small fixed TP.
- **Filters, not entries**: `adx` (is there a trend at all), `atr` / `stddev` (regime), `mfi` /
  `obv` (participation), `vwap` (session anchor). They belong in the entry group as a `above` /
  `below` state, never as the thing that fires the trade.

Test: state the strategy in one sentence — "buy strength, ride it, leave when it stops" or "buy
a stretch, sell it back to the middle". If the entry half and the exit half of that sentence
disagree, change the **family**, not the parameters, and say which side you changed to match the
other. Proposing the matching exits for the entry the user already has is usually the smaller,
more honest change.

The operator has to match the role too: `crosses_above` / `crosses_below` / `cross` are events
and belong to entries; `above` / `below` / `rising` / `falling` are states and belong to filters
and to `exit_on_reverse`. A filter written as a cross holds on one bar and blocks every other.

## How to change it

1. **One change per run**, with the same dataset, window and costs as the baseline. Two changes
   at once and you have learned nothing about either.
2. **Structure before parameters**: coherence, configuration bugs and cost realism first. Tuning
   a period on an incoherent rule is fitting noise with extra steps.
3. **Sweep the lever, do not pick a point.** `POST /api/backtest/sweep` with one axis over a
   range (max 4 axes, 64 trials) shows whether the lever has a plateau or one lucky value. A
   value that only works alone is noise, and the sweep reports `deflated_sharpe` and
   `selection_explains_it` so the trial count travels with the answer.
4. **Keep the baseline run_id** and name it (`POST /api/backtest/runs`) so every comparison is
   against a run that still exists.

## Not improvements

- A condition added to remove losing trades you have already seen. That is curve fitting with a
  clean face, unless the condition was stated as a hypothesis before the run.
- Shrinking the window to the part that worked, or dropping the losing side.
- More leverage or bigger sizing: the edge is unchanged and the drawdown scales with it.
- Anything reported without saying how many variants it took.

## Verification

Before calling a change an improvement: same window, same costs, same datasets; the trade count
still supports a claim; the neighbouring parameter values did not collapse; in-sample and
out-of-sample both moved, not just in-sample; and you can state the diagnosis the change came
from, not just the number that went up.

## Report shape

Baseline numbers, the diagnosis in one sentence, the single change, the new numbers beside the
baseline over the identical window, then the running trial count. Close with what this does and
does not establish: a coherent strategy that survives its neighbourhood is not a profitable one,
it is one whose result is worth testing further.
