import { test, mock } from 'node:test';
import assert from 'node:assert/strict';

// prefs.svelte.js uses `$state.raw` only in its reactive handle; a plain shim is enough here.
globalThis.$state = Object.assign((v) => v, { raw: (v) => v });

const data = new Map();
globalThis.localStorage = {
  getItem: (k) => (data.has(k) ? data.get(k) : null),
  setItem: (k, v) => data.set(k, String(v)),
  removeItem: (k) => data.delete(k)
};
let calls = [];
let server = {};
globalThis.fetch = async (url, init = {}) => {
  const call = { url, method: init.method ?? 'GET', body: init.body ? JSON.parse(init.body) : null };
  calls.push(call);
  return { ok: true, status: 200, json: async () => (call.method === 'GET' ? { prefs: server } : {}) };
};

const { entryKey, indexKey, nextIndex, legacyAdoptions, mergeIndex, boardStore, MAX_INSTRUMENTS } = await import(
  '../src/lib/modules/histviz/boards.js'
);
const { prefs } = await import('../src/lib/prefs.svelte.js');

const KEY_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;

test('an entry key is the instrument made key-safe, readable when it fits', () => {
  const k = entryKey('otw.histviz.drawings.v1', 'binance|crypto|BTCUSDT');
  assert.equal(k, 'otw.histviz.drawings.v1.binance_7ccrypto_7cBTCUSDT');
  assert.match(k, KEY_RE);
  assert.equal(entryKey('b', 'ibkr|stock|BRK B'), 'b.ibkr_7cstock_7cBRK_20B');
  assert.equal(entryKey('b', 'p|x|€'), 'b.p_7cx_7c_u20ac');
  // Escapes keep distinct instruments apart: `_` itself is escaped.
  assert.notEqual(entryKey('b', 'p|x|a_7c'), entryKey('b', 'p|x|a|'));
});

test('an id too long for the server key cap is a stable 64-bit hash', () => {
  const long = `yahoo|equity|${'A'.repeat(120)}`;
  const k = entryKey('otw.histviz.quick.v1', long);
  assert.match(k, /^otw\.histviz\.quick\.v1\.x[0-9a-f]{16}$/);
  assert.match(k, KEY_RE);
  assert.equal(k, entryKey('otw.histviz.quick.v1', long));
  assert.notEqual(k, entryKey('otw.histviz.quick.v1', `${long}B`));
  assert.equal(indexKey('otw.histviz.quick.v1'), 'otw.histviz.quick.v1.index');
});

test('the index keeps an instrument in place, appends a new one and drops the oldest past the cap', () => {
  assert.deepEqual(nextIndex(['a', 'b'], 'a', true), { index: ['a', 'b'], dropped: [] });
  assert.deepEqual(nextIndex(['a', 'b'], 'c', true), { index: ['a', 'b', 'c'], dropped: [] });
  assert.deepEqual(nextIndex(['a', 'b'], 'a', false), { index: ['b'], dropped: [] });
  assert.deepEqual(nextIndex(['a', 'b', 'c'], 'd', true, 3), { index: ['b', 'c', 'd'], dropped: ['a'] });
});

test('the one-blob split adopts only non-empty boards the server lacks, legacy ones oldest', () => {
  const blob = { 'p|e|A': [{ id: 1 }], 'p|e|B': [], 'p|e|C': [{ id: 3 }], 'p|e|D': 'junk' };
  const adopted = legacyAdoptions(blob, (k) => k === 'p|e|C');
  assert.deepEqual(adopted, [['p|e|A', [{ id: 1 }]]]);
  assert.deepEqual(legacyAdoptions(['x'], () => false), []);
  assert.deepEqual(legacyAdoptions(null, () => false), []);
  assert.deepEqual(mergeIndex(['p|e|C'], ['p|e|A', 'p|e|C']), { index: ['p|e|A', 'p|e|C'], dropped: [] });
  assert.deepEqual(mergeIndex(['x', 'y'], ['a', 'b'], 3), { index: ['b', 'x', 'y'], dropped: ['a'] });
});

test('an old browser reads its blob before load, splits it once after, and a server board wins', async () => {
  const BASE = 'otw.histviz.drawings.v1';
  const A = 'binance|crypto|BTCUSDT';
  const B = 'ibkr|future|ESZ6';
  const C = 'yahoo|equity|AAPL';
  mock.timers.enable({ apis: ['setTimeout'] });
  data.set(BASE, JSON.stringify({ [A]: [{ id: 'a1' }], [B]: [{ id: 'b-old' }], [C]: [{ id: 'c1' }] }));
  // Another browser already saved B's board and the index.
  server = { [entryKey(BASE, B)]: [{ id: 'b-server' }], [indexKey(BASE)]: [B] };
  const store = boardStore(BASE);

  // Before load: the blob is the first-paint copy.
  assert.deepEqual(store.load(A), [{ id: 'a1' }]);
  assert.deepEqual(store.load(B), [{ id: 'b-old' }]);
  // A board cleared before load is not brought back by the split.
  store.save(C, []);
  assert.equal(calls.filter((c) => c.method !== 'GET').length, 0, 'nothing is sent before load');

  // `load()` was asked for by the store itself; let it and the split run.
  await prefs.load();
  await new Promise((r) => setImmediate(r));
  mock.timers.tick(400);
  await new Promise((r) => setImmediate(r));

  assert.equal(data.has(BASE), false, 'the blob is gone once split');
  assert.deepEqual(store.load(A), [{ id: 'a1' }]);
  assert.deepEqual(store.load(B), [{ id: 'b-server' }], 'the server board wins over the blob');
  assert.deepEqual(store.load(C), []);
  const puts = calls.filter((c) => c.method === 'PUT').map((c) => decodeURIComponent(c.url.split('/').pop()));
  assert.ok(puts.includes(entryKey(BASE, A)), 'the missing board is uploaded');
  assert.ok(!puts.includes(entryKey(BASE, B)), 'the server board is not overwritten');
  assert.ok(!puts.includes(entryKey(BASE, C)), 'the cleared board is not uploaded');
  const index = calls.find((c) => c.url.endsWith(encodeURIComponent(indexKey(BASE))));
  assert.deepEqual(index.body, { value: [A, B] });
  // The entry's mirror is its own key, in JSON.
  assert.equal(data.get(entryKey(BASE, A)), JSON.stringify([{ id: 'a1' }]));

  // Not again: a later read or write finds no blob and uploads nothing more of it.
  const before = calls.length;
  store.load(A);
  boardStore(BASE).load(A);
  mock.timers.tick(400);
  await new Promise((r) => setImmediate(r));
  assert.equal(calls.length, before);

  // A new board past the cap drops the oldest instrument's entry.
  for (let i = 0; i < MAX_INSTRUMENTS; i++) store.save(`p|e|T${i}`, [{ id: i }]);
  mock.timers.tick(400);
  await new Promise((r) => setImmediate(r));
  const deleted = calls.filter((c) => c.method === 'DELETE').map((c) => decodeURIComponent(c.url.split('/').pop()));
  assert.ok(deleted.includes(entryKey(BASE, A)) && deleted.includes(entryKey(BASE, B)));
  assert.equal(JSON.parse(data.get(indexKey(BASE))).length, MAX_INSTRUMENTS);
  mock.timers.reset();
});
