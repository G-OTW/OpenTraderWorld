/** Number formatting for the backtest screens and reports. No imports, so a Node test loads it. */

export const fmtNum = (n, d = 2) =>
  n == null || Number.isNaN(n)
    ? '–'
    : Number(n).toLocaleString(undefined, { maximumFractionDigits: d });

/** A fraction the engine stores (0.7, 0.0005) as the percent a reader expects ("70%",
 *  "0.05%"). Spread, slippage, stops and the in-sample split are all fractions: printing the
 *  raw number with a "%" after it turned a 70% in-sample split into "0.7%". */
export const fmtFrac = (f, d = 4) =>
  f == null || Number.isNaN(Number(f)) ? '–' : `${fmtNum(Number(f) * 100, d)}%`;

/** The in-sample / out-of-sample split as "70% / 30%": `oos_split_pct` is the fraction kept
 *  in-sample, the tail is out-of-sample. */
export const fmtSplit = (f) => `${fmtFrac(f)} / ${fmtFrac(1 - Number(f))}`;
