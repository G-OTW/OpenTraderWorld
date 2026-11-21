// Data semantics shared by the Fundamentals and Quant dashboard cards.
export const COMPANY_METRICS = {
  market_cap: ['marketCap', 'money'], pe: ['pe', 'multiple'],
  ev_ebitda: ['evEbitda', 'multiple'], ps: ['ps', 'multiple'], pb: ['pb', 'multiple'],
  operating_margin: ['opMargin', 'percent'], net_margin: ['netMargin', 'percent'],
  gross_margin: ['grossMargin', 'percent'], fcf_yield: ['fcfYield', 'percent'],
  dividend_yield: ['divYield', 'percent'], roe: ['roe', 'percent'],
  roic: ['roic', 'percent'], debt_equity: ['debtEquity', 'multiple'],
  current_ratio: ['currentRatio', 'multiple']
};
export const DEFAULT_METRICS = ['market_cap', 'pe', 'operating_margin', 'fcf_yield'];

export function percentChange(current, prior) {
  return Number.isFinite(current) && Number.isFinite(prior) && prior !== 0
    ? (current - prior) / Math.abs(prior) * 100 : null;
}

/** Calendar identity matters: four reported rows are not necessarily four quarters. */
export function statementYoY(rows, last, line) {
  if (!last) return null;
  const prior = rows.find((r) => Number(r.fiscal_year) === Number(last.fiscal_year) - 1
    && r.fiscal_period === last.fiscal_period);
  // A loss-to-profit transition is best described explicitly, not as ordinary growth.
  const a = last.lines?.[line], b = prior?.lines?.[line];
  return b > 0 ? percentChange(a, b) : null;
}

/** Complete rolling fiscal years only. Balance-sheet stocks must never be summed. */
export function statementRows(rows, line, ttm = false) {
  if (!ttm) return rows;
  const indexed = new Map(rows.map((r) => [Number(r.fiscal_year) * 4 + Number(r.fiscal_period?.slice(1)) - 1, r]));
  return rows.map((r) => {
    const end = Number(r.fiscal_year) * 4 + Number(r.fiscal_period?.slice(1)) - 1;
    const year = [3, 2, 1, 0].map((back) => indexed.get(end - back)?.lines?.[line]);
    return { ...r, period: `TTM · ${r.period}`, lines: { [line]: year.every(Number.isFinite) ? year.reduce((a, b) => a + b, 0) : null } };
  });
}

export function compatibleDataset(a, b) {
  if (!a || !b || a.timeframe !== b.timeframe) return false;
  // The backend refuses different providers for intraday session bars.
  const intraday = /^\d+(s|m|h)$/.test(a.timeframe);
  return !intraday || a.provider === b.provider;
}

export function minimumBars(variant, states = 2) {
  if (variant === 'volatility') return 60;
  // HMM needs fifty returns per state; N closes produce N-1 returns.
  if (variant === 'regime') return Math.max(200, states * 50 + 1);
  return variant === 'correlation' ? 3 : 2;
}

export function correlationData(basket) {
  return { labels: basket?.correlation?.labels ?? basket?.labels ?? [], matrix: basket?.correlation?.matrix ?? [] };
}

export function correlationPairs(labels, matrix) {
  return matrix.flatMap((row, i) => row.flatMap((value, j) => j > i && Number.isFinite(value)
    ? [{ label: `${labels[i]} ↔ ${labels[j]}`, value }] : []))
    .sort((a, b) => Math.abs(b.value) - Math.abs(a.value)).slice(0, 6);
}

export function seasonValue(value, metric, number, percent) {
  if (!Number.isFinite(value)) return '—';
  return metric === 'volume' ? number(value, 0) : percent(value * 100, 2);
}

export function sortedCompanies(rows, key, direction = 'desc') {
  return [...rows].sort((a, b) => {
    const av = key === 'ticker' ? a.ticker : a.metrics?.[key];
    const bv = key === 'ticker' ? b.ticker : b.metrics?.[key];
    if (av == null) return bv == null ? 0 : 1;
    if (bv == null) return -1;
    const order = typeof av === 'string' ? av.localeCompare(bv) : av - bv;
    return direction === 'asc' ? order : -order;
  });
}

/** Restrained surface tint preserves foreground contrast in light and dark themes. */
export function heatShade(value, max = 1, mode = 'return') {
  if (!Number.isFinite(value)) return 'var(--surface-2)';
  if (value === 0) return 'var(--surface)';
  const magnitude = Math.min(1, Math.abs(value) / (max || 1));
  const hue = mode === 'correlation' ? (value < 0 ? 'var(--diverge-neg)' : 'var(--diverge-pos)')
    : mode === 'return' ? (value < 0 ? 'var(--red)' : 'var(--green)') : 'var(--amber)';
  return `color-mix(in srgb, ${hue} ${8 + 22 * magnitude}%, var(--surface))`;
}
