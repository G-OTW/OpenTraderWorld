/**
 * User preferences: one store for every interface choice the user makes (theme, privacy
 * mask, rail widths and picks, sounds, filters), kept in the database so it follows the
 * user to another browser.
 *
 * ── The rule ──────────────────────────────────────────────────────────────────────────
 * A user preference goes through `pref()` (components) or `getPref()` / `setPref()` (plain
 * `.js` modules), never a bare `localStorage` call. The database (`/api/ui-prefs`) is the
 * source of truth; localStorage is only its mirror: the first-paint cache read before the
 * server answers, and the fallback when the server cannot be reached. Bookkeeping that is
 * genuinely per browser (this module's own markers, a one-off notice ack) is not a
 * preference and may stay in localStorage.
 *
 * ── API ───────────────────────────────────────────────────────────────────────────────
 *   prefs.load(): Promise<boolean>
 *       Once per app boot, after sign-in (+layout.svelte calls it). Adopts the server map,
 *       uploads once what this browser held before the table existed (see PREF_KEYS), and
 *       sends the writes made before it resolved. Resolves false when the server could not
 *       be read (the app runs on the local copy). Safe to call twice: one request.
 *   prefs.loaded: boolean
 *       True once `load()` has adopted the server map.
 *   prefs.flush(): void
 *       Sends every pending write now (keepalive). Wired to pagehide / hidden already.
 *   pref(key, fallback) -> { get value, set value }
 *       Reactive handle for components. `value` reads the in-memory store (the local
 *       mirror until `load()` resolves, so there is no flash of defaults) and re-renders
 *       when it changes, from this tab or from the server answer. Assigning sets the pref
 *       (same as `setPref`). Values are replaced, never mutated in place: assign a new
 *       object (`h.value = { ...h.value, open: true }`), a nested write is not saved.
 *   getPref(key, fallback) -> value
 *       Non-reactive read for `.js` modules. `fallback` when the pref is unset.
 *   setPref(key, value): void
 *       Updates memory at once, mirrors to localStorage, PUTs to the server debounced per
 *       key (300 ms, trailing). `null` / `undefined` remove the pref (`removePref`).
 *   removePref(key): void
 *       Same path as `setPref`, ending in a DELETE.
 *   onPref(key, fn) -> unsubscribe
 *       Calls `fn(value)` whenever the pref changes (a write here or the server answer at
 *       load). For `.js` stores that must re-apply a value, e.g. the theme on <html>.
 *
 * Values are JSON only: strings, numbers, booleans, objects, arrays. Keys match
 * `^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$` and stay the historical localStorage names, so a
 * key reads the same in the database and in the mirror.
 *
 * ── Migrating a key ───────────────────────────────────────────────────────────────────
 * 1. Add it to PREF_KEYS below with the codec matching the string localStorage already
 *    holds for it: `json` (JSON.stringify), `string` (raw text), `bool01` ('1' / '0'),
 *    `number` (String(n)). The codec keeps the mirror byte-identical to what the old code
 *    wrote, so anything reading localStorage before the app boots (static/prepaint.js)
 *    keeps working, and so the one-time upload decodes the old value.
 * 2. Replace the localStorage calls with `pref` / `getPref` / `setPref`.
 * The upload is tracked per key (`otw.prefs.migrated.v1` lists the keys done), so a key
 * added to the table later is uploaded once at the next load, never overwriting a value
 * the server already has. A key not in the table is mirrored as JSON and never uploaded
 * from an old local value.
 *
 * ── Failure ───────────────────────────────────────────────────────────────────────────
 * A PUT that fails on the network or with a 5xx is retried once, then logged; a 4xx
 * (refused, e.g. the demo sandbox) is logged without a retry. Either way the local copy
 * stays. A write not acknowledged by the server is remembered (`otw.prefs.dirty.v1`), so
 * the next `load()` keeps the local value over the server's and sends it again.
 */

const API = '/api/ui-prefs';
const MIGRATED_KEY = 'otw.prefs.migrated.v1';
const DIRTY_KEY = 'otw.prefs.dirty.v1';
const DEBOUNCE_MS = 300;
const RETRY_MS = 1000;
/** Server caps (ui_prefs_api.rs): bytes per value, keys per bulk call. */
const VALUE_MAX = 256 * 1024;
const BULK_MAX = 200;
/** Bytes per bulk call, kept under the server's 2 MB request body limit. */
const BULK_BYTES = 1_500_000;
/** A keepalive request may carry at most 64 KiB; a bigger one is sent plainly. */
const KEEPALIVE_MAX = 60 * 1024;
const KEY_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;

/**
 * The localStorage keys migrated to the database, with the codec of the string each one
 * holds in localStorage. Listing a key here is what uploads its old local value once.
 */
export const PREF_KEYS = {
  'otw-theme': 'string',
  'otw.privacy.hidden': 'bool01',
  'otw.alert.sound': 'string',
  'otw.rail.open': 'bool01',
  'otw.rail.folded': 'json',
  'otw.histviz.railW': 'number',
  'otw.histviz.railOpen': 'bool01',
  'otw.histviz.rail.v1': 'string',
  'otw.calendar.overlays': 'json',
  'otw.calendar.viewIndex': 'number',
  'otw.todos.timeline': 'bool01',
  'mindset.day.collapsed': 'json',
  'otw-mindset-journal': 'string',
  'routines.board.collapsed': 'json',
  'otw.mailbox.tab': 'string',
  'options.chain.columns': 'json',
  'options.tab': 'string',
  'quant.positionSize': 'json',
  'quant.tab': 'string',
  'otw.mportfolios.tab.v1': 'string',
  'otw-scanner-board': 'string',
  'findb.ui': 'json',
  'otw.connectors.selected.v1': 'string',
  'otw.notifchannels.selected.v1': 'string',
  'otw.assistant.conv': 'string',
  'otw.assistant.open': 'bool01',
  'otw-agent-wide': 'bool01',
  'otw.search.deep': 'bool01',
  'otw.mobile.quick-search.position': 'json',
  'otw.journal.prefs.v1': 'json',
  'otw.journal.trades.filters.v1': 'json',
  'otw.journal.filters.v1': 'json',
  'otw.journal.breakdown.filters.v1': 'json',
  'otw.journal.analytics.filters.v1': 'json',
  'otw.journal.calendar.month.v1': 'json',
  'otw.journal.compare.period.v1': 'string',
  'otw.journal.scatter.v1': 'json',
  'otw.journal.scatter3d.v1': 'json',
  'otw.journal.streaks3d.v1': 'json',
  'otw.backtest.scatter3d.v1': 'json',
  'otw.backtest.streaks3d.v1': 'json',
  'otw.backtest.paper.scatter3d.v1': 'json',
  'otw.backtest.paper.streaks3d.v1': 'json',
  'otw.journal.breakdown.detailsOpen.v1': 'bool01',
  'otw.histdata.prefs.v1': 'json',
  'otw.histdata.datasetFilters.v1': 'json',
  'otw.histdata.jobFilters.v1': 'json',
  'otw.news.prefs.v1': 'json',
  'otw.resources.prefs.v1': 'json',
  'otw.prompt-store.prefs.v1': 'json',
  'otw.time.prefs.v1': 'json',
  'otw.communityDocs.prefs.v1': 'json',
  'fundamentals.prefs.v2': 'json',
  'otw.histviz.workspace.v1': 'string',
  'otw.histviz.orders.v1': 'json',
  'otw.histviz.data.v1': 'json',
  'otw.scanner.data.v1': 'json',
  'otw.histviz.ticket.v1': 'json',
  'otw.histviz.tradesize.v1': 'json',
  'otw.histviz.quick': 'bool01',
  'otw.histviz.recents.v1': 'json',
  'otw.backtest.speed.v1': 'json',
  'otw.backtest.chartH': 'number',
  'otw.backtest.dockH': 'number',
  'otw.backtest.dockOpen': 'bool01',
  'otw.backtest.cloud.v1': 'json',
  'otw.backtest.dca3d.v1': 'json'
};

const CODECS = {
  json: { decode: (raw) => JSON.parse(raw), encode: (v) => JSON.stringify(v) },
  string: { decode: (raw) => raw, encode: (v) => (typeof v === 'string' ? v : JSON.stringify(v)) },
  bool01: {
    decode: (raw) => (raw === '1' ? true : raw === '0' ? false : undefined),
    encode: (v) => (v ? '1' : '0')
  },
  number: {
    decode: (raw) => {
      const n = Number(raw);
      return raw.trim() !== '' && Number.isFinite(n) ? n : undefined;
    },
    encode: (v) => String(v)
  }
};

const codec = (key) => CODECS[PREF_KEYS[key]] ?? CODECS.json;

/** key -> value; a key absent from the map has not been read yet. */
const mem = new Map();
/** key -> debounce timer, or null for a write held until `load()` resolves. */
const pending = new Map();
const listeners = new Map();
let loaded = false;
let loading = null;

function storage() {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null; // storage blocked
  }
}

function readLocal(key) {
  try {
    const raw = storage()?.getItem(key);
    if (raw == null) return undefined;
    const v = codec(key).decode(raw);
    return v === null ? undefined : v;
  } catch {
    return undefined; // unreadable old value: treat as unset
  }
}

function writeLocal(key, value) {
  try {
    const s = storage();
    if (!s) return;
    if (value === undefined) s.removeItem?.(key);
    else s.setItem(key, codec(key).encode(value));
  } catch {
    /* storage full or blocked: memory and the server still hold it */
  }
}

function readList(name) {
  try {
    const v = JSON.parse(storage()?.getItem(name) ?? '[]');
    return new Set(Array.isArray(v) ? v : []);
  } catch {
    return new Set();
  }
}

function writeList(name, set) {
  try {
    storage()?.setItem(name, JSON.stringify([...set]));
  } catch {
    /* storage blocked */
  }
}

function markDirty(key, on) {
  const dirty = readList(DIRTY_KEY);
  if (on === dirty.has(key)) return;
  if (on) dirty.add(key);
  else dirty.delete(key);
  writeList(DIRTY_KEY, dirty);
}

function read(key) {
  if (!mem.has(key)) mem.set(key, readLocal(key));
  return mem.get(key);
}

function emit(key, value) {
  for (const fn of listeners.get(key) ?? []) {
    try {
      fn(value);
    } catch (e) {
      console.error(`pref listener for ${key}`, e);
    }
  }
}

function checkKey(key) {
  if (typeof key !== 'string' || !KEY_RE.test(key)) throw new Error(`invalid preference key ${JSON.stringify(key)}`);
}

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

// ── Server writes ─────────────────────────────────────────────────────────────────────

function schedule(key) {
  clearTimeout(pending.get(key) ?? undefined);
  // Before the server map is in, a write is held: sending it now could be overwritten by
  // the answer, and on the sign-in screen there is no session to send it with.
  pending.set(key, loaded ? setTimeout(() => send(key), DEBOUNCE_MS) : null);
}

async function send(key, { retry = true, keepalive = false } = {}) {
  pending.delete(key);
  const value = mem.get(key);
  const url = `${API}/${encodeURIComponent(key)}`;
  const init =
    value === undefined
      ? { method: 'DELETE' }
      : { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ value }) };
  if (keepalive && (init.body?.length ?? 0) <= KEEPALIVE_MAX) init.keepalive = true;
  let failure;
  try {
    const res = await fetch(url, init);
    if (res.ok) {
      // A newer write made meanwhile is still owed to the server.
      if (!pending.has(key)) markDirty(key, false);
      return;
    }
    if (res.status < 500) {
      // Refused (validation, or the demo sandbox): asking again would be refused again.
      if (!pending.has(key)) markDirty(key, false);
      console.warn(`preference ${key} was refused (${res.status}); kept in this browser only`);
      return;
    }
    failure = `status ${res.status}`;
  } catch (e) {
    failure = e?.message ?? String(e);
  }
  if (pending.has(key)) return; // a newer write supersedes this one
  if (retry && !keepalive) {
    pending.set(
      key,
      setTimeout(() => send(key, { retry: false }), RETRY_MS)
    );
    return;
  }
  console.warn(`preference ${key} was not saved to the server (${failure}); kept in this browser`);
}

function flush() {
  for (const [key, timer] of [...pending]) {
    if (timer === null) continue; // held until load(): nothing to send it with yet
    clearTimeout(timer);
    send(key, { keepalive: true });
  }
}

// ── Load and one-time upload ──────────────────────────────────────────────────────────

async function migrate(server) {
  const done = readList(MIGRATED_KEY);
  const considered = Object.keys(PREF_KEYS).filter((k) => !done.has(k));
  if (!considered.length) return;
  const upload = {};
  for (const key of considered) {
    if (key in server || pending.has(key)) continue; // the server has it, or a write is owed anyway
    const v = readLocal(key);
    if (v === undefined) continue;
    if (JSON.stringify(v).length > VALUE_MAX) {
      console.warn(`preference ${key} is too large to upload; kept in this browser`);
      continue;
    }
    upload[key] = v;
  }
  // Chunks under the server's caps (keys and bytes per call).
  const chunks = [];
  let chunk = {};
  let size = 0;
  for (const [key, v] of Object.entries(upload)) {
    const n = key.length + JSON.stringify(v).length + 4;
    if (Object.keys(chunk).length >= BULK_MAX || (size + n > BULK_BYTES && size > 0)) {
      chunks.push(chunk);
      chunk = {};
      size = 0;
    }
    chunk[key] = v;
    size += n;
  }
  if (Object.keys(chunk).length) chunks.push(chunk);
  for (const c of chunks) {
    try {
      const res = await fetch(API, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ prefs: c })
      });
      // A 4xx is final (the demo refuses bulk writes): the local copies stay, as they are.
      if (!res.ok && res.status >= 500) throw new Error(`status ${res.status}`);
      if (!res.ok) console.warn(`preference upload was refused (${res.status}); kept in this browser`);
    } catch (e) {
      console.warn('preference upload failed; retried at the next load', e);
      return; // not marked: the next boot tries again
    }
  }
  for (const k of considered) done.add(k);
  writeList(MIGRATED_KEY, done);
}

async function load() {
  if (loading) return loading;
  loading = (async () => {
    let server;
    try {
      const res = await fetch(API, { headers: { accept: 'application/json' } });
      if (!res.ok) throw new Error(`status ${res.status}`);
      server = (await res.json())?.prefs ?? {};
    } catch (e) {
      console.warn('preferences could not be loaded; using this browser copy', e);
      loading = null; // a later call may try again
      return false;
    }
    // Writes not yet acknowledged (this boot, or a past one that failed) beat the server.
    const owed = readList(DIRTY_KEY);
    for (const key of owed) if (!pending.has(key)) pending.set(key, null);
    for (const [key, value] of Object.entries(server)) {
      if (pending.has(key) || !KEY_RE.test(key)) continue;
      const before = read(key);
      mem.set(key, value);
      writeLocal(key, value);
      if (!same(before, value)) emit(key, value);
    }
    loaded = true;
    await migrate(server);
    for (const key of [...pending.keys()]) if (pending.get(key) === null) schedule(key);
    return true;
  })();
  return loading;
}

export const prefs = {
  load,
  flush,
  get loaded() {
    return loaded;
  }
};

if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
  window.addEventListener('pagehide', flush);
  document.addEventListener?.('visibilitychange', () => {
    if (document.visibilityState === 'hidden') flush();
  });
}

// ── Public reads and writes ───────────────────────────────────────────────────────────

export function getPref(key, fallback) {
  const v = read(key);
  return v === undefined ? fallback : v;
}

export function setPref(key, value) {
  checkKey(key);
  if (value === undefined || value === null) return removePref(key);
  const json = JSON.stringify(value);
  if (json === undefined) throw new Error(`preference ${key} is not a JSON value`);
  const before = read(key);
  if (json === JSON.stringify(before) && !pending.has(key)) return;
  const clean = JSON.parse(json); // a plain copy: no proxy, nothing shared with the caller
  mem.set(key, clean);
  writeLocal(key, clean);
  emit(key, clean);
  if (json.length > VALUE_MAX) {
    console.warn(`preference ${key} is over ${VALUE_MAX} bytes; kept in this browser only`);
    return;
  }
  markDirty(key, true);
  schedule(key);
}

export function removePref(key) {
  checkKey(key);
  if (read(key) === undefined && !pending.has(key)) return;
  mem.set(key, undefined);
  writeLocal(key, undefined);
  emit(key, undefined);
  markDirty(key, true);
  schedule(key);
}

export function onPref(key, fn) {
  let set = listeners.get(key);
  if (!set) listeners.set(key, (set = new Set()));
  set.add(fn);
  return () => set.delete(fn);
}

// ── Reactive handle ───────────────────────────────────────────────────────────────────

/** One reactive cell per key, shared by every handle on it: no per-component listener. */
const cells = new Map();

function cell(key) {
  let c = cells.get(key);
  if (!c) {
    let current = $state.raw(read(key));
    onPref(key, (v) => {
      current = v;
    });
    c = {
      get current() {
        return current;
      }
    };
    cells.set(key, c);
  }
  return c;
}

export function pref(key, fallback) {
  checkKey(key);
  const c = cell(key);
  return {
    get value() {
      const v = c.current;
      return v === undefined ? fallback : v;
    },
    set value(v) {
      setPref(key, v);
    }
  };
}
