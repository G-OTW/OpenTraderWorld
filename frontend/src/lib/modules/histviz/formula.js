/** Source formulas: a one-line expression over the bars, used as an indicator's input.
 *
 *   (high + low + 2 * close) / 4
 *   ema(close, 10) - sma(close, 30)
 *   rsi(hlc3, 14) / 100
 *
 * Parsed into a tree (no `eval`, nothing but what is listed here is reachable) and evaluated
 * bar by bar into one array aligned on the bars. A bar where an operand is missing comes out
 * `null`, so a 20-bar average starts at bar 20, not at a made-up number. Chart-side only. */
import { sma, ema, wma, hma, dema, rsi, roc, momentum, stddev, atr } from './indicators.js';

/** Bar series by name. `volume` is zeros when the provider serves none. */
export const FORMULA_VARS = ['open', 'high', 'low', 'close', 'volume', 'hl2', 'hlc3', 'ohlc4'];

const VAR_ALIASES = { o: 'open', h: 'high', l: 'low', c: 'close', v: 'volume' };

/** Functions. `series` arguments take any expression; `period` ones a positive whole number. */
export const FORMULA_FUNCS = {
  sma: { args: ['series', 'period'], hint: 'sma(close, 20)' },
  ema: { args: ['series', 'period'], hint: 'ema(close, 20)' },
  wma: { args: ['series', 'period'], hint: 'wma(close, 20)' },
  hma: { args: ['series', 'period'], hint: 'hma(close, 20)' },
  dema: { args: ['series', 'period'], hint: 'dema(close, 20)' },
  rsi: { args: ['series', 'period'], hint: 'rsi(close, 14)' },
  roc: { args: ['series', 'period'], hint: 'roc(close, 12)' },
  mom: { args: ['series', 'period'], hint: 'mom(close, 10)' },
  stdev: { args: ['series', 'period'], hint: 'stdev(close, 20)' },
  highest: { args: ['series', 'period'], hint: 'highest(high, 20)' },
  lowest: { args: ['series', 'period'], hint: 'lowest(low, 20)' },
  prev: { args: ['series', 'period'], hint: 'prev(close, 1)' },
  atr: { args: ['period'], hint: 'atr(14)' },
  abs: { args: ['series'], hint: 'abs(close - open)' },
  sqrt: { args: ['series'], hint: 'sqrt(volume)' },
  log: { args: ['series'], hint: 'log(close)' },
  min: { args: ['series', 'series'], hint: 'min(open, close)' },
  max: { args: ['series', 'series'], hint: 'max(open, close)' }
};

// ── Parsing ──

function tokenize(src) {
  const out = [];
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (/\s/.test(ch)) {
      i++;
    } else if (/[0-9.]/.test(ch)) {
      const m = /^\d*\.?\d+(e[+-]?\d+)?/i.exec(src.slice(i));
      if (!m) throw new Error(`bad number at ${i + 1}`);
      out.push({ t: 'num', v: Number(m[0]), at: i });
      i += m[0].length;
    } else if (/[a-z_]/i.test(ch)) {
      const m = /^[a-z_][a-z0-9_]*/i.exec(src.slice(i));
      out.push({ t: 'id', v: m[0].toLowerCase(), at: i });
      i += m[0].length;
    } else if ('+-*/^(),'.includes(ch)) {
      out.push({ t: ch, at: i });
      i++;
    } else {
      throw new Error(`unexpected '${ch}' at ${i + 1}`);
    }
  }
  return out;
}

/** Expression → tree. Throws an Error whose message says what and where. */
export function parseFormula(src) {
  const toks = tokenize(String(src ?? ''));
  if (!toks.length) throw new Error('empty formula');
  let k = 0;
  const peek = () => toks[k];
  const take = (t) => {
    const tok = toks[k];
    if (!tok || tok.t !== t) throw new Error(`expected '${t}'${tok ? ` at ${tok.at + 1}` : ' at the end'}`);
    k++;
    return tok;
  };
  // Precedence climbing: + - < * / < ^ (right-assoc) < unary.
  const BIN = { '+': 1, '-': 1, '*': 2, '/': 2, '^': 3 };
  function expr(minPrec = 1) {
    let left = unary();
    for (;;) {
      const tok = peek();
      const prec = tok && BIN[tok.t];
      if (!prec || prec < minPrec) return left;
      k++;
      const right = expr(tok.t === '^' ? prec : prec + 1);
      left = { op: tok.t, a: left, b: right };
    }
  }
  function unary() {
    const tok = peek();
    if (tok?.t === '-') {
      k++;
      // Below ^ as in math: -2^2 is -(2^2).
      return { op: 'neg', a: expr(3) };
    }
    if (tok?.t === '+') {
      k++;
      return unary();
    }
    return atom();
  }
  function atom() {
    const tok = peek();
    if (!tok) throw new Error('unexpected end of formula');
    if (tok.t === 'num') {
      k++;
      return { num: tok.v };
    }
    if (tok.t === '(') {
      k++;
      const e = expr();
      take(')');
      return e;
    }
    if (tok.t === 'id') {
      k++;
      if (peek()?.t === '(') {
        const fn = FORMULA_FUNCS[tok.v];
        if (!fn) throw new Error(`unknown function '${tok.v}'`);
        k++;
        const args = [];
        if (peek()?.t !== ')') {
          args.push(expr());
          while (peek()?.t === ',') {
            k++;
            args.push(expr());
          }
        }
        take(')');
        if (args.length !== fn.args.length) {
          throw new Error(`${tok.v}() takes ${fn.args.length} argument${fn.args.length > 1 ? 's' : ''}`);
        }
        fn.args.forEach((kind, j) => {
          if (kind !== 'period') return;
          const n = args[j].num;
          if (!(Number.isInteger(n) && n > 0 && n <= 5000)) {
            throw new Error(`${tok.v}(): the period must be a whole number between 1 and 5000`);
          }
        });
        return { fn: tok.v, args };
      }
      const name = VAR_ALIASES[tok.v] ?? tok.v;
      if (!FORMULA_VARS.includes(name)) throw new Error(`unknown series '${tok.v}'`);
      return { v: name };
    }
    throw new Error(`unexpected '${tok.t}' at ${tok.at + 1}`);
  }
  const tree = expr();
  if (k < toks.length) throw new Error(`unexpected '${toks[k].v ?? toks[k].t}' at ${toks[k].at + 1}`);
  return tree;
}

/** `null` when the formula parses, else the reason it does not. */
export function formulaError(src) {
  try {
    parseFormula(src);
    return null;
  } catch (e) {
    return e.message;
  }
}

// ── Evaluation ──

const ok = (x) => x != null && Number.isFinite(x);

/** Run a series function over the defined tail of `x` (leading nulls kept), forward-filling
 *  any gap inside it: the math below expects a dense series. */
function onTail(x, f) {
  const start = x.findIndex(ok);
  const out = new Array(x.length).fill(null);
  if (start < 0) return out;
  let last = x[start];
  const tail = x.slice(start).map((v) => (ok(v) ? (last = v) : last));
  const r = f(tail);
  for (let i = 0; i < r.length; i++) out[start + i] = ok(r[i]) ? r[i] : null;
  return out;
}

function rolling(x, n, pick) {
  return onTail(x, (t) =>
    t.map((_, i) => {
      if (i < n - 1) return null;
      let m = t[i];
      for (let j = i - n + 1; j < i; j++) m = pick(m, t[j]);
      return m;
    })
  );
}

function evalNode(node, bars, len) {
  if (node.num != null) return new Array(len).fill(node.num);
  if (node.v) return seriesOf(bars, node.v);
  if (node.op === 'neg') return evalNode(node.a, bars, len).map((x) => (ok(x) ? -x : null));
  if (node.op) {
    const a = evalNode(node.a, bars, len);
    const b = evalNode(node.b, bars, len);
    const f = { '+': (x, y) => x + y, '-': (x, y) => x - y, '*': (x, y) => x * y, '/': (x, y) => (y === 0 ? null : x / y), '^': (x, y) => x ** y }[node.op];
    return a.map((x, i) => {
      if (!ok(x) || !ok(b[i])) return null;
      const r = f(x, b[i]);
      return ok(r) ? r : null;
    });
  }
  const arg = (j) => evalNode(node.args[j], bars, len);
  const n = node.args[1]?.num;
  switch (node.fn) {
    case 'sma':
      return onTail(arg(0), (t) => sma(t, n));
    case 'ema':
      return onTail(arg(0), (t) => ema(t, n));
    case 'wma':
      return onTail(arg(0), (t) => wma(t, n));
    case 'hma':
      return onTail(arg(0), (t) => hma(t, n));
    case 'dema':
      return onTail(arg(0), (t) => dema(t, n));
    case 'rsi':
      return onTail(arg(0), (t) => rsi(t, n));
    case 'roc':
      return onTail(arg(0), (t) => roc(t, n));
    case 'mom':
      return onTail(arg(0), (t) => momentum(t, n));
    case 'stdev':
      return onTail(arg(0), (t) => stddev(t, n));
    case 'highest':
      return rolling(arg(0), n, Math.max);
    case 'lowest':
      return rolling(arg(0), n, Math.min);
    case 'prev': {
      const x = arg(0);
      return x.map((_, i) => (i >= n && ok(x[i - n]) ? x[i - n] : null));
    }
    case 'atr':
      return atr(bars.h, bars.l, bars.c, node.args[0].num).map((x) => (ok(x) ? x : null));
    case 'abs':
      return arg(0).map((x) => (ok(x) ? Math.abs(x) : null));
    case 'sqrt':
      return arg(0).map((x) => (ok(x) && x >= 0 ? Math.sqrt(x) : null));
    case 'log':
      return arg(0).map((x) => (ok(x) && x > 0 ? Math.log(x) : null));
    case 'min':
    case 'max': {
      const a = arg(0);
      const b = arg(1);
      const f = node.fn === 'min' ? Math.min : Math.max;
      return a.map((x, i) => (ok(x) && ok(b[i]) ? f(x, b[i]) : null));
    }
    default:
      return new Array(len).fill(null);
  }
}

function seriesOf(b, name) {
  switch (name) {
    case 'open':
      return b.o;
    case 'high':
      return b.h;
    case 'low':
      return b.l;
    case 'volume':
      return b.v ?? b.c.map(() => 0);
    case 'hl2':
      return b.c.map((_, i) => (b.h[i] + b.l[i]) / 2);
    case 'hlc3':
      return b.c.map((_, i) => (b.h[i] + b.l[i] + b.c[i]) / 3);
    case 'ohlc4':
      return b.c.map((_, i) => (b.o[i] + b.h[i] + b.l[i] + b.c[i]) / 4);
    default:
      return b.c;
  }
}

/** Evaluate a formula over bars `{o,h,l,c,v}`: one value (or null) per bar. An invalid
 *  formula yields all nulls rather than throwing mid-render. */
export function evalFormula(src, bars) {
  const len = bars?.c?.length ?? 0;
  try {
    return evalNode(parseFormula(src), bars, len);
  } catch {
    return new Array(len).fill(null);
  }
}
