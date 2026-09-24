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
// The custom-indicator library (DAG catalog, builder model, CRUD) is shared with the chart
// module — it lives in $lib/indicators, not here.
import { indicatorById } from '$lib/indicators/dag.js';
import { stats as tradeStats, returnBuckets } from '$lib/analysis/trades.js';

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

  /** Run a backtest over one or more datasets. `datasetIds` is an array (single-asset = one
   *  element). Returns { trades, equity, stats, per_asset, warmup_bars, alignment, … }. */
  run: (datasetIds, settings, { record = true, from = null, to = null } = {}) =>
    req('/backtest/run', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids: datasetIds, settings, record, from, to })
    }),

  /** Alignment preview (no simulation): overlap window, warm-up, per-asset inactive bars. */
  align: (datasetIds, settings) =>
    req('/backtest/align', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids: datasetIds, settings })
    }).then((r) => r.alignment),

  /** Run history, newest first. Every run is recorded automatically; `filter: 'saved'`
   *  narrows it to the runs the user named. */
  runs: (filter) =>
    req(`/backtest/runs${filter ? `?filter=${filter}` : ''}`).then((r) => r.runs),
  /** Name a run so it survives the history cap. `runId` comes from the run response. */
  save: (name, runId, { datasetIds, settings, stats, strategyId = null } = {}) =>
    req('/backtest/runs', {
      method: 'POST',
      body: JSON.stringify({
        name,
        run_id: runId,
        // Only used when there is no run_id (nothing to pin) — keeps older flows working.
        dataset_id: datasetIds?.[0],
        dataset_ids: datasetIds,
        strategy_id: strategyId,
        settings,
        stats
      })
    }),
  remove: (id) => req(`/backtest/runs/${id}`, { method: 'DELETE' }),
  /** Clear the auto history; named runs are kept. */
  clearHistory: () => req('/backtest/runs', { method: 'DELETE' }),

  /** URL of a saved run's Markdown report (for download links). */
  reportUrl: (id) => `/api/backtest/runs/${id}/report.md`,
  /** Fetch a saved run's Markdown report text. */
  reportMd: (id) =>
    fetch(`/api/backtest/runs/${id}/report.md`).then((r) => {
      if (!r.ok) throw new Error(`report failed (${r.status})`);
      return r.text();
    }),

  // ── Optimizer (parameter grid as a job) ──
  // The estimate measures one variant on this machine, so the count the user confirms comes with
  // a duration that means something. Starting returns a job id; everything else is polling.
  optimizeEstimate: (body) =>
    req('/backtest/optimize/estimate', { method: 'POST', body: JSON.stringify(body) }),
  optimizeStart: (body) => req('/backtest/optimize', { method: 'POST', body: JSON.stringify(body) }),
  optimizeJobs: () => req('/backtest/optimize').then((r) => r.jobs),
  /** Progress + one ranking page. `analysis` also brings the per-parameter effect table. */
  optimizeStatus: (id, { sort, dir, offset = 0, limit = 50, analysis = false } = {}) => {
    const q = new URLSearchParams({ offset, limit });
    if (sort) q.set('sort', sort);
    if (dir) q.set('dir', dir);
    if (analysis) q.set('analysis', 'true');
    return req(`/backtest/optimize/${id}?${q}`);
  },
  optimizeCancel: (id) => req(`/backtest/optimize/${id}/cancel`, { method: 'POST' }),
  optimizeForget: (id) => req(`/backtest/optimize/${id}`, { method: 'DELETE' }),

  // ── Strategy library (named Settings + tags the library search matches) ──
  strategies: () => req('/backtest/strategies').then((r) => r.strategies),
  getStrategy: (id) => req(`/backtest/strategies/${id}`).then((r) => r.strategy),
  createStrategy: (name, description, tags, settings) =>
    req('/backtest/strategies', { method: 'POST', body: JSON.stringify({ name, description, tags, settings }) }),
  updateStrategy: (id, name, description, tags, settings) =>
    req(`/backtest/strategies/${id}`, { method: 'PUT', body: JSON.stringify({ name, description, tags, settings }) }),
  deleteStrategy: (id) => req(`/backtest/strategies/${id}`, { method: 'DELETE' })
};

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
  if (o.kind === 'metric') return o.metric === 'change_pct' ? `change_pct(${o.period ?? 0})` : o.metric;
  if (o.kind === 'position') return o.field;
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

/** Sizing modes, for the dropdown. `needsStop` gates a validation hint in the UI. */
export const SIZING_MODES = [
  { id: 'percent_equity', label: 'Percent of equity' },
  { id: 'fixed_qty', label: 'Fixed quantity' },
  { id: 'risk', label: 'Risk per trade', needsStop: true },
  { id: 'equity_tiers', label: 'Equity tiers (step table)' },
  { id: 'kelly', label: 'Kelly (fractional)' }
];

/** A fresh sizing object for a given mode, with sensible defaults. */
export function defaultSizing(mode) {
  switch (mode) {
    case 'fixed_qty':
      return { mode: 'fixed_qty', qty: 1 };
    case 'risk':
      return { mode: 'risk', risk_pct: 1 };
    case 'equity_tiers':
      return {
        mode: 'equity_tiers',
        metric: 'qty',
        tiers: [
          { above: 0, value: 0.1 },
          { above: 1000, value: 0.5 },
          { above: 10000, value: 2 }
        ]
      };
    case 'kelly':
      return {
        mode: 'kelly',
        fraction: 0.5,
        window: 30,
        cap_pct: 20,
        warmup: { mode: 'percent_equity', percent: 2 }
      };
    default:
      return { mode: 'percent_equity', percent: 100 };
  }
}

/** True when the sizing mode can only size with an active stop-loss (risk-based). */
export function sizingNeedsStop(sizing) {
  return sizing?.mode === 'risk' || (sizing?.mode === 'equity_tiers' && sizing?.metric === 'risk_pct');
}

/** Does this side give risk sizing a stop *price at entry*? Mirrors the engine's `has_stop`:
    the v2 rule object, the legacy percent field, or a trailing stop that is live from the entry
    bar. A trail waiting on an activation threshold, or a bare breakeven step, is not one: there
    is no level at entry to size a risk against. */
export function sideHasStop(side) {
  if (!side) return false;
  if ((side.stop_loss?.value ?? 0) > 0 || (side.stop_loss_pct ?? 0) > 0) return true;
  const tr = side.trailing_stop;
  return (tr?.value ?? 0) > 0 && (tr?.activate_pct ?? 0) <= 0;
}

/** The three engine kinds and the i18n key that names each: history rows, the save modal and
    the strategy library all label a run the same way the Strategy step does. */
export const KIND_LABEL_KEYS = {
  signals: 'backtest.grid.signals',
  grid: 'backtest.grid.grid',
  dca: 'backtest.dca.mode'
};

/** Kind of a settings object, tolerant of legacy rows that predate the field. */
export function settingsKind(s) {
  const k = s?.kind;
  return k === 'grid' || k === 'dca' ? k : 'signals';
}

/** A fresh default settings object for a new strategy (long-only to start). */
/** A fresh grid config (long ladder). */
export function defaultGrid() {
  return {
    lower: 0,
    upper: 0,
    levels: 10,
    qty_per_level: 0,
    total_budget: 0,
    direction: 'long',
    stop_below: 0,
    stop_above: 0,
    // Anchored ladder: 'none' keeps the fixed [lower, upper] band, anything else centres the
    // grid on that line and sizes the band from width_kind/width_value.
    anchor: 'none',
    anchor_period: 20,
    width_kind: 'pct',
    width_value: 2,
    width_period: 14,
    reset_on_close: false
  };
}

/** Moving references a grid can be anchored to (engine: GridConfig.anchor). */
export const GRID_ANCHORS = ['none', 'sma', 'ema', 'dema', 'tema', 'wma', 'hma', 'vwap'];

// ── DCA plan ──
// A savings plan, not a strategy: a basket with fixed weights, an optional recurring
// contribution, then conditional buy and sell tranches on top. `weights` is filled from the
// selected datasets by the editor, so a plan always names the assets it is about. The starting
// capital is deployed in one shot at the first bar; contributions and conditional tranches are
// new money paid in on top.

/** A fresh DCA plan: 100 a month, nothing conditional yet. */
export function defaultDca() {
  return {
    weights: [],
    contribution: defaultContribution(),
    buys: [],
    sells: []
  };
}

export function defaultContribution() {
  return { amount: 100, period: 'month', every: 1, invest: true };
}

/** Contribution periods, in the order the dropdown lists them. */
export const DCA_PERIODS = ['bar', 'day', 'week', 'month', 'quarter', 'year'];

/** A fresh conditional buy tranche: 500 whenever the basket is 10% off its high. */
export function defaultDcaBuy() {
  return {
    name: '',
    amount_kind: 'fixed',
    amount: 500,
    per_asset: false,
    condition: {
      logic: 'all',
      conditions: [
        {
          left: { kind: 'metric', metric: 'dd_from_high', period: 0 },
          op: 'above',
          right: { kind: 'const', value: 10 }
        }
      ]
    },
    max_fires: 0,
    cooldown_bars: 0
  };
}

/** A fresh sell rule: take a quarter off the table once the position is up 50%. */
export function defaultDcaSell() {
  return {
    name: '',
    amount_kind: 'pct_position',
    amount: 25,
    per_asset: true,
    target_gain_pct: 50,
    condition: { logic: 'all', conditions: [] },
    max_fires: 0,
    cooldown_bars: 0,
    withdraw: false
  };
}

/** How much a tranche deploys (engine: DcaBuy.amount_kind). */
export const DCA_BUY_KINDS = ['fixed', 'pct_cash', 'pct_equity', 'pct_invested'];
/** How much a sell rule takes back (engine: DcaSell.amount_kind). */
export const DCA_SELL_KINDS = ['pct_position', 'all', 'units', 'amount'];

/** Derived price statistics usable as an operand anywhere (engine: `Operand::Metric`). */
export const METRICS = [
  { id: 'dd_from_high', param: false },
  { id: 'up_from_low', param: false },
  { id: 'change_pct', param: true },
  { id: 'change_from_start', param: false }
];

/** Live portfolio fields, readable only inside a DCA run (engine: `Operand::Position`). */
export const POSITION_FIELDS = [
  'pnl_pct',
  'since_last_buy_pct',
  'avg_cost',
  'units',
  'value',
  'weight_pct',
  'cash_pct',
  'drawdown_pct'
];

/** Bars in one calendar span at a given timeframe: what turns "1 year" into a `change_pct`
 *  period. Returns 0 when the timeframe is not one the catalog knows. */
export function barsPerSpan(timeframe, span) {
  const m = /^(\d+)\s*([mhdwM])$/.exec(String(timeframe ?? '').trim());
  if (!m) return 0;
  const per = { m: 1, h: 60, d: 60 * 24, w: 60 * 24 * 7, M: 60 * 24 * 30 };
  const mins = Number(m[1]) * (per[m[2]] ?? 0);
  if (!mins) return 0;
  const spans = { '1W': 60 * 24 * 7, '1M': 60 * 24 * 30, '3M': 60 * 24 * 91, '1Y': 60 * 24 * 365 };
  const want = spans[span];
  return want ? Math.max(1, Math.round(want / mins)) : 0;
}

export function defaultSettings() {
  return {
    kind: 'signals', // 'signals' | 'grid' | 'dca'
    grid: null,
    dca: null,
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
    fees: { amount_kind: 'pct', per: 'trade', amount: 0.1 },
    risk: {}, // portfolio limits + circuit breakers
    pyramid_steps: { scale: [], min_distance_pct: 0, after_add_sl: 'none' },
    instrument: { multiplier: 1, lot_step: 0, min_qty: 0 },
    slippage: { kind: 'pct', value: 0, tick_size: 0 },
    oos_split_pct: 0,
    funding: { annual_rate_pct: 0, interval_hours: 8 },
    filters: defaultFilters()
  };
}

/** Trading-window filters, all inert: no clock rule, no calendar rule, nothing gated. */
export function defaultFilters() {
  return {
    tz_offset_min: 0,
    weekdays: [],
    sessions: [],
    include_dates: [],
    exclude_dates: [],
    on_window_end: 'hold',
    block_adds: true
  };
}

/** Whether any filter rule is set (the engine's `is_active`, mirrored for the UI). */
export function filtersActive(f) {
  return !!(f?.weekdays?.length || f?.sessions?.length || f?.include_dates?.length || f?.exclude_dates?.length);
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
  out.risk ??= {};
  out.sizing ??= { mode: 'percent_equity', percent: 100 };
  out.pyramid_steps ??= { scale: [], min_distance_pct: 0, after_add_sl: 'none' };
  out.instrument ??= { multiplier: 1, lot_step: 0, min_qty: 0 };
  out.slippage ??= { kind: 'pct', value: 0, tick_size: 0 };
  out.oos_split_pct ??= 0;
  out.kind ??= 'signals';
  out.grid ??= null;
  out.dca ??= null;
  // A plan saved before a field existed has none of its keys; fill them so the form binds. A
  // dca-kind strategy always carries one, or the editor would have nothing to write into.
  if (out.dca || out.kind === 'dca') out.dca = { ...defaultDca(), ...(out.dca ?? {}) };
  // A grid saved before anchoring existed has none of those keys; fill them so the form binds.
  if (out.grid) out.grid = { ...defaultGrid(), ...out.grid };
  out.funding ??= { annual_rate_pct: 0, interval_hours: 8 };
  // A strategy saved before filters existed has no block at all; fill every key so the form binds.
  out.filters = { ...defaultFilters(), ...(out.filters ?? {}) };
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
    stop_loss: longSide.stop_loss ? clone(longSide.stop_loss) : null,
    take_profit: longSide.take_profit ? clone(longSide.take_profit) : null,
    trailing_stop: longSide.trailing_stop ? clone(longSide.trailing_stop) : null,
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
  // The DCA plan only means anything in DCA mode; sending it otherwise saves dead config.
  if (out.kind !== 'dca') out.dca = null;
  if (out.kind === 'dca' && out.dca) {
    // A weight row for an asset that is no longer selected would silently take a share of
    // every tranche, so the plan only ever carries the basket it is about. A zero weight is
    // kept: it is how the user says "hold none of this one", and dropping it made an
    // all-zero table read as equal weights.
    out.dca.weights = (out.dca.weights ?? []).filter((w) => w.ticker && Number(w.weight) >= 0);
    if (out.dca.contribution && !(Number(out.dca.contribution.amount) > 0)) out.dca.contribution = null;
    // A savings plan produces no out-of-sample block, and the engine refuses a split rather
    // than ignoring one.
    out.oos_split_pct = 0;
  }
  // A half-typed filter row (no time, no start date) is an editing state, not a rule: the
  // engine refuses malformed ones, so drop them here instead of failing the run.
  if (out.filters) {
    const f = out.filters;
    f.sessions = (f.sessions ?? []).filter((s) => s.from && s.to && s.from !== s.to);
    for (const key of ['include_dates', 'exclude_dates']) {
      f[key] = (f[key] ?? [])
        .filter((d) => d.from)
        .map((d) => ({ from: d.from, to: d.to || null }));
    }
  }
  return out;
}

/** Collect the ids of every custom-indicator operand referenced anywhere in a settings object. */
export function usedCustomIndicatorIds(settings) {
  const ids = new Set();
  const scanOperand = (o) => o?.kind === 'custom_indicator' && o.id && ids.add(o.id);
  const scanGroup = (g) =>
    (g?.conditions ?? []).forEach((c) => {
      scanOperand(c.left);
      scanOperand(c.right);
    });
  for (const side of [settings.long, settings.short]) {
    if (!side) continue;
    scanGroup(side.entry);
    scanGroup(side.exit);
  }
  for (const rule of [...(settings.dca?.buys ?? []), ...(settings.dca?.sells ?? [])]) {
    scanGroup(rule.condition);
  }
  return [...ids];
}

/** Embed the custom-indicator definitions a strategy uses into `settings.indicators`, keyed by
 *  id, so a run/save is self-contained and reproducible. `library` = [{ id, definition }]. */
export function embedIndicators(settings, library) {
  const out = clone(settings);
  const byId = new Map(library.map((i) => [i.id, i.definition]));
  const defs = {};
  for (const id of usedCustomIndicatorIds(out)) {
    if (byId.has(id)) defs[id] = byId.get(id);
  }
  out.indicators = defs;
  return out;
}

/** Engine trades → the normalised shape `$lib/analysis` reads, so a simulated run gets the
 *  same statistics and performance views as a quick backtest drawn on the chart. The engine
 *  reports MAE/MFE as positive amounts in account currency; the analysis shape wants the
 *  excursion pair signed (run-up above the line, heat below), and R over the heat taken. */
export function toAnalysisTrades(trades = []) {
  return trades.map((t) => {
    const heat = t.mae ?? 0;
    return {
      dir: t.direction === 'short' ? -1 : 1,
      qty: t.qty,
      entryAvg: t.entry_price,
      exitAvg: t.exit_price,
      openTs: Date.parse(t.entry_ts),
      closeTs: Date.parse(t.exit_ts),
      pnl: t.pnl,
      pnlPct: t.return_pct,
      runup: t.mfe ?? 0,
      mae: -heat,
      r: heat > 0 ? t.pnl / heat : null
    };
  });
}

/** How a trade ended → its i18n key. Shared by the trade list and the report. */
export const EXIT_REASON_KEYS = {
  signal: 'backtest.trades.reason.signal',
  exit_signal: 'backtest.trades.reason.exitSignal',
  stop_loss: 'backtest.trades.reason.stopLoss',
  trailing_stop: 'backtest.trades.reason.trailingStop',
  take_profit: 'backtest.trades.reason.takeProfit',
  reverse: 'backtest.trades.reason.reverse',
  grid_reset: 'backtest.trades.reason.gridReset',
  session_end: 'backtest.trades.reason.sessionEnd',
  end: 'backtest.trades.reason.end'
};

export const fmtNum = (n, d = 2) =>
  n == null || Number.isNaN(n)
    ? '–'
    : Number(n).toLocaleString(undefined, { maximumFractionDigits: d });

/** Build a Markdown report from an in-memory run result (the results screen as a document).
 *  Mirrors the server document (`/api/backtest/runs/{id}/report.md`) section for section, so a
 *  saved run and an unsaved one read the same. Deliberately no trade list: a run has thousands
 *  of them and the report is about the aggregate. */
export function buildReportMd({ name, settings, result }) {
  const r = result ?? {};
  const stats = r.stats ?? {};
  const L = [];
  const title = name || 'Backtest';
  const period = periodOf(r);

  L.push('---');
  L.push(`title: "${title.replace(/"/g, "'")}"`);
  L.push(`ticker: ${r.ticker ?? ''}`);
  L.push(`timeframe: ${r.timeframe ?? ''}`);
  if (stats.engine_version != null) L.push(`engine_version: ${stats.engine_version}`);
  if (r.bars != null) L.push(`bars: ${r.bars}`);
  if (period) L.push(`period_from: ${period[0]}`, `period_to: ${period[1]}`);
  L.push('---', '');
  L.push(`# Backtest report — ${title}`, '');
  L.push(`**${r.ticker ?? ''}** · \`${r.timeframe ?? ''}\``, '');

  // ── Summary ──
  L.push('## Summary', '');
  const ctx = contextLine(r, period);
  if (ctx) L.push(ctx, '');
  L.push('| Metric | Value |', '|---|---|');
  const stat = (label, value, hint) => L.push(`| ${label} | ${value}${hint ? ` _(${hint})_` : ''} |`);
  if (stats.return_pct != null) {
    const bh = stats.buy_hold_return_pct;
    const edge = bh != null ? (stats.return_pct - bh) : null;
    stat('Return', `${fmtNum(stats.return_pct)}%`,
      bh != null ? `buy & hold ${fmtNum(bh)}%, edge ${edge >= 0 ? '+' : ''}${fmtNum(edge)}%` : '');
  }
  if (stats.net_pnl != null)
    stat('Net PnL', fmtNum(stats.net_pnl),
      stats.final_equity != null ? `${fmtNum(stats.total_fees)} fees · equity ${fmtNum(stats.final_equity)}` : '');
  if (stats.win_rate != null)
    stat('Win rate', `${fmtNum(stats.win_rate, 1)}%`,
      stats.trades != null ? `${stats.wins ?? 0}W / ${stats.losses ?? 0}L of ${stats.trades} trades` : '');
  if (stats.profit_factor != null)
    stat('Profit factor', fmtNum(stats.profit_factor),
      stats.expectancy_pct != null ? `expectancy ${fmtNum(stats.expectancy_pct, 3)}% / trade` : '');
  if (stats.max_drawdown_pct != null)
    stat('Max drawdown', `−${fmtNum(stats.max_drawdown_pct)}%`,
      stats.max_drawdown != null ? `−${fmtNum(stats.max_drawdown)}` : '');
  if (stats.sharpe != null)
    stat('Sharpe', fmtNum(stats.sharpe), stats.sortino != null ? `Sortino ${fmtNum(stats.sortino)}` : '');
  if (stats.avg_trade != null) stat('Avg trade', fmtNum(stats.avg_trade));
  if (stats.final_equity != null) stat('Final equity', fmtNum(stats.final_equity));
  if (r.total_funding) stat('Funding', fmtNum(r.total_funding), 'estimate');
  L.push('');

  // ── Equity curve (sparkline, same shape the server renderer emits) ──
  const eq = (r.equity ?? []).map((p) => p.equity).filter((v) => v != null);
  if (eq.length >= 2) {
    L.push('## Equity curve', '', `\`${sparkline(eq)}\``, '');
    const min = Math.min(...eq);
    const max = Math.max(...eq);
    L.push(
      `Account equity: start ${fmtNum(eq[0])} → end ${fmtNum(eq[eq.length - 1])} · min ${fmtNum(min)} · max ${fmtNum(max)} (${eq.length} points)`,
      ''
    );
  }

  // ── Strategy definition (parameters, indicators, entry/exit signals) ──
  if (settings) L.push(...strategyReportSection(settings));

  // ── Performance, All / Long / Short ──
  // The excursion rows live on the trades, not on the engine's SideStats, so they are derived
  // here from the same analytics the statistics tab reads.
  const analysisTrades = toAnalysisTrades(r.trades ?? []);
  const byScope = analysisTrades.length
    ? [
        tradeStats(analysisTrades),
        tradeStats(analysisTrades.filter((t) => t.dir > 0)),
        tradeStats(analysisTrades.filter((t) => t.dir < 0))
      ]
    : null;
  if (stats.all && stats.long && stats.short) {
    L.push('## Performance', '', '| Metric | All | Long | Short |', '|---|---:|---:|---:|');
    for (const row of PERF_ROWS)
      L.push(`| ${row.label} | ${[stats.all, stats.long, stats.short].map((sc) => perfCell(sc, row)).join(' | ')} |`);
    if (byScope)
      for (const row of EXCURSION_ROWS)
        L.push(`| ${row.label} | ${byScope.map((sc) => perfCell(sc, row)).join(' | ')} |`);
    L.push('');
  }

  // ── Where the per-trade results fall ──
  if (analysisTrades.length) {
    const { buckets } = returnBuckets(analysisTrades);
    const filled = buckets.filter((b) => b.winners + b.losers > 0);
    if (filled.length > 1) {
      L.push('## Return distribution', '', '| Return | Winners | Losers | Trades | Share |', '|---|---:|---:|---:|---:|');
      for (const b of filled) {
        const n = b.winners + b.losers;
        L.push(`| ${fmtNum(b.from)}% to ${fmtNum(b.to)}% | ${b.winners} | ${b.losers} | ${n} | ${fmtNum((n * 100) / analysisTrades.length, 1)}% |`);
      }
      L.push('');
    }
  }

  // ── In-sample vs out-of-sample ──
  if (r.oos?.in_sample && r.oos?.out_sample) {
    const head = `In-sample (${Math.round((r.oos.split_pct ?? 0) * 100)}%)`;
    const out = r.oos.split_ts ? `Out-of-sample (from ${dayOf(r.oos.split_ts)})` : 'Out-of-sample';
    L.push('## Out-of-sample', '', `| Metric | ${head} | ${out} |`, '|---|---:|---:|');
    for (const row of OOS_ROWS)
      L.push(`| ${row.label} | ${perfCell(r.oos.in_sample, row)} | ${perfCell(r.oos.out_sample, row)} |`);
    L.push('', 'A wide gap between the two columns is the overfitting warning sign.', '');
  }

  // ── Per-asset breakdown (portfolio runs) ──
  if (r.per_asset?.length > 1) {
    L.push('## Per-asset breakdown', '', '| Asset | Trades | Win rate | Net PnL | Fees | Exposure |', '|---|---:|---:|---:|---:|---:|');
    for (const a of r.per_asset)
      L.push(
        `| ${a.ticker} | ${fmtNum(a.trades, 0)} | ${fmtNum(a.win_rate, 1)}% | ${fmtNum(a.net_pnl)} | ${fmtNum(a.total_fees)} | ${fmtNum(a.exposure_pct, 1)}% |`
      );
    L.push('');
    // The statistics screen, one column per asset: the detail the comparison table leaves out.
    const tickers = r.per_asset.map((a) => a.ticker);
    const perAsset = tickers.map((tk) => tradeStats(toAnalysisTrades((r.trades ?? []).filter((t) => t.ticker === tk))));
    if (perAsset.some((s) => s.trades > 0)) {
      L.push('### Per-asset statistics', '', `| Metric | ${tickers.join(' | ')} |`, `|---|${tickers.map(() => '---:').join('|')}|`);
      for (const row of ASSET_ROWS)
        L.push(`| ${row.label} | ${perAsset.map((sc) => perfCell(sc, row)).join(' | ')} |`);
      L.push('');
    }
  }

  // ── Grid-mode inventory ──
  if (r.grid) {
    L.push('## Grid', '', '| Metric | Value |', '|---|---|');
    L.push(`| Levels | ${fmtNum(r.grid.levels, 0)} |`);
    L.push(`| Fills | ${fmtNum(r.grid.fills, 0)} |`);
    L.push(`| Round trips | ${fmtNum(r.grid.round_trips, 0)} |`);
    L.push(`| End inventory | ${fmtNum(r.grid.end_inventory, 4)} _(worth ${fmtNum(r.grid.end_inventory_value)})_ |`);
    L.push('');
  }

  // ── Exit reasons ──
  const reasons = Object.entries(stats.exit_reasons ?? {});
  if (reasons.length) {
    const total = reasons.reduce((n, [, v]) => n + v, 0);
    L.push('## Exit reasons', '', '| Reason | Count | Share |', '|---|---:|---:|');
    for (const [k, v] of reasons.sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])))
      L.push(`| ${prettyKey(k)} | ${v} | ${fmtNum(total ? (v * 100) / total : 0, 1)}% |`);
    L.push('');
  }

  // ── What the engine refused / sat out ──
  const notes = executionNotes(r);
  if (notes.length) L.push('## Execution notes', '', ...notes.map((n) => `- ${n}`), '');

  return L.join('\n');
}

/** Rows of the All/Long/Short performance table: the PerfTable component, as Markdown. */
const PERF_ROWS = [
  { label: 'Net profit', key: 'net_pnl', fmt: 'num' },
  { label: 'Gross profit', key: 'gross_profit', fmt: 'num' },
  { label: 'Gross loss', key: 'gross_loss', fmt: 'neg' },
  { label: 'Profit factor', key: 'profit_factor', fmt: 'num' },
  { label: 'Total fees', key: 'total_fees', fmt: 'num' },
  { label: 'Total trades', key: 'trades', fmt: 'int' },
  { label: 'Winning trades', key: 'wins', fmt: 'int' },
  { label: 'Losing trades', key: 'losses', fmt: 'int' },
  { label: 'Win rate', key: 'win_rate', fmt: 'pct' },
  { label: 'Avg trade', key: 'avg_trade', fmt: 'num' },
  { label: 'Avg winning trade', key: 'avg_win', fmt: 'num' },
  { label: 'Avg losing trade', key: 'avg_loss', fmt: 'neg' },
  { label: 'Payoff ratio', key: 'payoff_ratio', fmt: 'num' },
  { label: 'Expectancy / trade', key: 'expectancy_pct', fmt: 'pct3' },
  { label: 'Largest winning trade', key: 'largest_win', fmt: 'num' },
  { label: 'Largest losing trade', key: 'largest_loss', fmt: 'neg' },
  { label: 'Max consecutive wins', key: 'max_consec_wins', fmt: 'int' },
  { label: 'Max consecutive losses', key: 'max_consec_losses', fmt: 'int' },
  { label: 'Avg bars in trade', key: 'avg_bars_held', fmt: 'bars' }
];

/** Trade-derived rows the engine's SideStats does not carry (excursions, R, closed drawdown). */
const EXCURSION_ROWS = [
  { label: 'Avg run-up', key: 'avgRunup', fmt: 'num' },
  { label: 'Avg heat (MAE)', key: 'avgMae', fmt: 'num' },
  { label: 'Avg R (result / heat)', key: 'avgR', fmt: 'num' },
  { label: 'Max drawdown, closed trades', key: 'maxDrawdown', fmt: 'neg' }
];

/** One column per asset: the statistics tab, per ticker. */
const ASSET_ROWS = [
  { label: 'Trades', key: 'trades', fmt: 'int' },
  { label: 'Win rate', key: 'winRate', fmt: 'pct' },
  { label: 'Net PnL', key: 'pnl', fmt: 'num' },
  { label: 'Gross profit', key: 'grossWin', fmt: 'num' },
  { label: 'Gross loss', key: 'grossLoss', fmt: 'neg' },
  { label: 'Profit factor', key: 'profitFactor', fmt: 'num' },
  { label: 'Expectancy', key: 'expectancy', fmt: 'num' },
  { label: 'Avg trade return', key: 'avgPnlPct', fmt: 'pct3' },
  { label: 'Avg win', key: 'avgWin', fmt: 'num' },
  { label: 'Avg loss', key: 'avgLoss', fmt: 'num' },
  { label: 'Best trade', key: 'best', fmt: 'num' },
  { label: 'Worst trade', key: 'worst', fmt: 'num' },
  { label: 'Max drawdown, closed trades', key: 'maxDrawdown', fmt: 'neg' },
  { label: 'Avg R (result / heat)', key: 'avgR', fmt: 'num' },
  { label: 'Avg run-up', key: 'avgRunup', fmt: 'num' },
  { label: 'Avg heat (MAE)', key: 'avgMae', fmt: 'num' }
];

const OOS_ROWS = [
  { label: 'Return', key: 'return_pct', fmt: 'pct3' },
  { label: 'Net PnL', key: 'net_pnl', fmt: 'num' },
  { label: 'Win rate', key: 'win_rate', fmt: 'pct' },
  { label: 'Profit factor', key: 'profit_factor', fmt: 'num' },
  { label: 'Trades', key: 'trades', fmt: 'int' },
  { label: 'Max drawdown', key: 'max_drawdown_pct', fmt: 'pct' }
];

function perfCell(scope, row) {
  const v = scope?.[row.key];
  if (v == null) return '–';
  switch (row.fmt) {
    case 'neg':
      return v > 0 ? `−${fmtNum(v)}` : fmtNum(0);
    case 'pct':
      return `${fmtNum(v, 1)}%`;
    case 'pct3':
      return `${fmtNum(v, 3)}%`;
    case 'int':
      return fmtNum(v, 0);
    case 'bars':
      return fmtNum(v, 1);
    default:
      return fmtNum(v);
  }
}

/** Entries the engine refused and bars it sat out: the fine print behind the numbers. */
function executionNotes(r) {
  const out = [];
  if (r.skipped_min_size) out.push(`${r.skipped_min_size} entries refused: below the instrument's minimum size after lot rounding.`);
  if (r.skipped_margin) out.push(`${r.skipped_margin} entries refused: required margin exceeded available equity.`);
  if (r.halted_bars) out.push(`${r.halted_bars} bars with new entries halted by a circuit breaker.`);
  if (r.filtered_bars) out.push(`${r.filtered_bars} bars outside the trading window (session, weekday or date filters).`);
  if (r.total_funding) out.push(`Funding is an estimate at a flat annual rate: ${fmtNum(r.total_funding)} over the run.`);
  return out;
}

/** [first, last] day of the simulated window, from the equity curve. */
function periodOf(r) {
  const eq = r.equity ?? [];
  if (eq.length < 2) return null;
  return [dayOf(eq[0].ts), dayOf(eq[eq.length - 1].ts)];
}

/** "2024-01-01 → 2024-06-30 · 4,320 bars · 200-bar warm-up (trading from 2024-01-09)". */
function contextLine(r, period) {
  const bits = [];
  if (period) bits.push(`${period[0]} → ${period[1]}`);
  if (r.bars != null) bits.push(`${fmtNum(r.bars, 0)} bars`);
  if (r.warmup_bars > 0)
    bits.push(`${r.warmup_bars}-bar warm-up${r.trading_start_ts ? ` (trading from ${dayOf(r.trading_start_ts)})` : ''}`);
  return bits.join(' · ');
}

const dayOf = (ts) => String(ts ?? '').split('T')[0];
const prettyKey = (k) => {
  const s = k.replace(/_/g, ' ');
  return s.charAt(0).toUpperCase() + s.slice(1);
};

/** Unicode sparkline over at most 60 buckets (mean per bucket). */
function sparkline(ys) {
  const BARS = '▁▂▃▄▅▆▇█';
  const min = Math.min(...ys);
  const span = Math.max(Math.max(...ys) - min, Number.EPSILON);
  const buckets = Math.min(ys.length, 60);
  let line = '';
  for (let b = 0; b < buckets; b++) {
    const lo = Math.floor((b * ys.length) / buckets);
    const hi = Math.max(Math.floor(((b + 1) * ys.length) / buckets), lo + 1);
    const slice = ys.slice(lo, hi);
    const mean = slice.reduce((a, v) => a + v, 0) / slice.length;
    line += BARS[Math.min(7, Math.max(0, Math.round(((mean - min) / span) * 7)))];
  }
  return line;
}

/** Render a signal group (entry/exit) as a bullet list of human-readable conditions. */
function groupLines(g, label) {
  const conds = g?.conditions ?? [];
  if (!conds.length) return [];
  const joiner = g.logic === 'any' ? 'ANY of' : 'ALL of';
  const out = [`- **${label}** (${joiner}):`];
  for (const c of conds) out.push(`  - ${conditionText(c)}`);
  return out;
}

/** Build the "## Strategy" section: type, parameters, capital/fees, and long/short signals. */
function strategyReportSection(s) {
  const L = ['## Strategy', ''];

  // Parameters table.
  L.push('| Parameter | Value |', '|---|---|');
  const p = (k, v) => v != null && v !== '' && L.push(`| ${k} | ${v} |`);
  p('Type', s.kind === 'grid' ? 'Grid' : 'Signals');
  p('Direction', s.mode);
  if (s.reverse_side) p('Short side', 'mirror of long (inverse)');
  if (s.stop_and_reverse) p('Stop & reverse', 'on');
  p('Pyramiding', s.pyramiding);
  p('Starting capital', fmtNum(s.starting_capital));
  p('Leverage', s.leverage != null ? `${s.leverage}×` : null);
  if (s.sizing)
    p(
      'Position sizing',
      s.sizing.mode === 'percent_equity'
        ? `${fmtNum(s.sizing.percent)}% of equity`
        : s.sizing.mode === 'fixed_qty'
          ? `${fmtNum(s.sizing.qty)} units${s.leverage > 1 ? ` × ${fmtNum(s.leverage)} leverage` : ''}`
          : s.sizing.mode === 'fixed_cash'
            ? `${fmtNum(s.sizing.cash)} cash`
            : s.sizing.mode
    );
  if (s.fees)
    p(
      'Fees',
      `${fmtNum(s.fees.amount)}${s.fees.amount_kind === 'pct' ? '%' : ''} per ${s.fees.per}`
    );
  if (s.spread_pct) p('Spread', `${fmtNum(s.spread_pct)}%`);
  if (s.slippage?.value) p('Slippage', s.slippage.kind === 'pct' ? `${fmtNum(s.slippage.value)}%` : `${fmtNum(s.slippage.value)} (${s.slippage.kind})`);
  if (s.instrument && (s.instrument.multiplier !== 1 || s.instrument.min_qty || s.instrument.lot_step))
    p('Instrument', `mult ${s.instrument.multiplier}, lot ${s.instrument.lot_step}, min ${s.instrument.min_qty}`);
  if (s.oos_split_pct) p('Out-of-sample split', `${fmtNum(s.oos_split_pct)}%`);
  if (s.funding?.annual_rate_pct) p('Funding', `${fmtNum(s.funding.annual_rate_pct)}%/yr every ${s.funding.interval_hours}h`);
  L.push('');

  // Grid params, if a grid strategy.
  if (s.kind === 'grid' && s.grid) {
    L.push('### Grid', '', '| Parameter | Value |', '|---|---|');
    for (const [k, v] of Object.entries(s.grid))
      if (v != null && typeof v !== 'object') L.push(`| ${k} | ${typeof v === 'number' ? fmtNum(v) : v} |`);
    L.push('');
  }

  // Entry/exit signals per side.
  for (const [label, side] of [
    ['Long', s.mode !== 'short' ? s.long : null],
    ['Short', s.mode !== 'long' ? (s.reverse_side ? inverseSide(s.long) : s.short) : null]
  ]) {
    if (!side) continue;
    const lines = [...groupLines(side.entry, 'Entry'), ...groupLines(side.exit, 'Exit')];
    const risk = [];
    if (side.stop_loss_pct != null) risk.push(`SL ${fmtNum(side.stop_loss_pct)}%`);
    if (side.take_profit_pct != null) risk.push(`TP ${fmtNum(side.take_profit_pct)}%`);
    const tr = side.trailing_stop;
    if (tr && ((tr.value ?? 0) > 0 || (tr.breakeven_pct ?? 0) > 0)) {
      const d = tr.kind === 'abs' ? fmtNum(tr.value, 4) : `${fmtNum(tr.value * 100)}%`;
      const steps = [];
      if ((tr.activate_pct ?? 0) > 0) steps.push(`from +${fmtNum(tr.activate_pct * 100)}%`);
      if ((tr.breakeven_pct ?? 0) > 0) steps.push(`BE at +${fmtNum(tr.breakeven_pct * 100)}%`);
      // A distance of 0 with a breakeven step is a legal plan: it is a breakeven rule, not a trail.
      const head = (tr.value ?? 0) > 0 ? `Trail ${d}` : 'Breakeven stop';
      risk.push(steps.length ? `${head} (${steps.join(', ')})` : head);
    }
    if (side.exit_on_reverse) risk.push('exit on reverse signal');
    if (!lines.length && !risk.length) continue;
    L.push(`### ${label} side`, '');
    L.push(...lines);
    if (risk.length) L.push(`- **Risk:** ${risk.join(' · ')}`);
    L.push('');
  }

  return L;
}

/** Filename-safe form of a user-chosen name: word characters, everything else one underscore. */
function slugName(name) {
  return String(name ?? '')
    .replace(/[^a-zA-Z0-9-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase()
    .slice(0, 60);
}

/** `my_strategy_20260818-094500.md`: the strategy the run came from, plus the export moment,
 *  so two exports of the same strategy never collide. Unnamed strategies get `otw_strategy`. */
export function reportFilename(strategyName, ext = 'md') {
  const d = new Date();
  const p = (n, w = 2) => String(n).padStart(w, '0');
  const stamp = `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
  return `${slugName(strategyName) || 'otw_strategy'}_${stamp}.${ext}`;
}

/** Trigger a browser download of `text` as `filename`. */
export function downloadText(filename, text, mime = 'text/markdown') {
  const blob = new Blob([text], { type: `${mime};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
