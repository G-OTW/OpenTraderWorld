/**
 * The one place numbers, money, percentages, dates and byte sizes become text.
 *
 * Before this module there were seven fmtMoney, five fmtPct, four fmtDate and two
 * fmtBytes, and they disagreed on output — not just on style:
 *
 *   - fmtPct(0.05) meant "0.05%" in journal and "5.00%" in quant. Same name, 100x apart.
 *   - The null sentinel was — (U+2014) in journal and – (U+2013) in quant.
 *   - fmtMoney(-1234.5) put the minus inside the symbol ("$-1,234.50") in portfolios
 *     and outside it ("-$1,234.50") in journal.
 *   - fmtBytes(null) was "—" in settings and "0 B" in histdata; histdata capped at GB,
 *     so 5 TB read as "5120.0 GB".
 *
 * Rules that hold everywhere below:
 *
 *   NULL   A value that does not exist renders EM_DASH. One glyph, app-wide.
 *   MINUS  Negative numbers use U+2212 MINUS, never U+002D HYPHEN. In a tabular
 *          font U+2212 is cut to the digit advance width and lines up under the
 *          "+" of the rows above it; the hyphen is narrower and makes the column
 *          ragged. Intl emits the hyphen even with signDisplay:'always', so every
 *          Intl result is post-processed. (journal's fmtSignedMoney carried a
 *          comment claiming Intl gives U+2212. It does not — that was the bug.)
 *   UNIT   Percentages come in two incompatible units and no function can infer
 *          which it was handed. `fmtPct` takes percent (5 -> "5.00%");
 *          `fmtRatioPct` takes a fraction (0.05 -> "5.00%"). The unit lives in
 *          the name so a call site cannot silently be off by 100x.
 *   LOCALE Everything formats in the user's locale (`undefined` locale argument).
 *          Never pin a locale for display.
 */

/** The one "no value" glyph. U+2014. */
export const EM_DASH = '—';

/** U+002D HYPHEN -> U+2212 MINUS. Intl emits the hyphen; a tabular column wants the minus. */
const trueMinus = (s) => s.replace(/-/g, '−');

const isNil = (n) => n === null || n === undefined || (typeof n === 'number' && Number.isNaN(n));

/* ------------------------------------------------------------------ money -- */

/**
 * Short symbols for the currencies this app actually deals in. Anything outside this
 * table is delegated to Intl, which prints the ISO code as its own symbol
 * ("SEK 1,234.50") — it never drops the currency.
 *
 * CNY is "CN¥", not "¥": Intl disambiguates it from the yen, and so must we. The
 * previous hand-rolled table mapped both to "¥", making a CNY total unreadable as
 * anything but yen.
 *
 * A word-like symbol takes a non-breaking space before the digits ("CHF 1,234.50"),
 * exactly as Intl does; a single-glyph symbol must not ("$1,234.50"). U+00A0 rather
 * than a plain space so the amount can never wrap away from its currency.
 */
const SYMBOLS = {
  USD: '$',
  EUR: '€',
  GBP: '£',
  JPY: '¥',
  CHF: 'CHF ',
  CAD: 'C$',
  AUD: 'A$',
  CNY: 'CN¥'
};

/**
 * Minor-unit digits for a currency, straight from Intl's ISO 4217 data.
 *
 * Not a second table to maintain: JPY and KRW have no minor unit (0 digits), BHD has
 * three. Hardcoding `digits = 2` invented a half-yen that does not exist.
 */
function currencyDigits(ccy) {
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency: ccy }).resolvedOptions()
      .minimumFractionDigits;
  } catch {
    return 2; // ccy was empty/null — the only input Intl rejects outright
  }
}

/** Format via Intl, for currencies outside SYMBOLS. */
function intlMoney(n, ccy, signed) {
  try {
    return trueMinus(
      n.toLocaleString(undefined, {
        style: 'currency',
        currency: ccy,
        ...(signed ? { signDisplay: 'always' } : {})
      })
    );
  } catch {
    // No usable currency code at all. Show the number rather than nothing.
    return trueMinus(n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 }));
  }
}

/**
 * Money. `digits` overrides the currency's natural minor unit — pass it only when a
 * column genuinely needs more precision (a unit price), never to force 2 on JPY.
 *
 *   fmtMoney(1234.5, 'USD') -> "$1,234.50"
 *   fmtMoney(1234.5, 'JPY') -> "¥1,235"
 *   fmtMoney(1234.5, 'SEK') -> "SEK 1,234.50"
 *   fmtMoney(-1234.5)       -> "−$1,234.50"   (sign outside the symbol, U+2212)
 */
export function fmtMoney(n, ccy = 'USD', digits) {
  if (isNil(n)) return EM_DASH;
  const sym = SYMBOLS[ccy];
  if (!sym) return intlMoney(n, ccy, false);
  const d = digits ?? currencyDigits(ccy);
  const abs = Math.abs(n).toLocaleString(undefined, {
    minimumFractionDigits: d,
    maximumFractionDigits: d
  });
  return `${n < 0 ? '−' : ''}${sym}${abs}`;
}

/**
 * Money carrying PnL semantics — the sign is always printed, never implied by color.
 *
 * Red/green alone does not communicate gain vs loss: roughly 1 in 12 men cannot
 * separate the two hues, and the distinction survives neither a greyscale print nor
 * a screen reader. So the sign goes in the text, and it leads the symbol:
 * "−$1,234.50", not "$−1,234.50".
 */
export function fmtSignedMoney(n, ccy = 'USD', digits) {
  if (isNil(n)) return EM_DASH;
  const sym = SYMBOLS[ccy];
  if (!sym) return intlMoney(n, ccy, true);
  const d = digits ?? currencyDigits(ccy);
  const abs = Math.abs(n).toLocaleString(undefined, {
    minimumFractionDigits: d,
    maximumFractionDigits: d
  });
  return `${n < 0 ? '−' : '+'}${sym}${abs}`;
}

/**
 * Compact money for cramped cells: $263.1B, $172M. Always USD-symboled — the callers
 * (market-cap columns) are USD by construction.
 */
export function fmtCompactMoney(n, fallback) {
  if (isNil(n)) return fallback ?? EM_DASH;
  const abs = Math.abs(n);
  const sign = n < 0 ? '−' : '';
  if (abs >= 1e9) return `${sign}$${(abs / 1e9).toFixed(1)}B`;
  if (abs >= 1e6) return `${sign}$${(abs / 1e6).toFixed(0)}M`;
  if (abs >= 1e3) return `${sign}$${(abs / 1e3).toFixed(0)}K`;
  return `${sign}$${abs.toFixed(0)}`;
}

/* -------------------------------------------------------------- percentages -- */

/** Percent in, percent out. fmtPct(5) -> "5.00%". */
export function fmtPct(n, digits = 2) {
  if (isNil(n)) return EM_DASH;
  return `${trueMinus(n.toFixed(digits))}%`;
}

/**
 * Fraction in, percent out. fmtRatioPct(0.05) -> "5.00%".
 * Separate from fmtPct because no function can tell 5% from 500% by looking at the
 * number, and getting it wrong is a silent 100x error on screen.
 */
export function fmtRatioPct(n, digits = 2) {
  if (isNil(n)) return EM_DASH;
  return `${trueMinus((n * 100).toFixed(digits))}%`;
}

/** Percent with an explicit sign, for a PnL column. Same U+2212 reasoning as money. */
export function fmtSignedPct(n, digits = 2) {
  if (isNil(n)) return EM_DASH;
  return `${n < 0 ? '−' : '+'}${Math.abs(n).toFixed(digits)}%`;
}

/* ------------------------------------------------------------------ numbers -- */

/** Plain number, thousands-separated, at most `digits` decimals. */
export function fmtNum(n, digits = 2) {
  if (isNil(n)) return EM_DASH;
  return trueMinus(Number(n).toLocaleString(undefined, { maximumFractionDigits: digits }));
}

/** Number with a fixed number of decimals (a price column that must not ragged-align). */
export function fmtFixed(n, digits = 2) {
  if (isNil(n)) return EM_DASH;
  return trueMinus(
    Number(n).toLocaleString(undefined, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits
    })
  );
}

/**
 * Byte size. Runs to TB — the old histdata copy stopped at GB, so a 5 TB dataset
 * displayed as "5120.0 GB". Distinguishes 0 (a real size) from null (no value).
 */
export function fmtBytes(n) {
  if (isNil(n)) return EM_DASH;
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  let i = 0;
  let v = Math.abs(n);
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  // Whole bytes, and anything ≥100, read better without a decimal.
  const s = v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1);
  return `${n < 0 ? '−' : ''}${s} ${units[i]}`;
}

/* -------------------------------------------------------------------- dates -- */

/** A date-only "YYYY-MM-DD" string is midnight *local*, not UTC. Parse it as such. */
const parseDate = (v) => {
  if (v instanceof Date) return v;
  if (typeof v !== 'string') return new Date(v);
  return new Date(/^\d{4}-\d{2}-\d{2}$/.test(v) ? `${v}T00:00:00` : v);
};

const valid = (d) => d instanceof Date && !Number.isNaN(d.getTime());

/** "Mar 14, 2026". Accepts a Date, an ISO instant, or a "YYYY-MM-DD" date. */
export function fmtDate(v) {
  if (!v) return EM_DASH;
  const d = parseDate(v);
  if (!valid(d)) return EM_DASH;
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

/** "Mar 14, 2026, 8:30 AM". */
export function fmtDateTime(v) {
  if (!v) return EM_DASH;
  const d = parseDate(v);
  if (!valid(d)) return EM_DASH;
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit'
  });
}

/** "8:30 AM". */
export function fmtTime(v) {
  if (!v) return EM_DASH;
  const d = parseDate(v);
  if (!valid(d)) return EM_DASH;
  return d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
}

/** "Mar 2026", for a chart's month axis. */
export function fmtMonth(v) {
  if (!v) return EM_DASH;
  const d = parseDate(v);
  if (!valid(d)) return EM_DASH;
  return d.toLocaleDateString(undefined, { month: 'short', year: '2-digit' });
}

/**
 * A Date -> "YYYY-MM-DD" in the *local* timezone, for use as a storage/lookup key.
 *
 * `toISOString().slice(0,10)` converts to UTC first, and is wrong in both directions:
 * west of Greenwich at a late local hour it returns tomorrow (Los Angeles, 23:30 on
 * the 14th -> "2026-03-15"), east of it at an early hour it returns yesterday (Tokyo,
 * 02:00 on the 14th -> "2026-03-13"). The dashboard widgets ask for *today's* key, so
 * either slip shows the wrong day's data.
 *
 * Two of them dodged this with `toLocaleDateString('en-CA')` — correct, but only
 * because that locale happens to order its parts as ISO does. This reads the local
 * parts directly and does not depend on a locale at all.
 */
export function dateKey(v = new Date()) {
  const d = parseDate(v);
  if (!valid(d)) return '';
  const p = (x) => String(x).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Seconds -> "H:MM:SS". */
export function fmtDuration(totalSeconds) {
  const s = Math.max(0, Math.floor(totalSeconds ?? 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}

/**
 * Relative time, as an i18n key plus its parameters — never as an assembled string.
 *
 * Returning text would force this module to own translations. Instead it returns
 * { key, params } and the caller runs it through $t, so "3m ago" localizes with the
 * rest of the page. Keys live under `common.time.*`.
 *
 *   const r = relativeTime(iso);   ->   {$t(r.key, r.params)}
 */
export function relativeTime(v) {
  const d = parseDate(v);
  if (!valid(d)) return { key: 'common.time.justNow', params: {} };
  const secs = Math.max(0, (Date.now() - d.getTime()) / 1000);
  if (secs < 60) return { key: 'common.time.justNow', params: {} };
  const mins = Math.floor(secs / 60);
  if (mins < 60) return { key: 'common.time.minsAgo', params: { mins } };
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return { key: 'common.time.hoursAgo', params: { hrs } };
  const days = Math.floor(hrs / 24);
  return { key: 'common.time.daysAgo', params: { days } };
}
