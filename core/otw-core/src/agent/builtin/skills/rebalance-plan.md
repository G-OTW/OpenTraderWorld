# Planning a rebalance

## When to use

When weights have drifted from targets, or the user asks what it would take to get back to them.
Requires targets to exist — if none do, that is the conversation to have first.

## Procedure

1. `GET /api/portfolios/{id}/analytics?blocks=drift`. It returns current against target per
   bucket, the deviation, whether the bucket is outside its own band, and the trades that would
   close the gap (per bucket, then pro rata across the lines inside it).
2. `status: "unavailable"` with `no_targets` means the user has never written them down. **Do not
   infer targets from the current book** — ask, and offer `PUT /api/portfolios/{id}/targets`.
   Inferred targets make every drift look like a plan.
3. The band is already in the data: `out_of_band` is the server's answer, per bucket, using the
   tolerance the user set. Rebalancing on every wobble converts a small drift into a real cost,
   so a bucket inside its band is a bucket to leave alone.
4. Cash is a bucket, not a position: it comes from the ledger, and it is often where the drift
   actually is.
5. Estimate the cost of acting: spread, fees, and — where it applies — tax on realised gains via
   `POST /api/taxcalc/compute`. Compare that against the size of the drift being corrected.
6. Present the options: full rebalance, threshold-only, or new-money-only (directing future
   contributions instead of selling, which avoids realising anything).
7. Record it if asked: trades go through `POST /api/portfolios/assets/{asset_id}/operations`,
   and cash movements (a contribution funding the buy) through
   `POST /api/portfolios/{id}/operations` with a `deposit` kind.
   State exactly what you are about to write before writing it.

## Boundaries

Rebalancing has a tax dimension and a personal one. You show the arithmetic — drift, turnover,
cost, and what each option does to exposure — and say clearly that the tax figure is an estimate
from the calculator, to be verified with a professional. You do not tell them to sell.

## Pitfalls

- **Ignoring the cost of the fix.** A 2-point drift corrected with 1 point of cost and tax is not
  an improvement.
- **Rebalancing into a concentrated loser** because the model says "underweight". Say when the
  arithmetic and the judgement point different ways.
- **Forgetting new money.** Directing contributions is usually cheaper than selling.
- **Silent lot selection.** Which lots get sold changes the tax outcome; if the platform does not
  model it, say so rather than implying precision you do not have.

## Verification

Do the post-trade weights actually hit the targets within the band? Recompute and show them, do
not assert them.

## Report shape

A table: position, current %, target %, gap, action, estimated cost. Then the total turnover and
total cost, the options with their tradeoffs, and the tax caveat.
