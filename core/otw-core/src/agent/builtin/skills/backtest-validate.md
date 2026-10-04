# Deciding whether a backtest means anything

## When to use

Before you call any strategy good, promising, or worth trading. Running the simulation is
`backtest-run`; this is the part that decides what the numbers are worth. A positive verdict
without this is not a verdict, it is a number.

## The stance

A backtest is a measurement of the past taken with a device you built after seeing the past.
Your job is to work out how much of the result is the market and how much is the device. Assume
the device until shown otherwise.

## Mandatory before any positive verdict

**1. Say what would falsify it, first.** Before running: the hypothesis, the parameter values
you will try, and the threshold that counts as success. Written in the reply or an `editor`
doc. Deciding afterwards what counts as good is how noise becomes a strategy.

**2. Keep a trial ledger.** Every run: parameters, `run_id`, in-sample and out-of-sample stats.
Report it. **If you tried twelve variants and report the best one without saying you tried
twelve, the number you are reporting is meaningless** — the best of twelve random strategies
also looks good.

Use `POST /api/backtest/sweep` for any grid rather than looping `/run` yourself: one tool
round instead of N, and it returns the ledger as data you cannot forget to mention. Body is a
normal run body plus `grid` = `{"<settings.path>": [values…]}` (max 4 axes, 64 trials). It
comes back with every trial, the spread of trial Sharpes, and `deflated_sharpe` — the Sharpe
the best of N *worthless* strategies would be expected to reach given how much these trials
varied. When `selection_explains_it` is true, the winner is inside the noise of having tried N
times: report no evidence of an edge. Report the spread next to any headline number, always.

Simulations are budgeted per conversation, and a sweep charges one per trial. Decide the grid
before you spend it — a plan you can state is also the pre-registration step 1 asks for.

**3. Out-of-sample, looked at once.** Set `oos_split_pct` (0.2–0.3 — a *fraction*). Report
in-sample and out-of-sample side by side. If the user asks to tune after seeing the OOS result,
say plainly that this split is now burned: it has become part of the fitting, and the next
honest test needs data neither of you has looked at.

**4. Enough trades.** Under ~30 trades in the tested window, report **"underpowered"** rather
than a verdict, and say what the floor was. A 70% win rate over 10 trades is not a finding.

**5. Compare to doing nothing.** `buy_hold_return_pct` comes back with every run. A strategy
that returns 30% while buy-and-hold returned 2200% is not a strategy; it is an expensive way
to avoid owning the asset. Say so in those terms. Also compare drawdown — sometimes that is
the honest reason to prefer it, and then *that* is the finding.

**6. Consistency across time, not just in aggregate.** Split the history with `from`/`to` and
run each part with identical settings. Adjacent windows partition the data
(`"from":"2019-01-01","to":"2022-06-30"` then `"from":"2022-07-01","to":"2025-12-31"`).
A result that lives in one regime and dies in the next is a regime observation, not an edge.
Report each window's numbers.

**7. Walk it forward.** Roll the window: fit on window N, check on window N+1, repeat. The same
`from`/`to` fields do this. It costs one run per step, so pick a step size that fits your
budget and say how many steps you did.

**8. Parameter plateau, not peak.** Run the neighbours of your chosen parameters (18/45, 22/55
for a 20/50). If they collapse while the centre shines, you found a hole in the noise. Report
the neighbourhood.

**9. Costs, doubled.** Rerun the finalist with 2× fees and 2× slippage. If the edge dies, it
was a fee-schedule artifact — report it as fragile. Remember `slippage.value` is a fraction
(`0.0005` = 0.05%) and `fees.amount` is a percent (`0.1` = 0.1%).

**10. Beat a dumb baseline.** Compare against something with no idea in it — buy-and-hold above,
or the same strategy with the entry rule inverted. If the baseline matches it, there is no edge
to discuss.

## Never

- Tune on the out-of-sample block.
- Drop a losing period because it was "unusual". Every period is unusual.
- Change the instrument or date range after seeing results and keep the old trial ledger.
- Report a Sharpe without the trade count and the window it came from.
- Use the word "optimised" without saying how many combinations were tried.

## Optional depth

`POST /api/backtest/runs/{id}/montecarlo` with **`view:"summary"`** resamples the run's realized
per-trade P&L into many equity paths: percentile bands for final equity and max drawdown, risk of
ruin, probability of loss. Needs at least 2 trades. The default `"full"` also returns the equity
fan, two histograms and the realized curve — those exist to draw a chart and will flood your
context.

It answers "how else could this sequence of trades have gone", **not** "does the edge exist" — it
resamples the trades you already have, so a fitted strategy yields confident-looking bands around
a fitted result. Useful for sizing and ruin risk; not evidence of an edge.

## Never compute a simulation yourself

If an endpoint you want is not in the catalog, **say so and stop**. Do not pull the per-trade list
and do the statistics in your head. Numbers you produce that way have no tool call behind them —
they cannot be checked, reproduced, or trusted, and they will be wrong in ways nobody can see.
Reporting "the platform cannot do X" is a useful answer; inventing X is not.

## Verification

Before reporting: can you state the trial count, the OOS figure, the trade count, the cost
assumptions, and the buy-and-hold comparison? If any is missing, you are not done.

## Report shape

Verdict first, in plain words — "does not survive out of sample", "underpowered, needs more
trades", "survives, with these caveats". Then the trial ledger, the IS/OOS table, the window
breakdown, costs used, and the `run_id`s. A negative verdict cleanly reported is a good
deliverable; a positive one without the ledger is not.
