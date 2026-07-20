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

  /** Run a backtest over one or more datasets. `datasetIds` is an array (single-asset = one
   *  element). Returns { trades, equity, stats, per_asset, warmup_bars, alignment, … }. */
  run: (datasetIds, settings, { record = true } = {}) =>
    req('/backtest/run', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids: datasetIds, settings, record })
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

  // ── Expert-mode library: strategies (named Settings) ──
  strategies: () => req('/backtest/strategies').then((r) => r.strategies),
  getStrategy: (id) => req(`/backtest/strategies/${id}`).then((r) => r.strategy),
  createStrategy: (name, description, settings) =>
    req('/backtest/strategies', { method: 'POST', body: JSON.stringify({ name, description, settings }) }),
  updateStrategy: (id, name, description, settings) =>
    req(`/backtest/strategies/${id}`, { method: 'PUT', body: JSON.stringify({ name, description, settings }) }),
  deleteStrategy: (id) => req(`/backtest/strategies/${id}`, { method: 'DELETE' }),

  // ── Expert-mode library: custom indicators (node-graph DAG) ──
  indicators: () => req('/backtest/indicators').then((r) => r.indicators),
  getIndicator: (id) => req(`/backtest/indicators/${id}`).then((r) => r.indicator),
  createIndicator: (name, description, definition) =>
    req('/backtest/indicators', { method: 'POST', body: JSON.stringify({ name, description, definition }) }),
  updateIndicator: (id, name, description, definition) =>
    req(`/backtest/indicators/${id}`, { method: 'PUT', body: JSON.stringify({ name, description, definition }) }),
  deleteIndicator: (id) => req(`/backtest/indicators/${id}`, { method: 'DELETE' })
};

/** Custom-indicator node op catalog. `inputs` = how many earlier steps a node references;
 *  `params` = numeric params on the node itself. Mirrors backtest::custom::Node. */
export const NODE_OPS = [
  { op: 'price', label: 'Price series', inputs: 0, field: true },
  { op: 'const', label: 'Constant', inputs: 0, params: [{ key: 'value', label: 'Value', def: 0 }] },
  { op: 'indicator', label: 'Built-in indicator', inputs: 0, indicator: true },
  { op: 'add', label: 'A + B', inputs: 2 },
  { op: 'sub', label: 'A − B', inputs: 2 },
  { op: 'mul', label: 'A × B', inputs: 2 },
  { op: 'div', label: 'A ÷ B', inputs: 2 },
  { op: 'min', label: 'min(A, B)', inputs: 2 },
  { op: 'max', label: 'max(A, B)', inputs: 2 },
  { op: 'abs', label: '|A|', inputs: 1 },
  { op: 'neg', label: '−A', inputs: 1 },
  { op: 'shift', label: 'Shift A by n', inputs: 1, params: [{ key: 'n', label: 'Bars', def: 1 }] },
  { op: 'sma_of', label: 'SMA of A', inputs: 1, params: [{ key: 'period', label: 'Length', def: 5 }] },
  { op: 'ema_of', label: 'EMA of A', inputs: 1, params: [{ key: 'period', label: 'Length', def: 5 }] },
  { op: 'highest', label: 'Highest A (n)', inputs: 1, params: [{ key: 'period', label: 'Length', def: 5 }] },
  { op: 'lowest', label: 'Lowest A (n)', inputs: 1, params: [{ key: 'period', label: 'Length', def: 5 }] },
  { op: 'change', label: 'Change of A (n)', inputs: 1, params: [{ key: 'period', label: 'Length', def: 1 }] },
  { op: 'clamp', label: 'Clamp A [lo, hi]', inputs: 1, params: [{ key: 'lo', label: 'Low', def: 0 }, { key: 'hi', label: 'High', def: 100 }] }
];

export const nodeOpById = (op) => NODE_OPS.find((n) => n.op === op);

/** Single-series indicators that can be chained onto an earlier step's output (mirrors the Rust
 *  `indicators::is_chainable`). Others read OHLC and only apply to the price. */
export const CHAINABLE_INDICATORS = new Set([
  'sma', 'ema', 'dema', 'tema', 'wma', 'hma', 'rsi', 'roc', 'momentum', 'stddev',
  'macd', 'macd_signal', 'macd_hist'
]);
export const isChainableIndicator = (id) => CHAINABLE_INDICATORS.has(id);

/** A fresh node of a given op with sensible defaults (refs default to step 0). */
export function defaultNode(op) {
  const spec = nodeOpById(op) ?? NODE_OPS[0];
  const node = { op };
  if (spec.field) node.field = 'close';
  if (spec.indicator) Object.assign(node, { indicator: 'rsi', period: 14 });
  if (spec.inputs >= 1) node.a = 0;
  if (spec.inputs >= 2) node.b = 0;
  for (const p of spec.params ?? []) node[p.key] = p.def;
  return node;
}

/** A fresh empty custom-indicator definition (one price source, output = it). */
export function defaultIndicatorDef() {
  return { nodes: [{ op: 'price', field: 'close' }], output: 0 };
}

// ── v2 builder model: named steps + explicit sources + formulas ↔ DAG ──
//
// The saved/engine format stays the node-index DAG (`{ nodes, output }`). The v2 builder edits a
// friendlier model — a list of *named* steps where each step is an indicator applied to a named
// source (`@close`, `@volume`, or an earlier step) or a math formula referencing steps by `@name`
// — and compiles it down. `defToModel` reverses a DAG back into that model when possible; a def it
// can't express as named steps (rare hand-built graphs) is flagged so the UI can fall back.

/** OHLCV field names usable as operands and as v2 indicator sources. */
export const PRICE_FIELDS = ['close', 'open', 'high', 'low', 'volume'];
/** Price fields as `@token`s for the v2 source picker. */
export const PRICE_TOKENS = PRICE_FIELDS.map((f) => '@' + f);

let _uid = 0;
const freshId = () => `s${++_uid}`;

/** A fresh v2 model: one indicator step on close, marked as the output. */
export function defaultBuilderModel() {
  const id = freshId();
  return { steps: [{ id, name: 'ind1', kind: 'ind', indicator: 'rsi', src: '@close', period: 14 }], outputs: [id] };
}

/** Indicators that emit several series; `.sub` selects one when used as a source/output. */
export const MULTI_OUTPUTS = { macd: ['line', 'signal', 'hist'] };

const indParamKeys = (id) => (indicatorById(id)?.params ?? []).map((p) => p.key);

/** Tokenise a formula into numbers, @refs, operators, parens, and function names. */
function tokenizeFormula(expr) {
  const out = [];
  const re = /\s*(@[a-zA-Z_][\w.]*|[a-zA-Z_]\w*|\d*\.?\d+|[()+\-*/,])/y;
  let m;
  let last = 0;
  while ((m = re.exec(expr))) {
    out.push(m[1]);
    last = re.lastIndex;
  }
  if (last !== expr.trim().length && expr.trim().length) {
    // trailing junk — surface as an error token
    const rest = expr.slice(last).trim();
    if (rest) out.push({ bad: rest });
  }
  return out;
}

/** Functions usable in a formula, arity-checked. Compiled to unary/binary transform nodes. */
const FORMULA_FUNCS = {
  abs: 1, neg: 1, sqrt: 1, log: 1,
  min: 2, max: 2,
  clamp: 3
};

/** Recursive-descent parser: formula string + resolver(name→nodeIndex) → node index in `nodes`.
 *  Emits const/transform nodes into `nodes`, returns the index of the result (or throws). */
function compileFormula(expr, resolve, nodes) {
  const toks = tokenizeFormula(expr);
  let pos = 0;
  const peek = () => toks[pos];
  const next = () => toks[pos++];
  const push = (node) => (nodes.push(node), nodes.length - 1);
  const err = (msg) => {
    throw new Error(msg);
  };

  function parseExpr() {
    let left = parseTerm();
    while (peek() === '+' || peek() === '-') {
      const op = next();
      const right = parseTerm();
      left = push({ op: op === '+' ? 'add' : 'sub', a: left, b: right });
    }
    return left;
  }
  function parseTerm() {
    let left = parseUnary();
    while (peek() === '*' || peek() === '/') {
      const op = next();
      const right = parseUnary();
      left = push({ op: op === '*' ? 'mul' : 'div', a: left, b: right });
    }
    return left;
  }
  function parseUnary() {
    if (peek() === '-') {
      next();
      const a = parseUnary();
      return push({ op: 'neg', a });
    }
    return parseAtom();
  }
  function parseAtom() {
    const t = next();
    if (t == null) err('formule incomplète');
    if (typeof t === 'object') err(`caractères invalides : « ${t.bad} »`);
    if (t === '(') {
      const e = parseExpr();
      if (next() !== ')') err('parenthèse fermante manquante');
      return e;
    }
    if (t.startsWith('@')) return resolve(t);
    if (/^\d*\.?\d+$/.test(t)) return push({ op: 'const', value: parseFloat(t) });
    if (t in FORMULA_FUNCS) {
      if (next() !== '(') err(`« ${t} » attend des parenthèses`);
      // clamp needs literal bounds → capture raw number tokens for args 2 & 3.
      const wantLiteralTail = t === 'clamp';
      const args = [parseExpr()];
      const literals = [];
      while (peek() === ',') {
        next();
        if (wantLiteralTail) {
          const lit = next();
          if (typeof lit !== 'string' || !/^-?\d*\.?\d+$/.test(lit)) err('clamp(x, min, max) : bornes numériques');
          literals.push(parseFloat(lit));
        } else {
          args.push(parseExpr());
        }
      }
      if (next() !== ')') err(`parenthèse fermante manquante après « ${t} »`);
      if (t === 'clamp') {
        if (literals.length !== 2) err('clamp(x, min, max) attend 3 arguments');
        return push({ op: 'clamp', a: args[0], lo: literals[0], hi: literals[1] });
      }
      if (args.length !== FORMULA_FUNCS[t]) err(`« ${t} » attend ${FORMULA_FUNCS[t]} argument(s)`);
      return compileFunc(t, args, push);
    }
    err(`référence inconnue : « ${t} »`);
  }
  const result = parseExpr();
  if (pos < toks.length) err(`texte en trop : « ${toks.slice(pos).join(' ')} »`);
  return result;
}

function compileFunc(name, args, push) {
  switch (name) {
    case 'abs': return push({ op: 'abs', a: args[0] });
    case 'neg': return push({ op: 'neg', a: args[0] });
    case 'min': return push({ op: 'min', a: args[0], b: args[1] });
    case 'max': return push({ op: 'max', a: args[0], b: args[1] });
    // sqrt/log aren't DAG ops yet — guarded here until the engine gains them.
    case 'sqrt': throw new Error('sqrt pas encore supporté');
    case 'log': throw new Error('log pas encore supporté');
    default: throw new Error(`fonction inconnue : ${name}`);
  }
}

/** Compile a v2 model → DAG `{ nodes, output }` (+ `outputs` list). Throws on a bad formula/ref so
 *  the UI can show a precise message. Ensures every referenced source resolves to an earlier node. */
export function modelToDef(model) {
  const nodes = [];
  const nameToIndex = new Map(); // step name (or "name.sub") → node index
  const priceIndex = new Map(); // '@close' → node index (created lazily, deduped)

  const priceNode = (field) => {
    if (!priceIndex.has(field)) {
      nodes.push({ op: 'price', field });
      priceIndex.set(field, nodes.length - 1);
    }
    return priceIndex.get(field);
  };

  const resolveRef = (token) => {
    const bare = token.replace(/^@/, '');
    if (PRICE_FIELDS.includes(bare)) return priceNode(bare);
    if (nameToIndex.has(bare)) return nameToIndex.get(bare);
    throw new Error(`référence introuvable : « ${token} » (définis-la plus haut)`);
  };

  for (const step of model.steps) {
    if (!step.name || !/^[a-zA-Z_]\w*$/.test(step.name)) {
      throw new Error(`nom d'étape invalide : « ${step.name ?? ''} »`);
    }
    if (nameToIndex.has(step.name)) throw new Error(`nom en double : « ${step.name} »`);

    let idx;
    if (step.kind === 'formula') {
      idx = compileFormula(step.expr ?? '', resolveRef, nodes);
    } else {
      const srcToken = step.src ?? '@close';
      const bare = srcToken.replace(/^@/, '');
      const isPrice = PRICE_FIELDS.includes(bare);
      const chainable = isChainableIndicator(step.indicator);
      const node = { op: 'indicator', indicator: step.indicator };
      for (const k of indParamKeys(step.indicator)) node[k] = step[k];

      if (isPrice && bare === 'close') {
        // Default OHLC path: the engine's Indicator node reads the bars directly.
      } else if (isPrice && !chainable) {
        // ATR/Stoch/etc. read full OHLC anyway; a non-close price source is meaningless → keep the
        // OHLC-reading node. (Only `@close` is offered for these in the UI.)
      } else {
        // Chained: either onto an earlier step, or onto a non-close price field (@volume, @high…).
        // Both become `Indicator{ src }` over the referenced series; only chainable names allow it.
        if (!chainable) {
          throw new Error(`« ${indicatorById(step.indicator)?.label ?? step.indicator} » ne peut pas s'appliquer à une autre étape (il lit les bougies OHLC)`);
        }
        node.src = resolveRef(srcToken);
      }
      nodes.push(node);
      idx = nodes.length - 1;
    }
    nameToIndex.set(step.name, idx);
    // register multi-output sub-series names
    for (const sub of MULTI_OUTPUTS[step.indicator] ?? []) nameToIndex.set(`${step.name}.${sub}`, idx);
  }

  const outNames = model.outputs?.length ? model.outputs : [model.steps.at(-1)?.name].filter(Boolean);
  const outIdx = outNames.map((n) => nameToIndex.get(n)).filter((i) => i != null);
  return { nodes, output: outIdx[0] ?? nodes.length - 1, outputs: outIdx };
}

/** Best-effort reverse: DAG → v2 model. Works for defs the v2 builder itself produces (indicator
 *  and formula steps). Returns `{ model }` or `{ unsupported: true }` for graphs it can't name. */
export function defToModel(def) {
  // A node is "nameable" as a step if it's an indicator or the root of a formula. We take a simple
  // route: expose every indicator node as a step; treat any transform chain as one formula step
  // rooted at the output. This round-trips the shapes the builder emits.
  if (!def?.nodes?.length) return { model: defaultBuilderModel() };
  const { nodes } = def;
  const stepName = new Map(); // node index → @token usable in formulas
  const steps = [];
  let counter = 0;
  const nextName = (base) => `${base}${++counter}`;

  const tokenFor = (i) => {
    const n = nodes[i];
    if (n.op === 'price') return '@' + n.field;
    if (stepName.has(i)) return '@' + stepName.get(i);
    return null; // inline (const/transform) — rendered into the formula
  };

  const isTransform = (op) => !['price', 'const', 'indicator'].includes(op);

  const inlineExpr = (i) => {
    const n = nodes[i];
    if (n.op === 'price') return '@' + n.field;
    if (n.op === 'const') return String(n.value);
    if (stepName.has(i)) return '@' + stepName.get(i);
    const A = () => inlineExpr(n.a);
    const B = () => inlineExpr(n.b);
    switch (n.op) {
      case 'add': return `(${A()} + ${B()})`;
      case 'sub': return `(${A()} - ${B()})`;
      case 'mul': return `(${A()} * ${B()})`;
      case 'div': return `(${A()} / ${B()})`;
      case 'min': return `min(${A()}, ${B()})`;
      case 'max': return `max(${A()}, ${B()})`;
      case 'abs': return `abs(${A()})`;
      case 'neg': return `neg(${A()})`;
      case 'clamp': return `clamp(${A()}, ${n.lo}, ${n.hi})`;
      default: return '@?'; // sma_of/shift/etc. — represented as their own step below
    }
  };

  for (let i = 0; i < nodes.length; i++) {
    const n = nodes[i];
    if (n.op === 'price' || n.op === 'const') continue;
    if (n.op === 'indicator') {
      const name = nextName(n.indicator);
      const step = { id: freshId(), name, kind: 'ind', indicator: n.indicator, src: n.src != null ? tokenFor(n.src) ?? '@close' : '@close' };
      for (const k of indParamKeys(n.indicator)) step[k] = n[k] ?? indicatorById(n.indicator)?.params.find((p) => p.key === k)?.def;
      steps.push(step);
      stepName.set(i, name);
    } else if (isTransform(n.op)) {
      // Only name a transform step if something else references it or it's the output; otherwise it
      // gets folded into a formula inline. To keep it simple + robust, expose it as a formula step.
      const name = nextName('f');
      steps.push({ id: freshId(), name, kind: 'formula', expr: inlineExpr(i) });
      stepName.set(i, name);
    }
  }

  if (!steps.length) return { model: defaultBuilderModel() };
  const outputs = [];
  const outName = stepName.get(def.output);
  if (outName) outputs.push(outName);
  for (const oi of def.outputs ?? []) {
    const nm = stepName.get(oi);
    if (nm && !outputs.includes(nm)) outputs.push(nm);
  }
  return { model: { steps, outputs: outputs.length ? outputs : [steps.at(-1).name] } };
}

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
    stop_above: 0
  };
}

export function defaultSettings() {
  return {
    kind: 'signals', // 'signals' | 'grid'
    grid: null,
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
    funding: { annual_rate_pct: 0, interval_hours: 8 }
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
  out.risk ??= {};
  out.sizing ??= { mode: 'percent_equity', percent: 100 };
  out.pyramid_steps ??= { scale: [], min_distance_pct: 0, after_add_sl: 'none' };
  out.instrument ??= { multiplier: 1, lot_step: 0, min_qty: 0 };
  out.slippage ??= { kind: 'pct', value: 0, tick_size: 0 };
  out.oos_split_pct ??= 0;
  out.kind ??= 'signals';
  out.grid ??= null;
  out.funding ??= { annual_rate_pct: 0, interval_hours: 8 };
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

export const fmtNum = (n, d = 2) =>
  n == null || Number.isNaN(n)
    ? '–'
    : Number(n).toLocaleString(undefined, { maximumFractionDigits: d });

/** Build a Markdown report from an in-memory run result (for unsaved results). Mirrors the
 *  server report shape closely enough for a convenience export; the server endpoint stays the
 *  source of truth for saved runs. */
export function buildReportMd({ name, ticker, timeframe, stats, perAsset, settings, meta }) {
  const L = [];
  L.push('---');
  L.push(`title: "${(name || 'Backtest').replace(/"/g, "'")}"`);
  L.push(`ticker: ${ticker ?? ''}`);
  L.push(`timeframe: ${timeframe ?? ''}`);
  if (meta?.bars != null) L.push(`bars: ${meta.bars}`);
  if (meta?.period) L.push(`period: ${meta.period}`);
  if (stats?.engine_version != null) L.push(`engine_version: ${stats.engine_version}`);
  L.push(`generated: ${new Date().toISOString()}`);
  L.push('---', '');
  L.push(`# Backtest report — ${name || 'Backtest'}`, '');
  L.push(`**${ticker ?? ''}** · \`${timeframe ?? ''}\``, '');
  if (meta?.period) L.push(`Period: ${meta.period}${meta?.bars != null ? ` · ${meta.bars.toLocaleString()} bars` : ''}${meta?.warmupBars ? ` · ${meta.warmupBars}-bar warm-up` : ''}`, '');

  // ── Strategy definition (parameters, indicators, entry/exit signals) ──
  if (settings) L.push(...strategyReportSection(settings));

  L.push('## Summary', '');
  L.push('| Metric | Value |', '|---|---|');
  const row = (label, v, suffix = '') => v != null && L.push(`| ${label} | ${fmtNum(v)}${suffix} |`);
  row('Net PnL', stats?.net_pnl);
  row('Return', stats?.return_pct, '%');
  row('Buy & hold', stats?.buy_hold_return_pct, '%');
  if (stats?.trades != null) L.push(`| Trades | ${stats.trades} |`);
  row('Win rate', stats?.win_rate, '%');
  row('Profit factor', stats?.profit_factor);
  row('Expectancy / trade', stats?.expectancy);
  row('Avg win', stats?.avg_win);
  row('Avg loss', stats?.avg_loss);
  row('Max drawdown', stats?.max_drawdown_pct, '%');
  row('Sharpe', stats?.sharpe);
  row('Sortino', stats?.sortino);
  row('Total fees', stats?.total_fees);
  row('Final equity', stats?.final_equity);
  L.push('');
  if (perAsset?.length > 1) {
    L.push('## Per-asset', '', '| Asset | Trades | Win rate | Net PnL | Fees | Exposure |', '|---|---|---|---|---|---|');
    for (const a of perAsset)
      L.push(
        `| ${a.ticker} | ${a.trades} | ${fmtNum(a.win_rate, 1)}% | ${fmtNum(a.net_pnl)} | ${fmtNum(a.total_fees)} | ${fmtNum(a.exposure_pct, 1)}% |`
      );
    L.push('');
  }
  if (stats?.exit_reasons && Object.keys(stats.exit_reasons).length) {
    L.push('## Exit reasons', '', '| Reason | Count |', '|---|---|');
    for (const [k, v] of Object.entries(stats.exit_reasons).sort((a, b) => b[1] - a[1])) L.push(`| ${k} | ${v} |`);
    L.push('');
  }
  return L.join('\n');
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
    if (side.exit_on_reverse) risk.push('exit on reverse signal');
    if (!lines.length && !risk.length) continue;
    L.push(`### ${label} side`, '');
    L.push(...lines);
    if (risk.length) L.push(`- **Risk:** ${risk.join(' · ')}`);
    L.push('');
  }

  return L;
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
