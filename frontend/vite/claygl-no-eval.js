// claygl (the WebGL engine under echarts-gl) sizes its post-effect textures from strings
// like `expr(width * dpr / 4)` and compiles them with `new Function`. The app's CSP has no
// 'unsafe-eval', so that throws and the 3D chart never draws. This plugin swaps the eval
// for a small parser of the same grammar (numbers, width / height / dpr, + - * /, brackets,
// `[a, b]` pairs) at build time, so the CSP stays as strict as it is.

/**
 * Compile a size expression into `(width, height, dpr) => number | number[]`.
 * Self-contained: its source is injected into claygl as is.
 */
function compileSizeExpr(src) {
  var toks = src.match(/\d+(?:\.\d+)?|[A-Za-z_]\w*|[-+*/()[\],]/g) || [];
  if (toks.join('') !== src.replace(/\s+/g, '')) throw new Error('Invalid expression.');
  var i = 0;
  function fail() {
    throw new Error('Invalid expression.');
  }
  function primary() {
    var t = toks[i++];
    if (t === '(') {
      var e = sum();
      if (toks[i++] !== ')') fail();
      return e;
    }
    if (t === '[') {
      var items = [sum()];
      while (toks[i] === ',') {
        i++;
        items.push(sum());
      }
      if (toks[i++] !== ']') fail();
      return function (w, h, d) {
        return items.map(function (f) {
          return f(w, h, d);
        });
      };
    }
    if (t === '-') {
      var neg = primary();
      return function (w, h, d) {
        return -neg(w, h, d);
      };
    }
    if (t !== undefined && /^\d/.test(t)) {
      var n = Number(t);
      return function () {
        return n;
      };
    }
    if (t === 'width') return function (w) { return w; };
    if (t === 'height') return function (w, h) { return h; };
    if (t === 'dpr') return function (w, h, d) { return d; };
    return fail();
  }
  // One closure per operator, built in its own call: a `var` captured inside the loop
  // would be shared by every iteration and make the tree call itself.
  function bin(op, a, b) {
    if (op === '*') return function (w, h, d) { return a(w, h, d) * b(w, h, d); };
    if (op === '/') return function (w, h, d) { return a(w, h, d) / b(w, h, d); };
    if (op === '+') return function (w, h, d) { return a(w, h, d) + b(w, h, d); };
    return function (w, h, d) { return a(w, h, d) - b(w, h, d); };
  }
  function product() {
    var l = primary();
    while (toks[i] === '*' || toks[i] === '/') {
      var op = toks[i++];
      l = bin(op, l, primary());
    }
    return l;
  }
  function sum() {
    var l = product();
    while (toks[i] === '+' || toks[i] === '-') {
      var op = toks[i++];
      l = bin(op, l, product());
    }
    return l;
  }
  var f = sum();
  if (i !== toks.length) fail();
  return f;
}

const TARGET = /[\\/]claygl[\\/]src[\\/]createCompositor\.js$/;
const EVAL = "new Function('width', 'height', 'dpr', 'return ' + exprRes[1])";

export { compileSizeExpr };

export default function claygl_no_eval() {
  return {
    name: 'claygl-no-eval',
    enforce: 'pre',
    transform(code, id) {
      if (!TARGET.test(id.split('?')[0])) return null;
      // Fail the build loudly if an upgrade moved the eval: a silent miss would ship a
      // blank 3D chart again.
      if (!code.includes(EVAL)) {
        this.error('claygl-no-eval: the expression eval in createCompositor.js has changed');
      }
      return {
        code: code.replace(EVAL, 'compileSizeExpr(exprRes[1])') + '\n' + compileSizeExpr.toString() + '\n',
        map: null
      };
    }
  };
}
