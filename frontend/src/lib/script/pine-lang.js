/** Pine Script v5/v6 highlighting: a small tokenizer, used both by CodeMirror (through
 *  `StreamLanguage`) and by tests. Types: comment string number color keyword namespace
 *  name operator. */
import { StreamLanguage } from '@codemirror/language';

export const KEYWORDS = new Set(
  'if else for to by while switch var varip import export method type true false na and or not continue break in'.split(' ')
);

/** Builtin namespaces (`ta.sma`, `color.new`, …). */
export const NAMESPACES = new Set(
  ('ta math color input str array matrix map request ticker syminfo timeframe barstate plot hline shape location size ' +
    'display line label box table chart session runtime log alert format position text extend font dayofweek currency ' +
    'xloc yloc barmerge polyline linefill order strategy adjustment backadjustment dividends earnings splits')
    .split(' ')
);

const OPERATORS = /^(:=|==|!=|<=|>=|=>|\+=|-=|\*=|\/=|%=|[-+*/%<>=?:.,()[\]])/;

/** One token starting at `pos` (whitespace already skipped): `{type, end}`. */
export function readToken(s, pos) {
  const rest = s.slice(pos);
  if (rest.startsWith('//')) return { type: 'comment', end: s.length };
  const q = rest[0];
  if (q === '"' || q === "'") {
    let k = 1;
    while (k < rest.length && rest[k] !== q) k += rest[k] === '\\' ? 2 : 1;
    return { type: 'string', end: pos + Math.min(k + 1, rest.length) };
  }
  let m = /^#[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?\b/.exec(rest);
  if (m) return { type: 'color', end: pos + m[0].length };
  m = /^(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/.exec(rest);
  if (m) return { type: 'number', end: pos + m[0].length };
  m = /^[A-Za-z_][A-Za-z0-9_]*/.exec(rest);
  if (m) {
    const w = m[0];
    const type = KEYWORDS.has(w) ? 'keyword' : NAMESPACES.has(w) && rest[w.length] === '.' ? 'namespace' : 'name';
    return { type, end: pos + w.length };
  }
  m = OPERATORS.exec(rest);
  if (m) return { type: 'operator', end: pos + m[0].length };
  return { type: null, end: pos + 1 };
}

/** Every token of one line. */
export function tokenize(line) {
  const out = [];
  let pos = 0;
  while (pos < line.length) {
    if (/\s/.test(line[pos])) {
      pos++;
      continue;
    }
    const t = readToken(line, pos);
    if (t.type) out.push({ type: t.type, text: line.slice(pos, t.end) });
    pos = t.end;
  }
  return out;
}

/** Token type → highlight tag name (`@lezer/highlight` tags, via StreamLanguage). */
const TAGS = {
  comment: 'comment',
  string: 'string',
  number: 'number',
  color: 'atom',
  keyword: 'keyword',
  namespace: 'namespace',
  name: 'variableName',
  operator: 'operator'
};

export const pineLanguage = StreamLanguage.define({
  name: 'pine',
  token(stream) {
    if (stream.eatSpace()) return null;
    const t = readToken(stream.string, stream.pos);
    stream.pos = Math.max(t.end, stream.pos + 1);
    return t.type ? TAGS[t.type] : null;
  },
  languageData: { commentTokens: { line: '//' } }
});
