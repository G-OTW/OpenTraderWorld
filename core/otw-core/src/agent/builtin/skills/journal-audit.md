# Auditing the trading journal

## When to use

On request, monthly, or whenever the user asks what their numbers say about how they trade. This
is about *their behaviour*, which is what a journal is uniquely able to show.

## Procedure

1. `GET /api/journal/breakdown` with `group` — `day`, `week`, `month`, `strategy`, `symbol` — is
   the workhorse. Query it several ways rather than pulling every trade.
2. `GET /api/journal/trades` with filters when you need the individual rows.
3. `GET /api/journal/calendar` for the day-by-day realised P&L shape.
4. Establish the sample first: how many closed trades, over what period. **Everything below is
   conditional on that number.**
5. Cut the data by: strategy, symbol, time of day/week, holding period, and long vs short. Look
   for where the money actually comes from and where it leaks.
6. Look at the distribution, not just the average — one outlier trade carrying an entire year is
   the single most common finding, and it means the average is fiction.

## What to look for

- **Concentration of P&L**: does removing the best trade change the verdict? If yes, say so.
- **Win rate versus payoff**: a 40% win rate with 3:1 payoff is healthier than 70% with 0.3:1.
  Report them together or neither.
- **Time-of-day / day-of-week** patterns, when the sample supports it.
- **Holding-period drift**: winners cut short and losers held long shows up as an asymmetry
  between average win duration and average loss duration.
- **Rule adherence**, when the journal records the plan alongside the outcome.

## Pitfalls

- **Small samples everywhere.** Slicing 60 trades five ways leaves cells of 12. Say when a cut is
  too thin to support a conclusion — most of them will be.
- **Fees and currency.** Check whether the numbers are net; check the display currency.
- **Open positions** distort period stats. Say whether you counted realised only.
- **Confusing correlation with cause.** "You lose on Fridays" is a description of 9 trades, not a
  reason to stop trading Fridays.
- **Post-hoc pattern hunting.** With enough cuts, something always looks significant. Prefer
  patterns the user already suspected, and label the rest as exploratory.

## Boundaries

Report the pattern and the user's own rule if the journal records one. Do not turn a behavioural
observation into a trading instruction, and never encourage trading back a loss.

## Verification

State the sample size next to every claim. If a cut has fewer than ~20 trades, present it as an
observation, not a finding.

## Report shape

Sample and period first. Then two or three findings that the numbers genuinely support, each
with its cut and its count. One behavioural observation. Nothing that reads as a signal.
