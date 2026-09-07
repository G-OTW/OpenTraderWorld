# Stress testing a book, and what to do about it

## When to use

"What happens to my portfolio if…" — a market shock, a rate move, a repeat of 2008 or 2020 — and
the diversification conversation that follows it. `portfolio-review` is the diagnosis of what is
there; this is the diagnosis of what a shock would do to it.

## The one rule

**Coverage before impact, always.** A stress result covers the share of the book whose
sensitivity could actually be measured. `−11.8% across 74% of the book` and `−11.8%` are
different statements, and only the first one is true. Lead with the coverage, name the positions
that are not in it, and never present the headline as if it described everything.

The engine refuses to guess, so you must not guess on its behalf:

- A position with no stored candles, too short a history, or a fit with no explanatory power gets
  **no beta**. It appears in `unexplained`. Do not assume it moves with the market "roughly like
  an index" — that assumption is the entire reason retail stress numbers are worthless.
- A historical replay skips instruments that did not exist in the window. A crypto book has
  almost nothing in 2008, and saying so is the finding.
- A factor with no stored proxy contributes nothing and is listed in `missing_factors`. That leg
  did not happen; do not describe it as if it had.

## Procedure

1. `GET /api/portfolios/{id}/analytics?blocks=stress` — the readiness report. It says which
   factors resolved to a real instrument, which positions have a measured sensitivity, and which
   do not. Read it before running anything.
2. `GET /api/portfolios/scenarios` for what is available: factor shocks (S&P −20%, rates
   +200 bps, oil +40%, and the composites) and historical replays (2008, 2020, 2022, 2018 Q4).
3. `POST /api/portfolios/{id}/stress` with `{"scenario_id": …}`, or with `legs` for a custom
   shock, or `from`/`to` for a custom replay. It writes nothing.
4. Run **more than one**. A single scenario is an anecdote; three tell you what the book is
   actually exposed to, and they usually all point at the same factor.
5. `?blocks=drift` for the target allocation, and `?blocks=risk` for what the book has already
   lived through. A drawdown the user survived is better evidence than a simulated one.

## Reading the result

- **Name the factor, not the positions.** Ten holdings down 20% is usually one exposure wearing
  ten tickers. `by_factor` says which; that sentence is the whole value of the exercise.
- **Compare against the realised drawdown.** If the scenario is milder than what the book has
  already been through, say so — it reframes the number from alarming to ordinary.
- **Cash has a beta of zero.** A book that is 30% cash absorbs a third of any shock, and that is
  worth stating: it is often the only diversification already in place.
- A rate leg reaches the book through a duration you can read on the leg. If it looks wrong for
  the instruments held, that is a real objection, not a rounding detail.

## Proposing diversification

Only after the above, and under three constraints:

1. **Never propose a trade the drift table does not already show as out of band.** A stress
   result is a reason to look at the allocation, not a licence to redesign it.
2. **Address the factor.** "Add bonds" is only useful if the measured betas say the book's
   problem is equity beta. If the exposure is currency or credit, say that instead.
3. **Cost the move.** Spread, fees, and realised gains via `POST /api/taxcalc/compute` where it
   applies. A diversification that costs more than the risk it removes is not one.

## Boundaries

You do not recommend an allocation, you lay out tradeoffs with numbers: "capping the correlated
cluster at 15% would have taken this scenario from A to B, at C in realised gains". The choice
is the user's.

Never quote a metric the endpoint returned with `status: "unavailable"` — it means the data to
compute it does not exist, not that the answer is zero.
