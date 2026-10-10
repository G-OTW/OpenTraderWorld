/** Shared ECharts styling and number formatting for the quant panels. */

/** A value axis in the house style. */
export const valueAxis = (p, extra = {}) => ({
  type: 'value',
  axisLine: { lineStyle: { color: p.border } },
  axisTick: { show: false },
  axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
  splitLine: { lineStyle: { color: p.gridLine, width: 0.5 } },
  ...extra
});

/** A category axis in the house style. */
export const categoryAxis = (p, data, extra = {}) => ({
  type: 'category',
  data,
  axisLine: { lineStyle: { color: p.border } },
  axisTick: { show: false },
  axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
  ...extra
});

/** A time axis in the house style. */
export const timeAxis = (p, extra = {}) => ({
  type: 'time',
  axisLine: { lineStyle: { color: p.border } },
  axisTick: { show: false },
  axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
  splitLine: { show: false },
  ...extra
});

export const tooltip = (p, extra = {}) => ({
  trigger: 'axis',
  backgroundColor: p.surface,
  borderColor: p.border,
  textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 },
  ...extra
});

export const legend = (p, extra = {}) => ({
  top: 0,
  right: 0,
  textStyle: { color: p.muted, fontSize: 11 },
  itemWidth: 14,
  itemHeight: 8,
  ...extra
});

export const title = (p, text, extra = {}) => ({
  text,
  left: 0,
  top: 0,
  textStyle: { fontSize: 11, fontWeight: 500, color: p.muted },
  ...extra
});

/** A p-value, with the floor spelled out rather than rounded to zero. */
export function fmtP(v) {
  if (v == null || !Number.isFinite(v)) return '−';
  if (v < 0.0001) return '< 0.0001';
  return v < 0.01 ? v.toFixed(4) : v.toFixed(3);
}

/** A plain number with a fixed number of decimals, or a dash. */
export function fmtX(v, d = 2) {
  if (v == null || !Number.isFinite(v)) return '−';
  return v.toFixed(d);
}

/** A fraction as a percent, or a dash. */
export function fmtPc(v, d = 2) {
  if (v == null || !Number.isFinite(v)) return '−';
  return `${(v * 100).toFixed(d)}%`;
}

/** Significance stars at the usual 5% / 1% / 0.1% cut-offs. */
export function stars(p) {
  if (p == null || !Number.isFinite(p)) return '';
  return p < 0.001 ? '***' : p < 0.01 ? '**' : p < 0.05 ? '*' : '';
}
