import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spanToRange } from '../src/lib/script/spans.js';
import { tokenize } from '../src/lib/script/pine-lang.js';

test('spanToRange counts the engine columns in code points, CodeMirror offsets in UTF-16', () => {
  const text = 'a😀b\nxy';
  assert.deepEqual(spanToRange(text, { line: 1, col: 3, len: 1 }), { from: 3, to: 4 });
  assert.deepEqual(spanToRange(text, { line: 2, col: 2, len: 1 }), { from: 6, to: 7 });
});

test('spanToRange clamps a span past the end instead of throwing', () => {
  const r = spanToRange('ab', { line: 9, col: 1, len: 3 });
  assert.ok(r.from <= 2 && r.to <= 2 && r.from <= r.to);
  assert.deepEqual(spanToRange('ab', { line: 1, col: 2, len: 10 }), { from: 1, to: 2 });
});

/** Is `want` a subsequence of `got`? */
const inOrder = (got, want) => {
  let k = 0;
  for (const g of got) if (g === want[k]) k++;
  return k === want.length;
};

test('tokenize: names, operators, namespaces, numbers, comments', () => {
  const types = tokenize('x = ta.sma(close, 14) // c').map((t) => t.type);
  assert.ok(inOrder(types, ['name', 'operator', 'namespace', 'number', 'comment']), types.join(' '));
});

test('tokenize: an escaped quote stays inside the string', () => {
  const toks = tokenize("s = 'a\\'b'");
  assert.deepEqual(toks.filter((t) => t.type === 'string').map((t) => t.text), ["'a\\'b'"]);
});

test('tokenize: color literals and keywords', () => {
  assert.ok(tokenize('c = #FF000080').some((t) => t.type === 'color' && t.text === '#FF000080'));
  assert.ok(tokenize('if x and not na(y)').filter((t) => t.type === 'keyword').length >= 3);
});
