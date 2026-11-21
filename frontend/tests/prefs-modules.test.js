import { test, before } from 'node:test';
import assert from 'node:assert/strict';

// The journal, histdata, news, resources, prompt-store, time, community-docs and
// fundamentals preferences, run as plain JS over one app boot: the legacy localStorage
// values must decode with their codec and upload once, and the fundamentals store must
// adopt the server's copy without sharing objects with the preference store.
globalThis.$state = Object.assign((v) => v, { raw: (v) => v });
globalThis.$effect = Object.assign(() => {}, { root: (fn) => fn() });

const data = new Map();
globalThis.localStorage = {
  getItem: (k) => (data.has(k) ? data.get(k) : null),
  setItem: (k, v) => data.set(k, String(v)),
  removeItem: (k) => data.delete(k)
};
const calls = [];
let server = {};
globalThis.fetch = async (url, init = {}) => {
  const call = { url, method: init.method ?? 'GET', body: init.body ? JSON.parse(init.body) : null };
  calls.push(call);
  return { ok: true, status: 200, json: async () => (call.method === 'GET' ? { prefs: server } : {}) };
};
console.warn = () => {};

// What the old code wrote, byte for byte.
const LEGACY = {
  'otw.journal.prefs.v1': '{"view":"calendar","categoryId":""}',
  'otw.journal.trades.filters.v1': '{"fTicker":"ES","fSide":"long","fResult":"","fSince":"","fUntil":""}',
  'otw.journal.breakdown.filters.v1': '{"fSide":"","fTicker":"NQ","fStrategy":"","fSignal":"","fAsset":"","fTag":"","fSince":"","fUntil":""}',
  'otw.journal.calendar.month.v1': '{"year":2026,"month":3}',
  'otw.journal.compare.period.v1': 'quarter',
  'otw.journal.scatter.v1': '{"x":"date","y":"net","color":"result","trend":false}',
  'otw.journal.breakdown.detailsOpen.v1': '1',
  'otw.backtest.streaks3d.v1': '{"unit":"money","show":"wins","minLen":3,"fold":true}',
  'otw.histdata.prefs.v1': '{"tab":"datasets"}',
  'otw.histdata.jobFilters.v1': '{"status":"done","provider":"","timeframe":"","ticker":""}',
  'otw.news.prefs.v1': '{"viewMode":"line","fSourceNames":["Reuters"],"selectedId":"d1"}',
  'otw.time.prefs.v1': '{"view":"breakdown","selectedId":4}',
  'otw.communityDocs.prefs.v1': '{"display":"list"}'
};

let p;
let fund;
before(async () => {
  for (const [k, v] of Object.entries(LEGACY)) data.set(k, v);
  data.set('fundamentals.prefs.v2', JSON.stringify({ density: 'compact', etf: { ticker: 'QQQ' } }));
  p = await import('$lib/prefs.svelte.js');
  fund = await import('$lib/modules/fundamentals/prefs.svelte.js');
});

test('every key of this group is in the migration table with its old codec', () => {
  for (const k of Object.keys(LEGACY)) assert.ok(k in p.PREF_KEYS, k);
  assert.equal(p.PREF_KEYS['otw.journal.compare.period.v1'], 'string');
  assert.equal(p.PREF_KEYS['otw.journal.breakdown.detailsOpen.v1'], 'bool01');
  assert.equal(p.PREF_KEYS['fundamentals.prefs.v2'], 'json');
});

test('before load, the legacy values read back decoded', () => {
  assert.equal(p.getPref('otw.journal.compare.period.v1', 'month'), 'quarter');
  assert.equal(p.getPref('otw.journal.breakdown.detailsOpen.v1', false), true);
  assert.deepEqual(p.getPref('otw.journal.calendar.month.v1', null), { year: 2026, month: 3 });
  assert.equal(fund.prefs.density, 'compact');
  assert.equal(fund.prefs.etf.ticker, 'QQQ');
  assert.equal(fund.prefs.library.tab, 'series'); // defaults fill what the old value lacks
});

test('the fundamentals store holds a copy, not the preference store value', () => {
  fund.prefs.etf.ticker = 'XLF';
  assert.equal(p.getPref('fundamentals.prefs.v2', null).etf.ticker, 'QQQ');
  fund.prefs.etf.ticker = 'QQQ';
});

test('load adopts the server copy and uploads the legacy values once', async () => {
  server = { 'fundamentals.prefs.v2': { density: 'comfortable', etf: { ticker: 'SPY' } }, 'otw.histdata.prefs.v1': { tab: 'download' } };
  assert.equal(await p.prefs.load(), true);
  // The server's copy wins over the browser's, in the store and in the mirror.
  assert.equal(fund.prefs.density, 'comfortable');
  assert.equal(fund.prefs.etf.ticker, 'SPY');
  assert.equal(data.get('otw.histdata.prefs.v1'), '{"tab":"download"}');
  const bulk = calls.filter((c) => c.method === 'PUT' && c.url === '/api/ui-prefs');
  assert.equal(bulk.length, 1);
  const sent = bulk[0].body.prefs;
  assert.ok(!('otw.histdata.prefs.v1' in sent)); // the server already had it
  assert.ok(!('fundamentals.prefs.v2' in sent));
  assert.equal(sent['otw.journal.compare.period.v1'], 'quarter');
  assert.equal(sent['otw.journal.breakdown.detailsOpen.v1'], true);
  assert.deepEqual(sent['otw.time.prefs.v1'], { view: 'breakdown', selectedId: 4 });
  assert.deepEqual(sent['otw.news.prefs.v1'].fSourceNames, ['Reuters']);
  for (const k of Object.keys(LEGACY)) if (k !== 'otw.histdata.prefs.v1') assert.ok(k in sent, k);
  // Marked done: a later boot would not upload them again.
  const done = JSON.parse(data.get('otw.prefs.migrated.v1'));
  for (const k of Object.keys(LEGACY)) assert.ok(done.includes(k), k);
});

test('the mirror keeps the old format after a write', () => {
  p.setPref('otw.journal.compare.period.v1', 'week');
  assert.equal(data.get('otw.journal.compare.period.v1'), 'week');
  p.setPref('otw.journal.breakdown.detailsOpen.v1', false);
  assert.equal(data.get('otw.journal.breakdown.detailsOpen.v1'), '0');
  p.setPref('otw.journal.calendar.month.v1', { year: 2026, month: 10 });
  assert.equal(data.get('otw.journal.calendar.month.v1'), '{"year":2026,"month":10}');
});
