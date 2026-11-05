import { test } from 'node:test';
import assert from 'node:assert/strict';
import { barColumns, syminfoFor, inputValues, planRun, sliceOutput, mergeTail, withWarm, isForming } from '../src/lib/script/host.js';
import { runScript, __setWorkerFactory } from '../src/lib/script/client.js';

const view = {
  ts: ['2024-01-01T00:00:00Z', '2024-01-01T01:00:00Z'],
  o: [1, 2],
  h: [2, 3],
  l: [0.5, 1.5],
  c: [1.5, 2.5],
  v: [10, 20]
};

test('barColumns turns RFC3339 stamps into epoch ms and keeps prices', () => {
  const cols = barColumns(view, { volume: true });
  assert.deepEqual([...cols.time], [1704067200000, 1704070800000]);
  assert.deepEqual([...cols.close], [1.5, 2.5]);
  assert.deepEqual([...cols.volume], [10, 20]);
  assert.ok(cols.time instanceof Float64Array);
});

test('barColumns without volume sends an empty column (unknown, never zeros)', () => {
  assert.equal(barColumns(view, { volume: false }).volume.length, 0);
});

test('syminfoFor fills crypto by rule and leaves the rest unknown', () => {
  const crypto = syminfoFor({ provider: 'binance', asset_type: 'crypto', ticker: 'BTCUSDT' }, null);
  assert.equal(crypto.ticker, 'BTCUSDT');
  assert.equal(crypto.tickerid, 'BINANCE:BTCUSDT');
  assert.equal(crypto.timezone, 'Etc/UTC');
  assert.equal(crypto.session, '24x7');
  assert.equal(crypto.kind, 'crypto');
  assert.equal(crypto.mintick, null);
  const stored = syminfoFor({ provider: 'binance', asset_type: 'crypto', ticker: 'BTCUSDT' }, { timezone: 'Asia/Tokyo', mintick: 0.1 });
  assert.equal(stored.timezone, 'Asia/Tokyo');
  assert.equal(stored.mintick, 0.1);
  const equity = syminfoFor({ provider: 'alpaca', asset_type: 'equity', ticker: 'AAPL' }, null);
  assert.equal(equity.timezone, null);
  assert.equal(equity.session, null);
});

const inputs = [
  { key: 'len', kind: 'Int', default: { Int: 14 }, minval: 1, maxval: null, step: null, options: [] },
  { key: 'mult', kind: 'Float', default: { Float: 2 }, minval: null, maxval: null, step: null, options: [] },
  { key: 'src', kind: 'Source', default: { Source: 'close' }, minval: null, maxval: null, step: null, options: [] },
  { key: 'mode', kind: 'Str', default: { Str: 'a' }, minval: null, maxval: null, step: null, options: ['a', 'b'] },
  { key: 'col', kind: 'Color', default: { Color: { r: 0, g: 0, b: 0, a: 255 } }, minval: null, maxval: null, step: null, options: [] },
  { key: 'on', kind: 'Bool', default: { Bool: true }, minval: null, maxval: null, step: null, options: [] }
];

test('inputValues converts raw form values to the engine encoding', () => {
  const r = inputValues(inputs, { mult: '2.5', src: 'hl2', col: '#ff000080', on: false });
  assert.deepEqual(r.errors, []);
  assert.deepEqual(r.values, { mult: { Float: 2.5 }, src: { Source: 'hl2' }, col: { Color: { r: 255, g: 0, b: 0, a: 128 } }, on: { Bool: false } });
});

test('inputValues refuses out-of-range, wrong type and unknown options, never clamps', () => {
  const r = inputValues(inputs, { len: 0, mode: 'c', mult: 'x' });
  assert.deepEqual(
    r.errors.map((e) => [e.key, e.reason]).sort(),
    [['len', 'range'], ['mode', 'option'], ['mult', 'type']]
  );
  assert.deepEqual(r.values, {});
});

test('runScript keeps one run in flight and one pending per key: the replaced one is stale', async () => {
  const posted = [];
  let onmessage = null;
  __setWorkerFactory(() => ({
    postMessage: (m) => posted.push(m),
    set onmessage(f) {
      onmessage = f;
    },
    terminate() {}
  }));
  const answer = (i, result) => onmessage({ data: { id: posted[i].id, result } });
  const req = (n) => ({ source: `s${n}`, cols: {}, ctx: '{}' });
  const p1 = runScript('k', req(1));
  const p2 = runScript('k', req(2));
  const p3 = runScript('k', req(3));
  assert.equal(posted.length, 1);
  answer(0, { ok: true, n: 1 });
  assert.deepEqual(await p1, { ok: true, n: 1 });
  assert.deepEqual(await p2, { stale: true });
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(posted.length, 2);
  assert.equal(posted[1].source, 's3');
  answer(1, { ok: true, n: 3 });
  assert.deepEqual(await p3, { ok: true, n: 3 });
  assert.equal(posted.length, 2);
});

import { groupWarnings, diagText } from '../src/lib/script/diag.js';

const w = (code, args, line) => ({ code, args, severity: 'Warning', span: { line, col: 1, len: 1 } });

test('groupWarnings groups by feature: count and lines (spec: one grouped report)', () => {
  const g = groupWarnings([
    w('SkippedVisual', ['label.new', '2'], 3),
    w('SkippedVisual', ['table.new', '1'], 6),
    w('SkippedUnused', ['f'], 9),
    w('SkippedUnused', ['f'], 12)
  ]);
  assert.deepEqual(
    g.map((x) => [x.code, x.feature, x.count, x.lines]),
    [
      ['SkippedVisual', 'label.new', 2, [3]],
      ['SkippedVisual', 'table.new', 1, [6]],
      ['SkippedUnused', 'f', 2, [9, 12]]
    ]
  );
});

test('diagText translates by code with positional args and appends the line', () => {
  const t = (key, p) => `${key}|${p?.a0 ?? ''}|${p?.a1 ?? ''}|${p?.line ?? ''}`;
  assert.equal(diagText(t, w('UnknownName', ['foo'], 4)), 'script.diag.UnknownName|foo||script.diag.atLine|||4');
  assert.equal(diagText(t, { code: 'LoopLimit', args: [], span: null }), 'script.diag.LoopLimit|||');
});

test('diagText: an unsupported feature with phase 0 reads "not planned"', () => {
  const t = (key, p) => `${key}|${p?.a0 ?? ''}`;
  assert.equal(diagText(t, { code: 'Unsupported', args: ['request.security', '0'], span: null }), 'script.diag.UnsupportedNever|request.security');
  assert.equal(diagText(t, { code: 'Unsupported', args: ['strategy.entry', '3'], span: null }), 'script.diag.Unsupported|strategy.entry');
});

test('every engine diagnostic code has text in every language pack', async () => {
  const { readFileSync } = await import('node:fs');
  const rs = readFileSync(new URL('../../core/otw-script/src/diag.rs', import.meta.url), 'utf8');
  const variants = (name) =>
    [...rs.match(new RegExp(`pub enum ${name} \\{([^}]*)\\}`))[1].matchAll(/^\s*([A-Z]\w*),/gm)].map((m) => m[1]);
  const codes = [...variants('DiagCode'), ...variants('RtCode'), 'BadContext', 'UnsupportedNever', 'atLine'];
  assert.ok(codes.length > 30, `${codes.length} codes`);
  for (const lang of ['en', 'fr', 'de', 'es', 'it', 'pt', 'zh']) {
    const pack = JSON.parse(readFileSync(new URL(`../src/lib/i18n/${lang}.json`, import.meta.url), 'utf8'));
    const missing = codes.filter((c) => !pack[`script.diag.${c}`]);
    assert.deepEqual(missing, [], `${lang} missing`);
  }
});

test('defaultRaw: an input default as the raw form value inputValues accepts', async () => {
  const { defaultRaw } = await import('../src/lib/script/host.js');
  const m = (kind, d, extra = {}) => ({ key: 'k', kind, default: d, ...extra });
  assert.equal(defaultRaw(m('Int', { Int: 14 })), 14);
  assert.equal(defaultRaw(m('Float', { Float: 2.5 })), 2.5);
  assert.equal(defaultRaw(m('Bool', { Bool: true })), true);
  assert.equal(defaultRaw(m('Source', { Source: 'hl2' })), 'hl2');
  assert.equal(defaultRaw(m('Session', { Str: '0930-1600' })), '0930-1600');
  assert.equal(defaultRaw(m('Color', { Color: { r: 255, g: 0, b: 16, a: 128 } })), '#ff001080');
  // Round trip: the default fed back is accepted unchanged.
  const metas = [m('Int', { Int: 14 }), m('Color', { Color: { r: 1, g: 2, b: 3, a: 255 } })].map((x, i) => ({ ...x, key: `k${i}` }));
  const raw = Object.fromEntries(metas.map((x) => [x.key, defaultRaw(x)]));
  const { values, errors } = inputValues(metas, raw);
  assert.deepEqual(errors, []);
  assert.deepEqual(values, { k0: { Int: 14 }, k1: { Color: { r: 1, g: 2, b: 3, a: 255 } } });
});

test('instrumentErrors mirrors the server rules (positive numbers, IANA zone, session grammar)', async () => {
  const { instrumentErrors } = await import('../src/lib/script/host.js');
  assert.deepEqual(instrumentErrors({ mintick: 0.01, timezone: 'America/New_York', session: '0930-1600:23456' }), {});
  assert.deepEqual(instrumentErrors({ session: '24x7' }), {});
  assert.deepEqual(instrumentErrors({ session: '0930-1600,1700-1800' }), {});
  assert.deepEqual(instrumentErrors({ mintick: 0, pointvalue: -1 }), { mintick: 'badPositive', pointvalue: 'badPositive' });
  assert.deepEqual(instrumentErrors({ timezone: 'Mars/Olympus' }), { timezone: 'badTz' });
  for (const s of ['9:30-16:00', '0930-2400', '0930-1600:8', '0930-1600:', '']) {
    assert.deepEqual(instrumentErrors({ session: s }), { session: 'badSession' }, s);
  }
});

test('inputValues refuses an emptied number field instead of reading 0', () => {
  const metas = [{ key: 'n', kind: 'Int', default: { Int: 14 } }];
  assert.deepEqual(inputValues(metas, { n: '' }).errors, [{ key: 'n', reason: 'type' }]);
});

test('linesText: singular for one line, plural for several', async () => {
  const { linesText } = await import('../src/lib/script/diag.js');
  const t = (key, p) => `${key}|${p.lines}`;
  assert.equal(linesText(t, [3]), 'histviz.script.line|3');
  assert.equal(linesText(t, [3, 9]), 'histviz.script.lines|3, 9');
});

test('inputValues matches the shared fixture', async () => {
  const { readFileSync } = await import('node:fs');
  const cases = JSON.parse(readFileSync(new URL('../../core/otw-script/tests/fixtures/input_values.json', import.meta.url), 'utf8'));
  assert.ok(cases.length >= 15);
  for (const c of cases) {
    const { values, errors } = inputValues([c.meta], { [c.meta.key]: c.raw });
    if (c.expect.reason) assert.deepEqual(errors, [{ key: c.meta.key, reason: c.expect.reason }], JSON.stringify(c));
    else assert.deepEqual(values[c.meta.key], c.expect, JSON.stringify(c));
  }
});

test('warmup constant matches the engine', async () => {
  const { readFileSync } = await import('node:fs');
  const { SCRIPT_WARMUP_BARS } = await import('../src/lib/script/host.js');
  const rs = readFileSync(new URL('../../core/otw-script/src/host.rs', import.meta.url), 'utf8');
  const m = /FIRST_BAR_WINDOW: usize = ([\d_]+);/.exec(rs);
  assert.ok(m, 'FIRST_BAR_WINDOW in host.rs');
  assert.equal(SCRIPT_WARMUP_BARS, Number(m[1].replaceAll('_', '')));
});

test('session lane coalesces advances and names the op and key', async () => {
  const { openSession, advanceSession, closeSession } = await import('../src/lib/script/client.js');
  const posted = [];
  let onmessage = null;
  __setWorkerFactory(() => ({
    postMessage: (m) => posted.push(m),
    set onmessage(f) {
      onmessage = f;
    },
    terminate() {}
  }));
  const answer = (i, result) => onmessage({ data: { id: posted[i].id, result } });
  const p0 = openSession('inst:1', { source: 's', cols: {}, ctx: '{}', forming: true });
  assert.deepEqual([posted[0].op, posted[0].key, posted[0].forming], ['open', 'inst:1', true]);
  const p1 = advanceSession('inst:1', { cols: {}, forming: true, from: 9 });
  const p2 = advanceSession('inst:1', { cols: {}, forming: true, from: 9 });
  assert.equal(posted.length, 1);
  assert.deepEqual(await p1, { stale: true });
  answer(0, { ok: true, from: 0, committed: 9 });
  assert.equal((await p0).committed, 9);
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual([posted[1].op, posted[1].from], ['advance', 9]);
  answer(1, { ok: true, from: 9, committed: 9 });
  assert.equal((await p2).from, 9);
  closeSession('inst:1');
  assert.deepEqual([posted[2].op, posted[2].key], ['close', 'inst:1']);
});

// ── Phase 3: first-bar warmup and live rollback ─────────────────────────────────────────────

const hours = (n, from = 0) => Array.from({ length: n }, (_, i) => new Date(Date.UTC(2024, 0, 1, from + i)).toISOString());
function barsN(n, from = 0) {
  const c = Array.from({ length: n }, (_, i) => 100 + i);
  return { ts: hours(n, from), o: [...c], h: c.map((x) => x + 1), l: c.map((x) => x - 1), c: [...c], v: c.map(() => 10) };
}
const prevOf = (bars, committed, sig = 's') => ({
  sig,
  ts: bars.ts,
  committed,
  lastBar: { o: bars.o[committed - 1], h: bars.h[committed - 1], l: bars.l[committed - 1], c: bars.c[committed - 1], v: bars.v[committed - 1] }
});
const nextOf = (bars, sig = 's') => ({ sig, ts: bars.ts, bars });
const copy = (b) => Object.fromEntries(Object.entries(b).map(([k, v]) => [k, [...v]]));

test('planRun: only the forming bar moved, advance from committed', () => {
  const a = barsN(10);
  const b = copy(a);
  b.c[9] += 0.5;
  assert.deepEqual(planRun(prevOf(a, 9), nextOf(b)), { op: 'advance', from: 9 });
});

test('planRun: one appended bar is an advance', () => {
  const a = barsN(10);
  assert.deepEqual(planRun(prevOf(a, 9), nextOf(barsN(11))), { op: 'advance', from: 9 });
});

test('planRun: a bar inserted before committed reopens', () => {
  const a = barsN(10);
  const b = copy(a);
  for (const k of Object.keys(b)) b[k].splice(3, 0, b[k][3]);
  b.ts[3] = new Date(Date.parse(a.ts[3]) - 1000).toISOString();
  assert.deepEqual(planRun(prevOf(a, 9), nextOf(b)), { op: 'open' });
});

test('planRun: a changed committed bar reopens', () => {
  const a = barsN(10);
  const b = copy(a);
  b.c[8] += 1;
  assert.deepEqual(planRun(prevOf(a, 9), nextOf(b)), { op: 'open' });
});

test('planRun: another signature reopens', () => {
  const a = barsN(10);
  assert.deepEqual(planRun(prevOf(a, 9, 's'), nextOf(a, 't')), { op: 'open' });
});

test('planRun: 51 new bars reopen', () => {
  const a = barsN(10);
  assert.deepEqual(planRun(prevOf(a, 10), nextOf(barsN(61))), { op: 'open' });
  assert.deepEqual(planRun(prevOf(a, 10), nextOf(barsN(60))), { op: 'advance', from: 10 });
  assert.deepEqual(planRun(null, nextOf(a)), { op: 'open' });
});

function outputN(n, shift = 0) {
  const col = (k) => Array.from({ length: n }, (_, i) => i * k + shift);
  const rgba = (i) => ({ r: i, g: 0, b: 0, a: 255 });
  return {
    plots: [
      { index: 0, values: col(1), colors: col(1).map(rgba), linewidth: 1, offset: 0, histbase: 0, hidden: false },
      { index: 1, values: col(2), colors: col(2).map(rgba), linewidth: 2, offset: 0, histbase: 0, hidden: false }
    ],
    hlines: [{ price: 1, title: 'h', color: null, style: 'solid', linewidth: 1 }],
    fills: [{ a: { Plot: 0 }, b: { Plot: 1 }, colors: col(1).map(rgba) }],
    bgcolor: [{ colors: col(3).map(rgba) }],
    barcolor: [],
    shapes: [{ kind: 'plotshape', when: col(1), style: 'circle', location: 'abovebar', color: col(1).map(rgba), text: '' }],
    candles: [{ kind: 'plotcandle', o: col(1), h: col(1), l: col(1), c: col(1), colors: col(1).map(rgba) }]
  };
}

test('sliceOutput and mergeTail round-trip every per-bar column', () => {
  const full = outputN(10);
  const tail = sliceOutput(full, 8);
  assert.equal(tail.plots[1].values.length, 2);
  assert.equal(tail.candles[0].c.length, 2);
  assert.deepEqual(tail.hlines, full.hlines);
  const stale = outputN(9, 1000);
  assert.deepEqual(mergeTail({ ...outputN(10), plots: outputN(10).plots }, 8, tail), full);
  // A 9-bar output whose last bar moved, merged with the session's answer from bar 8.
  const merged = mergeTail(stale, 8, tail);
  assert.deepEqual(merged.plots[0].values.slice(8), [8, 9]);
  assert.equal(merged.plots[0].values.length, 10);
  assert.equal(merged.shapes[0].when[7], 1007);
});

test('withWarm keeps only warm bars strictly older than the first drawn bar', () => {
  const warm = barsN(5); // 00:00 .. 04:00
  const bars = barsN(3, 4); // 04:00 .. 06:00
  const all = withWarm(warm, bars);
  assert.deepEqual(all.ts, [...hours(4), ...bars.ts]);
  assert.equal(all.c.length, 7);
  assert.deepEqual(withWarm(null, bars).ts, bars.ts);
});

test('isForming: a bar whose close time is now is closed', () => {
  const t = '2024-01-01T00:00:00.000Z';
  const close = Date.parse(t) + 3600 * 1000;
  assert.equal(isForming(t, 3600, close - 1), true);
  assert.equal(isForming(t, 3600, close), false);
});
