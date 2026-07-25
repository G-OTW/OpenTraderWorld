# Reviewing a portfolio

## When to use

A periodic book review, or "how am I doing / am I too exposed / what's my risk". Rebalancing
mechanics are `rebalance-plan`; this is the diagnosis that comes first.

## Procedure

1. `GET /api/portfolios` for the list with valuation summaries.
2. `GET /api/portfolios/{id}` for detail — positions, valuation, history. **This response is
   large**: use `pick` for the fields you need rather than pulling it whole.
3. `POST /api/portfolios/{id}/refresh` first if the valuation is stale — a review built on last
   month's prices describes last month.
4. Compute exposure: by position, by asset class, by currency, and by sector where known.
   Percentages of the total, not just absolute values.
5. `POST /api/quant/portfolio` for correlation and drawdown across the book.
6. Cross-check against the wider picture when the user has it: `GET /api/wealth/breakdown` puts
   the trading book in the context of net worth — a 40% position is a different animal at 5% of
   net worth than at 90%.

## What to look for

- **Concentration** — the largest position and the top three as a share of the book. State the
  numbers; let the user judge whether it is too much.
- **Correlation clusters** — five positions that all move together is one position wearing five
  hats. This is the finding users most often miss.
- **Currency exposure** they did not choose deliberately.
- **Drift** from whatever targets they set, if any exist.
- **Drawdown**, current and worst, against the tolerance they have stated before.

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
