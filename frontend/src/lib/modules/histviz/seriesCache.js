/** Series a chart pane already read, kept in this browser so going back to a symbol or a
 *  timeframe, or reloading the page, draws at once and only asks the server for the bars that
 *  came after.
 *
 *  Two levels: a memory map the panes read synchronously, mirrored into IndexedDB so a reload
 *  finds it again. Records are compact (stamps in ms, columns as Float64Array). Bounded twice,
 *  by series and by bars, least recently used out first, in memory and on disk alike.
 *
 *  How long a series is kept is the user's choice (Settings → Chart cache): until the tab closes, or
 *  1 h to 7 days since it was last written. "Until the tab closes" rides a session id in
 *  sessionStorage (it survives a reload, not a close) and a heartbeat per open tab, so a tab
 *  being opened never clears what another open tab holds. A logout clears it all when the user
 *  chose so. The choice is kept per browser (localStorage), like the cache itself. Without IndexedDB (a private window, a test) it is memory only. */

const MAX_SERIES = 20;
const MAX_BARS = 400_000;

const DB_NAME = 'otw-chart-cache';
const SERIES = 'series';
const SESSIONS = 'sessions';
const CONFIG_KEY = 'otw.chart.cache.v1';
const SESSION_KEY = 'otw.chart.session';
const HEARTBEAT_MS = 60_000;
const DEAD_AFTER_MS = 3 * HEARTBEAT_MS;
const PURGE_EVERY_MS = 3_600_000;

/** Retention choices, in ms since a series was last written; `tab` = until the tab closes. */
export const RETENTIONS = {
  tab: null,
  '1h': 3_600_000,
  '6h': 6 * 3_600_000,
  '1d': 86_400_000,
  '3d': 3 * 86_400_000,
  '7d': 7 * 86_400_000
};
export const DEFAULT_RETENTION = '1d';

// ── Config, mirrored per browser: the cache is this browser's, and a logout or a page that
// never opens a chart still has to know it. ──

function readConfig() {
  try {
    const raw = JSON.parse(localStorage.getItem(CONFIG_KEY) ?? 'null');
    return normalizeConfig(raw);
  } catch {
    return normalizeConfig(null);
  }
}

function normalizeConfig(raw) {
  const retention = raw?.retention in RETENTIONS ? raw.retention : DEFAULT_RETENTION;
  return { retention, clearOnLogout: raw?.clearOnLogout !== false };
}

let config = readConfig();

/** The choice in force in this browser: `{ retention, clearOnLogout }`. */
export const seriesCacheConfig = () => ({ ...config });

/** Apply the choice made in Settings → Chart cache, and purge what it no longer keeps. */
export function configureSeriesCache({ retention, clearOnLogout } = {}) {
  const next = normalizeConfig({ retention, clearOnLogout });
  if (next.retention === config.retention && next.clearOnLogout === config.clearOnLogout) return;
  config = next;
  try {
    localStorage.setItem(CONFIG_KEY, JSON.stringify(config));
  } catch {
    // Storage blocked: the choice holds for this page.
  }
  void purge();
}

// ── Memory level ──

/** key → record, oldest use first (a Map keeps insertion order). */
const memory = new Map();
let held = 0;

const size = (rec) => rec?.t?.length ?? 0;
const COLUMNS = ['o', 'h', 'l', 'c', 'v'];

/** A server stamp again: RFC 3339 without milliseconds when there are none, as the server
 *  writes it. */
const stamp = (ms) => new Date(ms).toISOString().replace('.000Z', 'Z');

/** A series answer as a compact record: stamps in ms, columns as Float64Array, the rest of
 *  the answer (provider, notice, continuous…) as it was. */
export function toRecord(key, bars, atHistoryStart) {
  const { ts, o, h, l, c, v, ...meta } = bars;
  return {
    key,
    session: sessionId(),
    at: Date.now(),
    atHistoryStart: !!atHistoryStart,
    meta,
    t: Float64Array.from(ts, (s) => Date.parse(s)),
    o: Float64Array.from(o),
    h: Float64Array.from(h),
    l: Float64Array.from(l),
    c: Float64Array.from(c),
    v: Float64Array.from(v ?? [], (x) => x ?? 0)
  };
}

/** A record as the series answer the pane draws. */
export function fromRecord(rec) {
  const bars = { ...rec.meta, ts: Array.from(rec.t, stamp), count: rec.t.length };
  for (const k of COLUMNS) bars[k] = Array.from(rec[k]);
  return { bars, atHistoryStart: rec.atHistoryStart };
}

/** The cache key of a pane's coordinates: the connector too, two accounts at one provider
 *  can disagree on what they serve. */
export const seriesKey = (c) =>
  c ? `${c.connector_id ?? ''}|${c.provider}|${c.asset_type}|${c.ticker}|${c.timeframe}` : '';

function remember(rec) {
  forget(rec.key, false);
  memory.set(rec.key, rec);
  held += size(rec);
  while (memory.size > MAX_SERIES || (held > MAX_BARS && memory.size > 1)) {
    forget(memory.keys().next().value, true);
  }
}

function forget(key, fromDisk) {
  const rec = memory.get(key);
  if (!rec) return;
  held -= size(rec);
  memory.delete(key);
  if (fromDisk) void idb('readwrite', (s) => s.delete(key));
}

/** Keep a pane's series. `bars` is a plain snapshot (the series answer with every bar the pane
 *  holds); an empty one is not kept. */
export function keepSeries(key, bars, atHistoryStart) {
  if (!key || !bars?.ts?.length) return;
  const rec = toRecord(key, bars, atHistoryStart);
  remember(rec);
  void idb('readwrite', (s) => s.put(rec));
}

/** The series kept under `key`, now the most recently used, or null. */
export function takeSeries(key) {
  const rec = memory.get(key);
  if (!rec) return null;
  memory.delete(key);
  memory.set(key, rec);
  return fromRecord(rec);
}

/** Forget everything, in memory and on disk. */
export function clearSeries() {
  memory.clear();
  held = 0;
  void idb('readwrite', (s) => s.clear());
}

/** A logout: clears the cache when the user chose to. */
export function logoutSeries() {
  if (config.clearOnLogout) clearSeries();
}

/** A kept series brought up to date by a fresh read of the newest bars: the kept bars before
 *  the fresh ones, then the fresh ones, with the fresh answer's metadata (notice, live reach,
 *  stored dataset). Null when the two do not meet (bars missing between them), so the caller
 *  reads the window again instead of drawing a hole as if the market had not traded. */
export function joinFresh(kept, fresh) {
  if (!kept?.ts?.length) return null;
  if (!fresh?.ts?.length) {
    return { ...fresh, ts: kept.ts, o: kept.o, h: kept.h, l: kept.l, c: kept.c, v: kept.v, count: kept.ts.length };
  }
  const first = Date.parse(fresh.ts[0]);
  if (first > Date.parse(kept.ts[kept.ts.length - 1])) return null;
  let n = 0;
  while (n < kept.ts.length && Date.parse(kept.ts[n]) < first) n++;
  const out = {
    ...fresh,
    ts: kept.ts.slice(0, n).concat(fresh.ts),
    o: kept.o.slice(0, n).concat(fresh.o),
    h: kept.h.slice(0, n).concat(fresh.h),
    l: kept.l.slice(0, n).concat(fresh.l),
    c: kept.c.slice(0, n).concat(fresh.c),
    v: kept.v.slice(0, n).concat(fresh.v)
  };
  out.count = out.ts.length;
  return out;
}

/** For tests: how many series and bars are held in memory. */
export const heldSeries = () => ({ series: memory.size, bars: held });

// ── Disk level ──

let session = null;

/** This tab's id: kept in sessionStorage, so a reload keeps it and a closed tab's is gone. */
function sessionId() {
  if (session) return session;
  try {
    session = sessionStorage.getItem(SESSION_KEY);
    if (!session) {
      session = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
      sessionStorage.setItem(SESSION_KEY, session);
    }
  } catch {
    session ??= 'memory';
  }
  return session;
}

let dbOpen = null;

function openDb() {
  if (typeof indexedDB === 'undefined') return Promise.resolve(null);
  dbOpen ??= new Promise((resolve) => {
    try {
      const req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = () => {
        req.result.createObjectStore(SERIES, { keyPath: 'key' });
        req.result.createObjectStore(SESSIONS, { keyPath: 'id' });
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
      req.onblocked = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
  return dbOpen;
}

/** One transaction on a store, resolved when it commits; nothing at all without a database.
 *  A failed write only costs a refetch later, so it never throws. */
async function idb(mode, run, store = SERIES) {
  const db = await openDb();
  if (!db) return null;
  return new Promise((resolve) => {
    try {
      const tx = db.transaction(store, mode);
      const out = run(tx.objectStore(store));
      tx.oncomplete = () => resolve(out?.result ?? null);
      tx.onerror = () => resolve(null);
      tx.onabort = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

const all = (store) => idb('readonly', (s) => s.getAll(), store).then((r) => r ?? []);

function beat() {
  return idb('readwrite', (s) => s.put({ id: sessionId(), seen: Date.now() }), SESSIONS);
}

/** Drop what the retention no longer keeps: past its age, or, kept until the tab closes,
 *  written by a tab that has stopped beating. Memory follows. */
async function purge() {
  const now = Date.now();
  const ms = RETENTIONS[config.retention];
  let stale;
  if (ms == null) {
    const sessions = await all(SESSIONS);
    const alive = new Set(sessions.filter((x) => now - x.seen < DEAD_AFTER_MS).map((x) => x.id));
    alive.add(sessionId());
    stale = (rec) => !alive.has(rec.session);
    const dead = sessions.filter((x) => !alive.has(x.id)).map((x) => x.id);
    if (dead.length) await idb('readwrite', (s) => dead.forEach((id) => s.delete(id)), SESSIONS);
  } else {
    stale = (rec) => now - rec.at > ms;
  }
  const records = await all(SERIES);
  const gone = records.filter(stale).map((r) => r.key);
  if (gone.length) await idb('readwrite', (s) => gone.forEach((k) => s.delete(k)));
  for (const k of gone) forget(k, false);
}

/** Open the disk level: beat, purge, then load what is left into memory, newest writes kept
 *  first when it holds more than memory does. Resolves once memory is warm. */
async function start() {
  if (typeof window === 'undefined' || !(await openDb())) return;
  await beat();
  setInterval(beat, HEARTBEAT_MS);
  setInterval(purge, PURGE_EVERY_MS);
  try {
    void navigator.storage?.persist?.();
  } catch {
    // Best effort: the browser may still evict under disk pressure.
  }
  await purge();
  const records = (await all(SERIES)).sort((a, b) => a.at - b.at);
  for (const rec of records) if (!memory.has(rec.key)) remember(rec);
}

/** Resolves when the series kept on disk are in memory. */
export const seriesReady = start();
