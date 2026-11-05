/** Custom-indicator definitions: the node-graph DAG shared by every module that draws or
 *  evaluates one (backtest engine, chart overlays).
 *
 *  The stored/engine format is an index DAG (`{ nodes, output }`, mirrored in Rust by
 *  `backtest::custom::CustomIndicatorDef`). The builder UI edits a friendlier model — named
 *  steps with explicit sources and text formulas — and compiles it down here. This file owns
 *  both the built-in indicator catalog (same ids/params as the Rust `resolve_builtin`) and the
 *  model↔DAG compiler; `evaluate.js` next door turns a DAG into a series in the browser.
 */


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
