# Risk metrics

## When to use

When asked how risky something is — an asset, a strategy's returns, or a whole book — and when
a performance number needs its risk counterpart before it means anything.

## What is available

- `POST /api/quant/single` — one series: VaR, CVaR, volatility, drawdown, plus a per-bar
  drawdown curve and a histogram. The response is large; pass `pick` (e.g.
  `["result.cvar","result.var"]`) when the user wants figures rather than a chart.
- `POST /api/quant/portfolio` — book level: correlations, volatility, drawdown. Also large; pick
  the fields you were asked about.
- `POST /api/quant/kelly` and `POST /api/quant/size` — sizing from an edge estimate.
- `POST /api/quant/asset-signals` — technical signals for one asset.

## What each number does and does not say

- **Volatility** is symmetric: it treats a 5% gain and a 5% loss as the same event. Nobody
  experiences them that way.
- **VaR** is a threshold, not a worst case: "95% VaR of 4%" means one day in twenty is *worse
  than* 4%, and says nothing about how much worse. Quoting VaR as a maximum loss is the single
  most common misuse.
- **CVaR** answers what VaR does not — the average of that bad tail. Prefer it when the question
  is "how bad does it get".
- **Max drawdown** is one realised path, not a distribution. It is a historical fact, not a
  bound: the next one can be larger, and usually the record only gets worse with more data.
- **Correlation** is an average over the sample and rises toward 1 in a crisis, which is exactly
  when diversification was supposed to help. Never present a correlation matrix as a promise.
- **Sharpe** assumes returns whose tails are thinner than markets actually have; it flatters
  strategies that sell tail risk (steady small wins, rare large losses).

## Pitfalls

- **Sample length.** All of these are estimates. A 90-day VaR from a calm quarter describes a
  calm quarter. State the window every time.
- **Kelly is fragile.** It maximises long-run growth *given* the edge you supplied — and the edge
  estimate is usually optimistic. Full Kelly on an overestimated edge is a fast way to ruin;
  half-Kelly or less is the normal practice. Say which fraction you used.
- **Annualising a short window** magnifies whatever noise it contained.

## Verification

Every figure needs its window and its sample size next to it. If you cannot say what period a
number came from, do not report it.

## Report shape

The metric, its window, and what it does not cover — in the same breath. When the user asked
"how risky", answer with the tail (CVaR, drawdown), not just the average (volatility).

## A portfolio's own risk

For a tracked portfolio, do not recompute any of this from prices:
`GET /api/portfolios/{id}/analytics?blocks=risk` already returns volatility, max drawdown,
Sharpe, Sortino, Calmar, the best and worst calendar periods, and the full underwater table with
recovery times.

Two things about that block are worth knowing before you quote it:

- Every figure is computed on the **deposit-adjusted** curve. On raw account value a monthly
  savings plan posts a heroic Sharpe and a drawdown of zero, because every contribution reads as
  a gain. That trap is already closed; do not reopen it by recomputing from balances.
- `periods_per_year` is **measured** off the portfolio's own curve, not taken from a table. The
  same daily series is ~252 periods a year on an exchange and ~365 on a 24/7 book, and quoting
  an annualized number against the wrong constant is off by 20%.

Under its sample floor the block returns `status: "unavailable"` with `too_short`. That is the
honest answer, and repeating the calculation yourself to produce a number anyway is not.
