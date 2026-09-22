# Testing an idea

## When to use

When the user proposes a strategy, a rule, or a hunch — "does X work?", "should I buy when Y?",
"is there an edge in Z?". Use it *before* touching the backtest engine.

## The point

Most trading ideas arrive as a vibe. Your job is to turn one into a claim that can lose, decide
in advance what losing looks like, and only then measure. Deciding what counts as success after
seeing results is not testing, it is decorating.

## Procedure

1. **Restate it as a falsifiable claim.** "Momentum works on BTC" is not testable. "A 20/50 EMA
   crossover, long-only, on BTCUSDT daily, beats buy-and-hold risk-adjusted after 0.1% fees and
   0.05% slippage" is. Read it back to the user and get agreement — you will often find you were
   about to test a different idea than the one they meant.
2. **Write the decision rule first.** What result would make you say no? Name the threshold:
   trade count floor, minimum out-of-sample return, maximum drawdown, whether it must beat
   buy-and-hold or merely match it with less risk.
3. **Name the data.** Instrument, timeframe, date range, and where it stops. Check it exists
   (`data-acquire`) before designing around it.
4. **List what could produce a false positive.** Survivorship, a single regime, one lucky trade
   carrying the whole result, a parameter you picked because you already knew the answer.
5. **Run it** (`backtest-run`) — once, at the parameters you pre-registered.
6. **Judge it** (`backtest-validate`) against the rule you wrote in step 2, not against how the
   number feels.
7. **Report the verdict in the user's own terms**, including the "no" case. An idea killed
   cheaply is a good outcome — it costs one afternoon instead of a year of live trading.

## Pitfalls

- **Sliding the goalposts.** If you find yourself explaining why a bad result is actually
  encouraging, stop and re-read step 2.
- **Testing the idea you can measure instead of the one they asked.** If the data cannot answer
  the real question, say so rather than answering a nearby one.
- **One test, many implicit choices.** Timeframe, instrument, and date range are parameters too.
  Trying three instruments and reporting the best is three trials, not one.
- **Confusing "no edge found" with "no edge exists".** Your test had limited power; say what it
  could and could not have detected.

## Verification

Before reporting: did you write the decision rule before seeing the result? If not, the test is
exploratory — label it that way and treat any positive as a hypothesis for a fresh test, not a
finding.

## Report shape

The claim as tested, the decision rule, the data, the result, the verdict against the rule, and
what would change it. Then what you would test next, if anything.
