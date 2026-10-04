// Render the actual Svelte primitives with fixtures; no provider or running backend needed.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import { fileURLToPath } from 'node:url';

const folder = new URL('../node_modules/.cache/widget-render-tests/', import.meta.url);
const strings = JSON.parse(await readFile(new URL('../src/lib/i18n/en.json', import.meta.url), 'utf8'));
async function component(name) {
  const path = new URL(`../src/lib/modules/dashboard/widgets/parts/${name}.svelte`, import.meta.url);
  let source = await readFile(path, 'utf8');
  source = source.replace("import { t } from '$lib/i18n';", `const strings = ${JSON.stringify(strings)}; const t = { subscribe(fn) { fn((key, params = {}) => Object.entries(params).reduce((text, [key,value]) => text.replaceAll('{'+key+'}', String(value)), strings[key] ?? key)); return () => {}; } };`);
  const helper = new URL('../src/lib/modules/dashboard/widgets/insights.js', import.meta.url).href;
  source = source.replace("from '../insights.js'", `from '${helper}'`);
  const result = compile(source, { filename: fileURLToPath(path), generate: 'server' });
  await mkdir(folder, { recursive: true });
  const output = new URL(`${name}.mjs`, folder);
  await writeFile(output, result.js.code);
  return (await import(output.href)).default;
}

test('statement bars place losses below zero, show missing periods, and expose values to keyboard users', async () => {
  const Bars = await component('StatementBars');
  const rows = [20, -30, null, 0].map((value, i) => ({ period: `Q${i + 1} FY2026`, lines: { revenue: value } }));
  const { body } = render(Bars, { props: { rows, line: 'revenue', format: (v) => v == null ? '—' : String(v), label: 'Revenue' } });
  const negative = body.match(/<span[^>]*class="bar [^"]*negative[^>]*>/)?.[0].replaceAll(' ', '');
  assert.ok(negative?.includes('top:40%'), negative);
  assert.ok(negative?.includes('height:60%'), negative);
  assert.match(body, /aria-label="Q3 FY2026: —"/);
  assert.match(body, /aria-label="Q4 FY2026: 0"/);
  assert.equal((body.match(/<button/g) ?? []).length, 4);
  assert.match(body, /aria-live="polite"/);
});

test('matrix renders nested correlation values, accessible pair labels, and sample counts', async () => {
  const Matrix = await component('Matrix');
  const { body } = render(Matrix, { props: {
    rows: ['A', 'B'], columns: ['A', 'B'], values: [[1, -.35], [-.35, 1]], counts: [[100, 100], [100, 100]],
    format: (v) => v.toFixed(2), mode: 'correlation', label: 'Correlation'
  } });
  assert.match(body, /aria-label="A · B: -0.35 · 100 samples"/);
  assert.match(body, /scope="row"/);
  assert.match(body, /scope="col"/);
  assert.equal((body.match(/<button/g) ?? []).length, 4);
  assert.match(body, /−1/);
  assert.match(body, /\+1/);
});

test('matrix visibly distinguishes observed zero from missing observations', async () => {
  const Matrix = await component('Matrix');
  const { body } = render(Matrix, { props: { rows: ['Jan'], columns: ['Mon', 'Tue'], values: [[0, null]], counts: [[2, 0]], format: String } });
  assert.match(body, /Jan · Mon: 0 · 2 samples/);
  assert.match(body, /Jan · Tue: no data · 0 samples/);
  assert.equal((body.match(/class="cell [^"]*missing/g) ?? []).length, 1);
});
