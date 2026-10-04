# Reviewing a trading session

## When to use

At session end, or the morning after. The value here is honesty about process, not a P&L recap
the user can already see.

## Procedure

1. `GET /api/journal/trades?from=…&to=…` for the session's trades.
2. `GET /api/journal/breakdown?from=…&to=…` for the aggregate.
3. `GET /api/trader/board` for which routine items were actually checked.
4. `GET /api/mindset/day` for what they wrote before the session, if anything.
5. Compare **plan against execution**: did they trade what they said they would, at the size they
   said, with the exits they said?

## What to report

- Realised P&L for the session, with the trade count.
- Rule adherence: which of their own rules held and which did not. This is the point of the
  exercise.
- One behavioural observation, supported by the data — not five.
- Tomorrow's single focus item, drawn from the observation.

## Boundaries

A losing session is a losing session; say it plainly and without consolation. A winning session
that broke the rules is **worse** than a small loss that followed them, and that is the most
valuable thing you can point out — say it. Never suggest making it back.

## Pitfalls

- **Outcome bias.** Judging the process by the result is the exact error the journal exists to
  correct. A good decision can lose.
- **Piling on.** One observation, actionable. A list of failings after a bad day gets ignored.
- **Inventing a narrative** for what was noise. Some sessions have no lesson; say so.
- **Praising a win.** Report it; do not celebrate it. Both directions of sycophancy distort.

## Verification

Every number from the journal. The behavioural observation must point at a specific trade or a
specific rule, not a mood.

## Report shape

P&L and trade count, rule adherence, one observation with its evidence, one focus item.
