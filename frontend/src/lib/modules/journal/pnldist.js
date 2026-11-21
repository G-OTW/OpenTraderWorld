// Per-trade PnL histogram: how many trades landed in each money bucket.
//
// The analytics endpoint sends the raw sorted PnL of every closed trade, so the reader
// can change the bucket width without a round trip. Zero is always a bucket boundary,
// which is the whole point: no bar may mix a winner with a loser.
//
// Output rows carry the same shape as the server's `Bucket`, so `DistChart` draws them
// without knowing where they came from. `lo`/`hi` null = open-ended edge bucket.

/** Value at `q` of an already sorted array (linear interpolation). */
function quantile(sorted, q) {
  if (sorted.length === 0) return 0;
  const i = (sorted.length - 1) * q;
  const lo = Math.floor(i);
  const hi = Math.ceil(i);
  return lo === hi ? sorted[lo] : sorted[lo] + (sorted[hi] - sorted[lo]) * (i - lo);
}

/**
 * A readable bucket width for `values`: aim for ~24 bars over the bulk of the data
 * (2nd to 98th percentile, so one freak trade cannot flatten the histogram) and snap
 * to a 1 / 2 / 2.5 / 5 x 10^n graduation.
 */
export function autoStep(values) {
  if (!values.length) return 1;
  let span = quantile(values, 0.98) - quantile(values, 0.02);
  // A tight bulk (or a single value) leaves nothing to divide: fall back to the full
  // range, then to a unit step.
  if (!(span > 0)) span = values[values.length - 1] - values[0];
  const raw = span / 24;
  if (!(raw > 0)) return 1;
  const mag = 10 ** Math.floor(Math.log10(raw));
  for (const m of [1, 2, 2.5, 5]) {
    if (raw <= m * mag) return m * mag;
  }
  return 10 * mag;
}

/**
 * Bucket `values` (sorted ascending) into bars of width `step`.
 *
 * `maxBins` caps the axis: a 1-unit step over a five-figure range would otherwise draw
 * thousands of bars. Past the cap the window shrinks to the 1-99% range and the two
 * edge buckets swallow everything outside, which is what the open `lo`/`hi` mean.
 */
export function pnlBuckets(values, step, maxBins = 121) {
  if (!values.length || !(step > 0)) return [];
  const idx = (v) => Math.floor(v / step);
  let lo = idx(values[0]);
  let hi = idx(values[values.length - 1]);
  if (hi - lo + 1 > maxBins) {
    lo = idx(quantile(values, 0.01));
    hi = idx(quantile(values, 0.99));
    if (hi - lo + 1 > maxBins) {
      lo = idx(quantile(values, 0.5)) - Math.floor((maxBins - 1) / 2);
      hi = lo + maxBins - 1;
    }
  }

  const out = [];
  for (let i = lo; i <= hi; i++) {
    out.push({
      key: String(i),
      trades: 0,
      wins: 0,
      net: 0,
      avg: 0,
      win_rate: null,
      lo: i * step,
      hi: (i + 1) * step
    });
  }
  for (const v of values) {
    const b = out[Math.min(hi, Math.max(lo, idx(v))) - lo];
    b.trades += 1;
    if (v > 0) b.wins += 1;
    b.net += v;
  }
  for (const b of out) {
    if (b.trades > 0) {
      b.avg = b.net / b.trades;
      b.win_rate = (b.wins / b.trades) * 100;
    }
  }
  if (idx(values[0]) < lo) out[0].lo = null;
  if (idx(values[values.length - 1]) > hi) out[out.length - 1].hi = null;
  return out;
}
