/** Price formatting for the chart, shared by everything that prints one.
 *
 * Precision follows the *instrument*, not the magnitude: an index quoted in whole points must
 * not sprout cents, and a token at 0.00000013 must not be rounded away. We read the decimals
 * the bars actually use (float noise stripped by the toPrecision round-trip) and keep the
 * deepest one, so every price on the chart, in the readout and in the trade list is written
 * the same way.
 */

export const MAX_PRICE_DIGITS = 10;

/** Shortest decimal a 32-bit float reads back as: 211.8699951171875 is 211.87. Sources that
 *  keep prices as 32-bit floats (Yahoo) send the whole binary expansion, which ten
 *  significant digits do not strip. Stored bars come back cut to 15 digits
 *  (211.869995117188), so a value that close to a 32-bit float counts as one; anything
 *  else is kept. */
function unfloat32(x) {
  const f = Math.fround(x);
  if (Math.abs(f - x) > Math.abs(x) * 1e-14) return x;
  for (let p = 1; p < 10; p++) {
    const v = Number(x.toPrecision(p));
    if (Math.fround(v) === f) return v;
  }
  return x;
}

/** Decimals a number is really written with, ignoring binary-float tails. */
export function decimalsOf(x) {
  if (!Number.isFinite(x) || x === 0) return 0;
  const s = String(Number(unfloat32(Math.abs(x)).toPrecision(10)));
  const e = s.indexOf('e');
  if (e < 0) {
    const dot = s.indexOf('.');
    return dot < 0 ? 0 : s.length - dot - 1;
  }
  const exp = Number(s.slice(e + 1));
  const dec = (s.slice(0, e).split('.')[1] ?? '').length;
  return Math.max(0, dec - exp);
}

/** Deepest decimal count over a bar series ({o,h,l,c} arrays), sampled from the newest end. */
export function priceDigitsOf(view) {
  const n = view?.ts?.length ?? 0;
  if (!n) return 2;
  const step = Math.max(1, Math.ceil(n / 500));
  let d = 0;
  for (let i = n - 1; i >= 0 && d < MAX_PRICE_DIGITS; i -= step) {
    d = Math.max(
      d,
      decimalsOf(view.o[i]),
      decimalsOf(view.h[i]),
      decimalsOf(view.l[i]),
      decimalsOf(view.c[i])
    );
  }
  return Math.min(d, MAX_PRICE_DIGITS);
}

/** Bars a tick is read from at the least: fewer could all sit on a coarser grid by chance. */
const TICK_MIN_BARS = 20;

const gcd = (a, b) => {
  while (b) [a, b] = [b, a % b];
  return a;
};

/** The price increment the bars trade on, read from the prices themselves: every O/H/L/C
 *  written as an integer at the series' precision, and their common divisor. A future on
 *  0.25 only ever ends in .00/.25/.50/.75, so the divisor is 25 hundredths. One price off
 *  the grid (adjusted data) brings it down to the plain precision, never to a wrong grid.
 *  Null when the bars are too few or too large to count exactly. */
export function priceTickOf(view) {
  const n = view?.ts?.length ?? 0;
  if (n < TICK_MIN_BARS) return null;
  const digits = priceDigitsOf(view);
  const scale = 10 ** digits;
  let g = 0;
  for (let i = n - 1; i >= 0 && g !== 1; i--) {
    for (const v of [view.o[i], view.h[i], view.l[i], view.c[i]]) {
      if (!Number.isFinite(v)) continue;
      const k = Math.abs(Math.round(v * scale));
      if (k > Number.MAX_SAFE_INTEGER) return null;
      g = gcd(g, k);
    }
  }
  return g > 0 ? Number((g / scale).toFixed(digits)) : null;
}

/** Deepest decimal count over a plain list of prices. */
export function priceDigitsOfValues(values) {
  let d = 0;
  for (const v of values) {
    d = Math.max(d, decimalsOf(v));
    if (d >= MAX_PRICE_DIGITS) break;
  }
  return Math.min(d, MAX_PRICE_DIGITS);
}

/** Zeros written as a subscript, the way exchanges and CoinMarketCap write micro-caps. */
const SUBSCRIPT = '₀₁₂₃₄₅₆₇₈₉';
const subscript = (n) =>
  String(n)
    .split('')
    .map((d) => SUBSCRIPT[Number(d)])
    .join('');

/** Below this many zeros after the decimal point, a price is written out in full. */
const COMPACT_FROM = 4;

/**
 * Compact form of a very small price: `0.0₅4549` is 0.000004549 — the subscript counts the
 * zeros. Ten decimals of a micro-cap never fit an axis gutter, and "0.0000" on every tick
 * says nothing; this keeps four significant digits in the width of a normal label.
 *
 * @returns the compact string, or null when the price is ordinary enough to write in full.
 */
export function compactPrice(v, sig = 4) {
  const a = Math.abs(v);
  if (!Number.isFinite(v) || a === 0 || a >= 0.001) return null;
  // Round first, then read the exponent: 9.9999e-6 must come out as 0.0₄1000, not 0.0₅1000.
  const [mantissa, e] = a.toExponential(sig - 1).split('e');
  const zeros = -Number(e) - 1;
  if (zeros < COMPACT_FROM) return null;
  // Trailing zeros in the mantissa claim a precision the price does not have: 1.3e-7 is
  // 0.0₆13, not 0.0₆1300.
  const digits = mantissa.replace('.', '').replace(/0+$/, '') || '0';
  return `${v < 0 ? '-' : ''}0.0${subscript(zeros)}${digits}`;
}

/** Fixed-precision formatter: every price gets exactly `digits` decimals. */
export function makePriceFmt(digits) {
  const f = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits
  });
  return (v) => (Number.isFinite(v) ? (compactPrice(v) ?? f.format(v)) : '');
}

/** Axis formatter: same precision as a *ceiling*, trailing zeros dropped, so a tick labelled
 *  63 000 lines up under a tag reading 63 000.00 and still fits the gutter. */
export function makeAxisFmt(digits) {
  const f = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: digits
  });
  return (v) => (Number.isFinite(v) ? (compactPrice(v) ?? f.format(v)) : '');
}
