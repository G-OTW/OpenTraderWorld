# Writing a report

## When to use

For a deliverable the user will keep: a company study, a portfolio review, a backtest
conclusion, a research brief. Not for a two-line answer — a short question deserves a short
answer, and wrapping one in report structure is noise.

## Where it lands

Chat for anything the user reads once. An `editor` document when it is reference material, when
it is long enough to scroll, or when they asked for a document. Ask if it is genuinely
ambiguous; otherwise pick and say which you picked.

## Structure

1. **The answer, first.** One paragraph, or a single sentence when the question had one. A
   reader who stops after it should have the conclusion, not the preamble.
2. **What the answer rests on.** The figures, each traceable to where it came from — endpoint,
   dataset, date range, sample size. A table beats prose for more than three numbers.
3. **Limits.** Mandatory, never omitted, never softened. What the data does not cover, what you
   assumed, what would change the conclusion. If the sample is thin, this is the section that
   says so in plain words.
4. **What would falsify it.** For any claim about behaviour or performance: the observation
   that would prove it wrong. A claim nothing could falsify is not a finding.

## Rules

- **Every number is sourced.** If you cannot say which call produced a figure, it does not go
  in. This includes figures you computed — say what you computed them from.
- **Reference runs by id.** A backtest conclusion cites its `run_id`s so the user can reopen the
  exact run. Name runs worth keeping so they survive the history cap.
- **Distinguish measured from assumed.** Fees, slippage, and rates you supplied are assumptions;
  label them as such next to the results they produced.
- **No decoration.** No "excellent results", no "strong performance". State the numbers and what
  they imply. If the finding is negative, the report says the finding is negative — a failed
  test cleanly reported is a useful deliverable.
- **Date it.** Anything built on market data or news is a snapshot; say as of when.

## Verification

Before sending, re-read your own report and check each number against a tool result in this
conversation. Any figure you cannot locate came from your own head — remove it or go fetch it.

Then check the limits section actually contains the weakest part of the analysis. If it reads
like a formality, you have hidden the real caveat somewhere else.
