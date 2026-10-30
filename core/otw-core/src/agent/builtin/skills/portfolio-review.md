# Reviewing a portfolio

## When to use

A periodic book review, or "how am I doing / am I too exposed / what's my risk". Rebalancing
mechanics are `rebalance-plan`; this is the diagnosis that comes first.

## Procedure

1. `GET /api/portfolios` for the list with valuation summaries.
2. `GET /api/portfolios/{id}/analytics?blocks=book,performance,risk,costs` — one call, and the
   server builds each substrate once for the blocks you asked for. Ask for what you will use:
   `blocks=book` alone reads the ledger and no market data at all, which is the right call on a
   fresh install.
3. **A block with `status: "unavailable"` has no answer, not a zero.** Its `missing` list says
   why (`no_history`, `too_short`, `no_benchmark`, `no_targets`). Report the gap, never a
   fabricated figure, and never treat a null as zero.
4. **Read `sample` on every block.** A Sharpe over 43 days and a Sharpe over a decade look
   identical without it; quoting the first as if it were the second is the most common way this
   review goes wrong.
5. `POST /api/portfolios/{id}/refresh` first if the valuation is stale — a review built on last
   month's prices describes last month.
6. `?blocks=benchmark` for performance against the risk taken. `risk_adjusted_edge_pct` is the
   line that answers "was the extra return worth the extra risk"; a book that beat its index can
   still be negative there.
7. `?blocks=drift` when targets exist, `?blocks=stress` for what a shock would do
   (`stress-and-diversify` owns that conversation).
8. Cross-check against the wider picture when the user has it: `GET /api/wealth/breakdown` puts
   the trading book in the context of net worth — a 40% position is a different animal at 5% of
   net worth than at 90%.

## What to look for

- **Concentration** — the largest position and the top three as a share of the book. State the
  numbers; let the user judge whether it is too much.
- **Correlation clusters** — five positions that all move together is one position wearing five
  hats. This is the finding users most often miss.
- **Currency exposure** they did not choose deliberately.
- **Drift** from whatever targets they set, if any exist.
- **Drawdown**, current and worst, against the tolerance they have stated before. The `episodes`
  table carries recovery time, which is what a drawdown actually costs: one 40% hole that took
  two years to fill and twelve 5% dips that filled in a week are not the same book.
- **Cash**. The `book` block reports it as a share, and an accidental 30% in cash is as much a
  decision as any position.
- **Costs**, from the `costs` block. The annual drag in basis points against average net worth is
  the number that compounds, and most users have never seen theirs.

## Boundaries

You do not recommend an allocation. You lay out **scenarios with tradeoffs**: "capping this at
15% would have cut peak drawdown from A to B and reduced return by C". The choice is theirs, and
saying so is not hedging — it is the correct division of labour.

## Pitfalls

- **Stale prices.** Always report the valuation timestamp.
- **Percentages without the base.** "Up 30%" of what, since when.
- **Ignoring cash.** Uninvested cash is a position — it has a currency and an opportunity cost.
- **Treating unrealised gains as realised.** Tax and slippage stand between the two.

## Verification

Do the exposure percentages sum to 100% including cash? If not, something is missing from your
picture — find it before reporting.

## Report shape

Total value and as-of timestamp, exposure table, the correlation observation, drawdown, then any
constraint breaches — each as a fact with its number, and the tradeoff framing for anything that
looks like a decision.
