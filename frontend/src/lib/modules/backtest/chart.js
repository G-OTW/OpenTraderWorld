/** What the result chart draws on top of the candles: the strategy's own indicators, and the
 *  trades the run produced.
 *
 * The result chart *is* the visualization module's chart, so both have to be expressed in its
 * vocabulary. That is not a rename: the strategy speaks in single series (`bb_upper`,
 * `macd_signal`, `stoch_k`), the chart in drawable objects (one Bollinger, one MACD pane).
 * An operand the chart has no object for is dropped rather than approximated, so a line on
 * screen is always a line the engine actually read.
 */
import { catalogDef, DEFAULT_STYLE } from '$lib/modules/histviz/indicators.js';

const num = (v, fb) => (Number.isFinite(Number(v)) ? Number(v) : fb);

/** Operand indicator id → [chart type, params]. Several operands map to the same object
 *  (the three Bollinger bands are one instance), which the dedup below collapses. */
const MAP = {
  sma: (o) => ['sma', { period: num(o.period, 20) }],
  ema: (o) => ['ema', { period: num(o.period, 20) }],
  dema: (o) => ['dema', { period: num(o.period, 20) }],
  tema: (o) => ['tema', { period: num(o.period, 20) }],
  wma: (o) => ['wma', { period: num(o.period, 20) }],
  hma: (o) => ['hma', { period: num(o.period, 20) }],
  vwap: (o) => ['vwap', { period: num(o.period, 20) }],
  supertrend: (o) => ['supertrend', { period: num(o.period, 10), mult: num(o.mult, 3) }],
  psar: (o) => ['psar', { step: num(o.mult, 0.02), max: 0.2 }],

  rsi: (o) => ['rsi', { period: num(o.period, 14) }],
  stoch_k: (o) => ['stochastic', { k: num(o.period, 14), d: num(o.signal_period, 3) }],
  stoch_d: (o) => ['stochastic', { k: num(o.period, 14), d: num(o.signal_period, 3) }],
  cci: (o) => ['cci', { period: num(o.period, 20) }],
  willr: (o) => ['williamsr', { period: num(o.period, 14) }],
  roc: (o) => ['roc', { period: num(o.period, 12) }],
  momentum: (o) => ['momentum', { period: num(o.period, 10) }],
  adx: (o) => ['adx', { period: num(o.period, 14) }],
  macd: (o) => ['macd', { fast: num(o.fast, 12), slow: num(o.slow, 26), signal: num(o.signal_period, 9) }],
  macd_signal: (o) => ['macd', { fast: num(o.fast, 12), slow: num(o.slow, 26), signal: num(o.signal_period, 9) }],
  macd_hist: (o) => ['macd', { fast: num(o.fast, 12), slow: num(o.slow, 26), signal: num(o.signal_period, 9) }],

  atr: (o) => ['atr', { period: num(o.period, 14) }],
  stddev: (o) => ['stddev', { period: num(o.period, 20) }],
  bb_upper: (o) => ['bollinger', { period: num(o.period, 20), mult: num(o.mult, 2) }],
  bb_mid: (o) => ['bollinger', { period: num(o.period, 20), mult: num(o.mult, 2) }],
  bb_lower: (o) => ['bollinger', { period: num(o.period, 20), mult: num(o.mult, 2) }],
  keltner_upper: (o) => ['keltner', { period: num(o.period, 20), mult: num(o.mult, 2) }],
  keltner_lower: (o) => ['keltner', { period: num(o.period, 20), mult: num(o.mult, 2) }],
  donchian_upper: (o) => ['donchian', { period: num(o.period, 20) }],
  donchian_mid: (o) => ['donchian', { period: num(o.period, 20) }],
  donchian_lower: (o) => ['donchian', { period: num(o.period, 20) }],

  mfi: (o) => ['mfi', { period: num(o.period, 14) }],
  obv: () => ['obv', {}]
};

/**
 * Chart instances for a strategy: every indicator its conditions reference, plus the line an
 * anchored grid is built around.
 *
 * @param settings the strategy settings (editor shape)
 * @param library  the custom-indicator rows, `[{ id, name, definition }]`
 */
export function strategyInstances(settings, library = []) {
  if (!settings) return [];
  const out = [];
  const seen = new Set();
  let id = 1;

  const push = (type, params) => {
    if (!catalogDef(type)) return;
    const key = `${type}|${JSON.stringify(params)}`;
    if (seen.has(key)) return;
    seen.add(key);
    out.push({ id: id++, type, params, style: { ...DEFAULT_STYLE }, visible: true });
  };
  const pushCustom = (rowId) => {
    const row = library.find((r) => r.id === rowId);
    if (!row || seen.has(`custom|${rowId}`)) return;
    seen.add(`custom|${rowId}`);
    // Pane of its own: a strategy carries no display choice, and a custom definition rarely
    // shares the price scale.
    out.push({
      id: id++,
      type: 'custom',
      pane: 'pane',
      custom: { id: row.id, name: row.name, def: row.definition },
      visible: true
    });
  };

  const operand = (o) => {
    if (!o) return;
    if (o.kind === 'custom_indicator') return pushCustom(o.id);
    if (o.kind !== 'indicator') return;
    const built = MAP[o.indicator]?.(o);
    if (built) push(built[0], built[1]);
  };
  const group = (g) => (g?.conditions ?? []).forEach((c) => {
    operand(c.left);
    operand(c.right);
  });

  if (settings.kind === 'grid') {
    const g = settings.grid;
    if (g?.anchor && g.anchor !== 'none') push(g.anchor, { period: num(g.anchor_period, 20) });
  } else if (settings.kind === 'dca') {
    for (const rule of [...(settings.dca?.buys ?? []), ...(settings.dca?.sells ?? [])]) group(rule.condition);
  } else {
    for (const side of [settings.long, settings.short]) {
      if (!side) continue;
      group(side.entry);
      group(side.exit);
    }
  }
  return out;
}

/**
 * Trades → chart markers, the same shape a quick backtest posts: a triangle at the fill price,
 * pointing the way the fill went. The exit is the opposite side of the entry (a long is closed
 * by a sell), and is drawn hollow because its `role` is not an entry.
 */
/**
 * DCA fills → chart markers. A savings plan has no round trips to draw: what happened is a
 * series of buys (and the occasional sell), each at its own price, so the fill list is marked
 * directly instead of being folded into entry/exit pairs. Only the charted asset's fills are
 * kept: the chart shows one instrument at a time.
 */
export function dcaMarks(events = [], ticker = '') {
  const out = [];
  events.forEach((e, i) => {
    if (ticker && e.ticker && e.ticker !== ticker) return;
    const x = Date.parse(e.ts);
    if (!Number.isFinite(x)) return;
    const buy = e.action !== 'sell';
    out.push({
      id: `d${i}`,
      x,
      y: e.price,
      side: buy ? 'long' : 'short',
      qty: e.qty,
      role: buy ? 'entry' : 'exit'
    });
  });
  return out;
}

export function tradeMarks(trades = []) {
  const out = [];
  trades.forEach((t, i) => {
    const entry = t.direction === 'short' ? 'short' : 'long';
    const exit = entry === 'long' ? 'short' : 'long';
    const at = Date.parse(t.entry_ts);
    const xt = Date.parse(t.exit_ts);
    if (Number.isFinite(at)) out.push({ id: `${i}e`, x: at, y: t.entry_price, side: entry, qty: t.qty, role: 'entry' });
    if (Number.isFinite(xt)) out.push({ id: `${i}x`, x: xt, y: t.exit_price, side: exit, qty: t.qty, role: 'exit', realized: t.pnl });
  });
  return out;
}
