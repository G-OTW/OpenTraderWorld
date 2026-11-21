import { test, beforeEach, afterEach, mock } from 'node:test';
import assert from 'node:assert/strict';

// prefs.svelte.js creates its reactive cells with `$state.raw`, which the Svelte compiler
// rewrites. Run as plain JS here, a cell is an ordinary closure variable: same values,
// no reactivity (the browser check covers that).
globalThis.$state = Object.assign((v) => v, { raw: (v) => v });

let boot = 0;
/** A fresh module instance: one app boot over whatever localStorage holds. */
const fresh = () => import(`../src/lib/prefs.svelte.js?boot=${++boot}`);

let data;
let calls;
let server;
let respond;

beforeEach(() => {
  data = new Map();
  globalThis.localStorage = {
    getItem: (k) => (data.has(k) ? data.get(k) : null),
    setItem: (k, v) => data.set(k, String(v)),
    removeItem: (k) => data.delete(k)
  };
  calls = [];
  server = {};
  respond = null;
  globalThis.fetch = async (url, init = {}) => {
    const call = { url, method: init.method ?? 'GET', body: init.body ? JSON.parse(init.body) : null, keepalive: !!init.keepalive };
    calls.push(call);
    if (respond) return respond(call);
    return ok(call.method === 'GET' ? { prefs: server } : {});
  };
  mock.timers.enable({ apis: ['setTimeout'] });
  mock.method(console, 'warn', () => {});
});

afterEach(() => {
  mock.timers.reset();
  mock.restoreAll();
});

const ok = (body) => ({ ok: true, status: 200, json: async () => body });
const status = (code) => ({ ok: false, status: code, json: async () => ({ error: 'no' }) });
const settle = () => new Promise((r) => setImmediate(r));
const writes = () => calls.filter((c) => c.method !== 'GET');

test('before load, reads come from the localStorage copy in its old format', async () => {
  data.set('otw-theme', 'dark');
  data.set('otw.privacy.hidden', '1');
  data.set('otw.histviz.railW', '240');
  data.set('otw.rail.folded', '{"tools":true}');
  const p = await fresh();
  assert.equal(p.getPref('otw-theme', 'system'), 'dark');
  assert.equal(p.getPref('otw.privacy.hidden', false), true);
  assert.equal(p.getPref('otw.histviz.railW', 198), 240);
  assert.deepEqual(p.getPref('otw.rail.folded', {}), { tools: true });
  assert.equal(p.getPref('otw.alert.sound', 'off'), 'off');
  assert.equal(p.pref('otw-theme', 'system').value, 'dark');
  assert.equal(p.prefs.loaded, false);
  assert.equal(calls.length, 0);
});

test('a write updates memory and mirrors to localStorage in the key\'s own format', async () => {
  const p = await fresh();
  const theme = p.pref('otw-theme', 'system');
  theme.value = 'light';
  assert.equal(theme.value, 'light');
  assert.equal(data.get('otw-theme'), 'light'); // raw, as static/prepaint.js reads it
  p.setPref('otw.privacy.hidden', true);
  assert.equal(data.get('otw.privacy.hidden'), '1');
  p.setPref('otw.histviz.railW', 250);
  assert.equal(data.get('otw.histviz.railW'), '250');
  p.setPref('otw.rail.folded', { workspace: true });
  assert.equal(data.get('otw.rail.folded'), '{"workspace":true}');
  p.setPref('otw.some.new.v1', { a: [1] }); // not in the table: JSON
  assert.equal(data.get('otw.some.new.v1'), '{"a":[1]}');
  p.removePref('otw.some.new.v1');
  assert.equal(data.has('otw.some.new.v1'), false);
  assert.equal(p.getPref('otw.some.new.v1', 'gone'), 'gone');
  assert.throws(() => p.setPref('bad key', 1));
});

test('load adopts the server map, rewrites the mirror and tells listeners', async () => {
  data.set('otw-theme', 'dark');
  server = { 'otw-theme': 'light', 'otw.histviz.railW': 300 };
  const p = await fresh();
  const handle = p.pref('otw-theme', 'system');
  const seen = [];
  p.onPref('otw-theme', (v) => seen.push(v));
  assert.equal(await p.prefs.load(), true);
  assert.equal(handle.value, 'light');
  assert.deepEqual(seen, ['light']);
  assert.equal(data.get('otw-theme'), 'light');
  assert.equal(data.get('otw.histviz.railW'), '300');
  assert.equal(p.prefs.loaded, true);
  // A second call is the same request.
  await p.prefs.load();
  assert.equal(calls.filter((c) => c.method === 'GET').length, 1);
});

test('writes are debounced per key: a burst sends the last value once', async () => {
  const p = await fresh();
  await p.prefs.load();
  calls.length = 0;
  p.setPref('otw-theme', 'dark');
  p.setPref('otw-theme', 'light');
  mock.timers.tick(200);
  p.setPref('otw-theme', 'system');
  p.setPref('otw.privacy.hidden', true);
  mock.timers.tick(299);
  assert.equal(writes().length, 0);
  mock.timers.tick(1);
  await settle();
  assert.deepEqual(
    writes().map((c) => [c.method, c.url, c.body]),
    [
      ['PUT', '/api/ui-prefs/otw-theme', { value: 'system' }],
      ['PUT', '/api/ui-prefs/otw.privacy.hidden', { value: true }]
    ]
  );
  // An unchanged value is not sent again.
  p.setPref('otw-theme', 'system');
  mock.timers.tick(300);
  await settle();
  assert.equal(writes().length, 2);
  // A removal is a DELETE.
  p.removePref('otw-theme');
  mock.timers.tick(300);
  await settle();
  assert.deepEqual(writes().at(-1), { url: '/api/ui-prefs/otw-theme', method: 'DELETE', body: null, keepalive: false });
});

test('a write made before load is held, then sent once load resolves', async () => {
  const p = await fresh();
  p.setPref('otw-theme', 'dark');
  mock.timers.tick(1000);
  assert.equal(writes().length, 0);
  server = { 'otw-theme': 'light' };
  await p.prefs.load();
  assert.equal(p.getPref('otw-theme'), 'dark'); // the user's latest choice wins
  mock.timers.tick(300);
  await settle();
  assert.deepEqual(writes().map((c) => [c.url, c.body]), [['/api/ui-prefs/otw-theme', { value: 'dark' }]]);
});

test('the one-time upload sends only the keys the server lacks, and only once', async () => {
  data.set('otw-theme', 'dark');
  data.set('otw.privacy.hidden', '1');
  data.set('otw.histviz.railW', '250');
  data.set('otw.histviz.rail.v1', 'chart:7');
  data.set('otw.unlisted.v1', '"x"'); // not in PREF_KEYS: never uploaded
  server = { 'otw-theme': 'light' };
  let p = await fresh();
  await p.prefs.load();
  const bulk = writes();
  assert.equal(bulk.length, 1);
  assert.equal(bulk[0].url, '/api/ui-prefs');
  assert.equal(bulk[0].method, 'PUT');
  assert.deepEqual(bulk[0].body, {
    prefs: { 'otw.privacy.hidden': true, 'otw.histviz.railW': 250, 'otw.histviz.rail.v1': 'chart:7' }
  });
  assert.equal(p.getPref('otw-theme'), 'light'); // the server's value was kept
  assert.deepEqual(new Set(JSON.parse(data.get('otw.prefs.migrated.v1'))), new Set(Object.keys(p.PREF_KEYS)));

  // The next boot has nothing left to upload.
  calls.length = 0;
  p = await fresh();
  await p.prefs.load();
  assert.equal(writes().length, 0);

  // A key added to the table later is uploaded at the next load, alone.
  data.set('otw.prefs.migrated.v1', JSON.stringify(Object.keys(p.PREF_KEYS).filter((k) => k !== 'otw.alert.sound')));
  data.set('otw.alert.sound', 'bell');
  calls.length = 0;
  p = await fresh();
  await p.prefs.load();
  assert.deepEqual(writes().map((c) => c.body), [{ prefs: { 'otw.alert.sound': 'bell' } }]);
});

test('a failed upload is not marked done, so the next boot tries again', async () => {
  data.set('otw.privacy.hidden', '1');
  respond = (c) => (c.method === 'GET' ? ok({ prefs: {} }) : status(503));
  let p = await fresh();
  await p.prefs.load();
  assert.equal(data.has('otw.prefs.migrated.v1'), false);
  respond = null;
  calls.length = 0;
  p = await fresh();
  await p.prefs.load();
  assert.deepEqual(writes().map((c) => c.body), [{ prefs: { 'otw.privacy.hidden': true } }]);
});

test('a failed PUT is retried once, then logged, and the local value stays', async () => {
  const p = await fresh();
  await p.prefs.load();
  respond = (c) => {
    if (c.method === 'GET') return ok({ prefs: server });
    throw new TypeError('network down');
  };
  calls.length = 0;
  p.setPref('otw-theme', 'dark');
  mock.timers.tick(300);
  await settle();
  assert.equal(writes().length, 1);
  mock.timers.tick(1000);
  await settle();
  assert.equal(writes().length, 2); // the one retry
  mock.timers.tick(10_000);
  await settle();
  assert.equal(writes().length, 2);
  assert.equal(console.warn.mock.callCount() >= 1, true);
  assert.equal(p.getPref('otw-theme'), 'dark');
  assert.equal(data.get('otw-theme'), 'dark');

  // Next boot: the server still has the old value, the unacknowledged local one wins and is sent.
  respond = null;
  server = { 'otw-theme': 'light' };
  calls.length = 0;
  const next = await fresh();
  await next.prefs.load();
  assert.equal(next.getPref('otw-theme'), 'dark');
  mock.timers.tick(300);
  await settle();
  assert.deepEqual(writes().map((c) => [c.url, c.body]), [['/api/ui-prefs/otw-theme', { value: 'dark' }]]);
  assert.deepEqual(JSON.parse(data.get('otw.prefs.dirty.v1')), []);
});

test('a refused PUT (4xx) is not retried and the local value stays', async () => {
  const p = await fresh();
  await p.prefs.load();
  respond = (c) => (c.method === 'GET' ? ok({ prefs: {} }) : status(403));
  calls.length = 0;
  p.setPref('otw.privacy.hidden', true);
  mock.timers.tick(300);
  await settle();
  mock.timers.tick(5000);
  await settle();
  assert.equal(writes().length, 1);
  assert.equal(p.getPref('otw.privacy.hidden'), true);
  assert.equal(data.get('otw.privacy.hidden'), '1');
});

test('flush sends pending writes at once with keepalive', async () => {
  const p = await fresh();
  await p.prefs.load();
  calls.length = 0;
  p.setPref('otw.rail.open', false);
  p.prefs.flush();
  await settle();
  assert.deepEqual(writes(), [{ url: '/api/ui-prefs/otw.rail.open', method: 'PUT', body: { value: false }, keepalive: true }]);
  mock.timers.tick(300);
  await settle();
  assert.equal(writes().length, 1); // the debounced send was cancelled
});

test('an unreachable server leaves the app on the local copy and load can be retried', async () => {
  data.set('otw-theme', 'dark');
  respond = () => {
    throw new TypeError('offline');
  };
  const p = await fresh();
  assert.equal(await p.prefs.load(), false);
  assert.equal(p.getPref('otw-theme'), 'dark');
  respond = null;
  server = { 'otw-theme': 'light' };
  assert.equal(await p.prefs.load(), true);
  assert.equal(p.getPref('otw-theme'), 'light');
});
