/** Backtest API client.
 *
 * Runs are stateless: POST settings + dataset id → trades, equity, stats. Saved runs are a
 * history of {settings, stats} the user can rerun. Datasets come from the histdata catalog
 * (reuses the same data the visualization module charts).
 *
 * Settings model (v2): each side has an `entry` group and optional `exit` group — a list of
 * signal conditions combined with "all" (AND) or "any" (OR) — plus SL/TP and pyramiding at
 * the strategy level. v1 saved runs (single `signal` per side) are migrated on load. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

export const backtestApi = {
  /** Stored datasets (same catalog as Historical Data). */
  datasets: () => req('/histdata/datasets').then((r) => r.datasets),
  /** Bars for charting the result (parallel ts/o/h/l/c/v arrays). */
  bars: (id, limit = 50000) => req(`/histdata/datasets/${id}/bars?limit=${limit}`),

  /** Run a backtest. Returns { trades, equity, stats, ticker, timeframe, bars }. */
  run: (dataset_id, settings) =>
    req('/backtest/run', { method: 'POST', body: JSON.stringify({ dataset_id, settings }) }),

  /** Saved-run history (newest first). */
  runs: () => req('/backtest/runs').then((r) => r.runs),
  save: (name, dataset_id, settings, stats) =>
    req('/backtest/runs', {
      method: 'POST',
      body: JSON.stringify({ name, dataset_id, settings, stats })
    }),
  remove: (id) => req(`/backtest/runs/${id}`, { method: 'DELETE' })
};

/** Param descriptor: input key, short label, default value, input step. */
const P = (key, label, def, step = 1) => ({ key, label, def, step });

/** Indicator catalog — mirrors the Rust engine's `resolve`. Grouped for the dropdown. */
export const INDICATORS = [
  // Moving averages & trend
  { id: 'sma', label: 'SMA', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'ema', label: 'EMA', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'dema', label: 'DEMA', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'tema', label: 'TEMA', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'wma', label: 'WMA', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'hma', label: 'Hull MA', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'vwap', label: 'VWAP (rolling)', group: 'Moving averages', params: [P('period', 'Length', 20)] },
  { id: 'supertrend', label: 'SuperTrend', group: 'Moving averages', params: [P('period', 'Length', 10), P('mult', 'Mult', 3, 0.1)] },
  { id: 'psar', label: 'Parabolic SAR', group: 'Moving averages', params: [P('mult', 'Step', 0.02, 0.01)] },
  // Momentum
  { id: 'rsi', label: 'RSI', group: 'Momentum', params: [P('period', 'Length', 14)] },
  { id: 'stoch_k', label: 'Stochastic %K', group: 'Momentum', params: [P('period', 'Length', 14), P('signal_period', 'Smooth', 3)] },
  { id: 'stoch_d', label: 'Stochastic %D', group: 'Momentum', params: [P('period', 'Length', 14), P('signal_period', 'Smooth', 3)] },
  { id: 'cci', label: 'CCI', group: 'Momentum', params: [P('period', 'Length', 20)] },
  { id: 'willr', label: 'Williams %R', group: 'Momentum', params: [P('period', 'Length', 14)] },
  { id: 'roc', label: 'Rate of Change', group: 'Momentum', params: [P('period', 'Length', 12)] },
  { id: 'momentum', label: 'Momentum', group: 'Momentum', params: [P('period', 'Length', 10)] },
  { id: 'macd', label: 'MACD line', group: 'Momentum', params: [P('fast', 'Fast', 12), P('slow', 'Slow', 26)] },
  { id: 'macd_signal', label: 'MACD signal', group: 'Momentum', params: [P('fast', 'Fast', 12), P('slow', 'Slow', 26), P('signal_period', 'Signal', 9)] },
  { id: 'macd_hist', label: 'MACD histogram', group: 'Momentum', params: [P('fast', 'Fast', 12), P('slow', 'Slow', 26), P('signal_period', 'Signal', 9)] },
  { id: 'adx', label: 'ADX', group: 'Momentum', params: [P('period', 'Length', 14)] },
  // Volatility & bands
  { id: 'atr', label: 'ATR', group: 'Volatility & bands', params: [P('period', 'Length', 14)] },
  { id: 'stddev', label: 'Std deviation', group: 'Volatility & bands', params: [P('period', 'Length', 20)] },
  { id: 'bb_upper', label: 'Bollinger upper', group: 'Volatility & bands', params: [P('period', 'Length', 20), P('mult', 'Mult', 2, 0.1)] },
  { id: 'bb_mid', label: 'Bollinger mid', group: 'Volatility & bands', params: [P('period', 'Length', 20)] },
  { id: 'bb_lower', label: 'Bollinger lower', group: 'Volatility & bands', params: [P('period', 'Length', 20), P('mult', 'Mult', 2, 0.1)] },
  { id: 'keltner_upper', label: 'Keltner upper', group: 'Volatility & bands', params: [P('period', 'Length', 20), P('mult', 'Mult', 2, 0.1)] },
  { id: 'keltner_lower', label: 'Keltner lower', group: 'Volatility & bands', params: [P('period', 'Length', 20), P('mult', 'Mult', 2, 0.1)] },
  { id: 'donchian_upper', label: 'Donchian upper', group: 'Volatility & bands', params: [P('period', 'Length', 20)] },
  { id: 'donchian_mid', label: 'Donchian mid', group: 'Volatility & bands', params: [P('period', 'Length', 20)] },
  { id: 'donchian_lower', label: 'Donchian lower', group: 'Volatility & bands', params: [P('period', 'Length', 20)] },
  // Volume
  { id: 'mfi', label: 'Money Flow Index', group: 'Volume', params: [P('period', 'Length', 14)] },
  { id: 'obv', label: 'On-Balance Volume', group: 'Volume', params: [] }
];

/** Indicators grouped for `<optgroup>` rendering, preserving catalog order. */
export const INDICATOR_GROUPS = (() => {
  const map = new Map();
  for (const i of INDICATORS) {
    if (!map.has(i.group)) map.set(i.group, []);
    map.get(i.group).push(i);
  }
  return [...map.entries()].map(([label, items]) => ({ label, items }));
})();

export function indicatorById(id) {
  return INDICATORS.find((i) => i.id === id);
}

export const PRICE_FIELDS = ['close', 'open', 'high', 'low', 'volume'];

/** Signal comparison operators, grouped for the dropdown. */
export const OPS = [
  { id: 'crosses_above', label: 'crosses above', binary: true },
  { id: 'crosses_below', label: 'crosses below', binary: true },
  { id: 'cross', label: 'crosses (either)', binary: true },
  { id: 'above', label: 'is above', binary: true },
  { id: 'below', label: 'is below', binary: true },
  { id: 'rising', label: 'is rising', binary: false },
  { id: 'falling', label: 'is falling', binary: false },
  { id: 'closing_above', label: 'close above', binary: true },
  { id: 'closing_below', label: 'close below', binary: true },
  { id: 'opening_above', label: 'open above', binary: true },
  { id: 'opening_below', label: 'open below', binary: true }
];

export function opIsBinary(opId) {
  return OPS.find((o) => o.id === opId)?.binary ?? true;
}

/** Human-readable operand, e.g. "EMA(20)", "close", "70". */
export function operandText(o) {
  if (!o) return '';
  if (o.kind === 'price') return o.field;
  if (o.kind === 'const') return fmtNum(o.value);
  const def = indicatorById(o.indicator);
  const ps = (def?.params ?? []).map((p) => o[p.key]).filter((v) => v != null);
  return `${def?.label ?? o.indicator}${ps.length ? `(${ps.join(',')})` : ''}`;
}

/** Human-readable condition, e.g. "EMA(20) crosses above close". */
export function conditionText(c) {
  const op = OPS.find((o) => o.id === c.op)?.label ?? c.op;
  return opIsBinary(c.op) && c.right
    ? `${operandText(c.left)} ${op} ${operandText(c.right)}`
    : `${operandText(c.left)} ${op}`;
}

/** A fresh signal condition. */
export function defaultCondition(op = 'crosses_above') {
  return {
    left: { kind: 'indicator', indicator: 'ema', period: 20 },
    op,
    right: { kind: 'price', field: 'close' }
  };
}

/** A fresh side config: one entry condition, empty exit group, SL/TP, signal-exit. */
export function defaultSide(op = 'crosses_above') {
  return {
    entry: { logic: 'all', conditions: [defaultCondition(op)] },
    exit: { logic: 'all', conditions: [] },
    stop_loss_pct: 0.02,
    take_profit_pct: 0.04,
    exit_on_reverse: true
  };
}

/** A fresh default settings object for a new strategy (long-only to start). */
export function defaultSettings() {
  return {
    mode: 'long',
    long: defaultSide('crosses_above'),
    short: defaultSide('crosses_below'),
    reverse_side: false, // UI-only: derive short from long's inverse
    stop_and_reverse: false,
    pyramiding: 1,
    sizing: { mode: 'percent_equity', percent: 100 },
    starting_capital: 10000,
    leverage: 1,
    spread_pct: 0,
    fees: { amount_kind: 'pct', per: 'trade', amount: 0.1 }
  };
}

/** Deep clone plain settings data. `structuredClone` rejects Svelte 5 reactive proxies, so
 * round-trip through JSON — settings are pure data (no functions/dates/undefined-as-value). */
const clone = (v) => JSON.parse(JSON.stringify(v));

/** Fold a v1 side (single `signal`) into the v2 entry/exit-group shape, in place. */
function migrateSide(side, fallbackOp) {
  if (!side) return defaultSide(fallbackOp);
  if (!side.entry?.conditions?.length) {
    side.entry = {
      logic: 'all',
      conditions: side.signal ? [side.signal] : [defaultCondition(fallbackOp)]
    };
  }
  if (!side.exit) side.exit = { logic: 'all', conditions: [] };
  delete side.signal;
  return side;
}

/** Upgrade loaded settings (from a saved run) to the current shape. */
export function migrateSettings(s) {
  const out = clone(s);
  out.long = migrateSide(out.long, 'crosses_above');
  out.short = migrateSide(out.short, 'crosses_below');
  out.reverse_side ??= false;
  out.stop_and_reverse ??= false;
  out.pyramiding ??= 1;
  return out;
}

/** Inverse of a comparison op (for the "reverse side" convenience). */
const OP_INVERSE = {
  crosses_above: 'crosses_below',
  crosses_below: 'crosses_above',
  cross: 'cross',
  above: 'below',
  below: 'above',
  rising: 'falling',
  falling: 'rising',
  closing_above: 'closing_below',
  closing_below: 'closing_above',
  opening_above: 'opening_below',
  opening_below: 'opening_above'
};

function inverseCondition(c) {
  return {
    left: clone(c.left),
    op: OP_INVERSE[c.op] ?? c.op,
    right: c.right ? clone(c.right) : undefined
  };
}

function inverseGroup(g) {
  return { logic: g?.logic ?? 'all', conditions: (g?.conditions ?? []).map(inverseCondition) };
}

/** Build a short side as the mirror image of a long side (inverse ops + same SL/TP/exit). */
export function inverseSide(longSide) {
  return {
    entry: inverseGroup(longSide.entry),
    exit: inverseGroup(longSide.exit),
    stop_loss_pct: longSide.stop_loss_pct,
    take_profit_pct: longSide.take_profit_pct,
    exit_on_reverse: longSide.exit_on_reverse
  };
}

/** Strip UI-only fields and apply reverse-side derivation before posting/saving. */
export function normalizeSettings(s) {
  const out = clone(s);
  delete out.reverse_side;
  if (s.reverse_side && s.long) out.short = inverseSide(s.long);
  // Drop the side(s) the mode doesn't use so the engine ignores them cleanly.
  if (out.mode === 'long') out.short = null;
  if (out.mode === 'short') out.long = null;
  out.pyramiding = Math.max(1, Math.min(20, Math.round(out.pyramiding || 1)));
  return out;
}

export const fmtNum = (n, d = 2) =>
  n == null || Number.isNaN(n)
    ? '–'
    : Number(n).toLocaleString(undefined, { maximumFractionDigits: d });
