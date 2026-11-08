/** Per-instrument boards (chart drawings, quick-backtest fills) as user preferences.
 *
 * A board map used to be one localStorage value, `{ [instrument]: list }`, capped at 60
 * instruments. As one preference it would cross the server's 256 KiB per value: a drawing
 * is about 160 bytes and a quick fill about 80 once serialized, so 60 instruments with 30
 * drawings, or 100 fills, each is already 280 to 480 KiB. So a map is stored one pref per
 * instrument, `<base>.<id>` (the list), plus a small index `<base>.index` (the instrument
 * keys, oldest first) that keeps the 60-instrument cap and says what to drop. One entry
 * stays far under the limit: 256 KiB is some 1,600 drawings on one instrument.
 *
 * `<id>` is the instrument key made key-safe: letters, digits, `.` and `-` kept, any other
 * character written `_` + two hex digits (`_u` + four above 0xff), so `binance|crypto|BTCUSDT`
 * becomes `binance_7ccrypto_7cBTCUSDT`. An id that would make the key longer than the
 * server's 100 characters is `x` + a 64-bit hash instead. The two forms cannot meet: an
 * instrument key holds two `|`, so an escaped id always contains `_`, a hashed one never.
 *
 * The old one-blob value is migrated once, after `prefs.load()` resolved: every instrument
 * the server does not hold yet is written as its own entry (a board another browser already
 * saved wins), then the blob is removed from localStorage. Until then, a read falls back to
 * the blob, so an old browser shows its boards on first paint. The blob is read and removed
 * with bare localStorage calls on purpose: it is the migration's source, not a preference.
 */
import { getPref, onPref, prefs, removePref, setPref } from '$lib/prefs.svelte.js';

/** The server's key cap (ui_prefs_api.rs). */
const KEY_MAX = 100;
/** Instruments kept per board map, so it cannot grow forever. */
export const MAX_INSTRUMENTS = 60;

const SAFE = /[A-Za-z0-9.-]/;

function hash64(s) {
  // Two FNV-1a passes with different offsets: 64 bits, no collision among 60 instruments.
  let a = 0x811c9dc5;
  let b = 0x01000193 ^ 0x5bd1e995;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    a = Math.imul(a ^ c, 0x01000193);
    b = Math.imul(b ^ c, 0x01000193) ^ (b >>> 15);
  }
  return (a >>> 0).toString(16).padStart(8, '0') + (b >>> 0).toString(16).padStart(8, '0');
}

/** The pref key holding one instrument's list in the map `base`. */
export function entryKey(base, instrument) {
  let id = '';
  for (const ch of String(instrument)) {
    if (SAFE.test(ch)) id += ch;
    else {
      const code = ch.codePointAt(0);
      id += code <= 0xff ? `_${code.toString(16).padStart(2, '0')}` : `_u${code.toString(16).padStart(4, '0')}`;
    }
  }
  const key = `${base}.${id}`;
  return key.length <= KEY_MAX && id !== 'index' ? key : `${base}.x${hash64(String(instrument))}`;
}

export const indexKey = (base) => `${base}.index`;

/** The index with `instrument` added (last) or removed, and what the cap pushes out. */
export function nextIndex(index, instrument, keep, max = MAX_INSTRUMENTS) {
  const list = index.filter((k) => k !== instrument);
  // An instrument already listed keeps its place: the cap drops the oldest *added*.
  if (keep) {
    const at = index.indexOf(instrument);
    if (at < 0) list.push(instrument);
    else list.splice(at, 0, instrument);
  }
  const dropped = list.length > max ? list.splice(0, list.length - max) : [];
  return { index: list, dropped };
}

/** The legacy blob's boards this browser should upload: non-empty lists of the instruments
 *  `has` (the server, after load) does not hold. In the blob's order, oldest first. */
export function legacyAdoptions(blob, has) {
  if (!blob || typeof blob !== 'object' || Array.isArray(blob)) return [];
  return Object.entries(blob).filter(([k, list]) => k && Array.isArray(list) && list.length && !has(k));
}

/** The index once `adopted` (older than anything listed) joined it, and what the cap drops. */
export function mergeIndex(index, adopted, max = MAX_INSTRUMENTS) {
  const list = [...adopted.filter((k) => !index.includes(k)), ...index];
  const dropped = list.length > max ? list.splice(0, list.length - max) : [];
  return { index: list, dropped };
}

function storage() {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** A board map kept as one pref per instrument; `base` is the old one-blob localStorage key. */
export function boardStore(base) {
  const INDEX = indexKey(base);
  let legacy; // the old blob, parsed once; null once migrated or absent
  let waiting = false;

  function readLegacy() {
    if (legacy !== undefined) return legacy;
    try {
      const raw = storage()?.getItem(base);
      const v = raw ? JSON.parse(raw) : null;
      legacy = v && typeof v === 'object' && !Array.isArray(v) ? v : null;
    } catch {
      legacy = null;
    }
    return legacy;
  }

  function writeLegacy() {
    try {
      const s = storage();
      if (!s) return;
      if (legacy && Object.keys(legacy).length) s.setItem(base, JSON.stringify(legacy));
      else s.removeItem(base);
    } catch {
      /* storage blocked: the blob is re-read next boot, its boards already have entries */
    }
  }

  function index() {
    const v = getPref(INDEX, []);
    return Array.isArray(v) ? v.filter((k) => typeof k === 'string') : [];
  }

  function migrate() {
    if (legacy === null) return;
    const blob = readLegacy();
    if (!blob) return;
    const adopted = legacyAdoptions(blob, (k) => getPref(entryKey(base, k)) !== undefined);
    for (const [k, list] of adopted) setPref(entryKey(base, k), list);
    const { index: next, dropped } = mergeIndex(index(), adopted.map(([k]) => k));
    for (const k of dropped) removePref(entryKey(base, k));
    setPref(INDEX, next);
    legacy = null;
    writeLegacy();
  }

  function track(instrument, keep) {
    const before = index();
    const { index: next, dropped } = nextIndex(before, instrument, keep);
    for (const k of dropped) removePref(entryKey(base, k));
    // An empty board on an instrument never listed changes nothing: no write.
    if (next.join('\n') !== before.join('\n')) setPref(INDEX, next);
  }

  /** Before the server map is in, nothing can tell a board the server lacks, and an index
   *  written now would replace the server's: both wait for the load. The boards saved
   *  meanwhile join the index then. */
  const early = new Map();
  function ensure() {
    if (prefs.loaded) {
      migrate();
      for (const [k, keep] of early) track(k, keep);
      early.clear();
      return;
    }
    if (waiting || (!early.size && !readLegacy())) return;
    waiting = true;
    prefs.load().then((ok) => {
      waiting = false;
      if (ok) ensure();
    });
  }

  function load(instrument) {
    if (!instrument) return [];
    ensure();
    const v = getPref(entryKey(base, instrument));
    if (Array.isArray(v)) return v;
    const old = legacy ? legacy[instrument] : undefined;
    return Array.isArray(old) ? old : [];
  }

  function save(instrument, list) {
    if (!instrument) return;
    // A write supersedes the blob's board: drop it there, or the migration would bring
    // back a board the user just cleared.
    if (readLegacy() && instrument in legacy) {
      delete legacy[instrument];
      writeLegacy();
    }
    const key = entryKey(base, instrument);
    const keep = !!list?.length;
    if (keep) setPref(key, list);
    else removePref(key);
    if (prefs.loaded) track(instrument, keep);
    else early.set(instrument, keep);
    ensure();
  }

  /** Calls `fn(list)` when an instrument's board changes, here or from the server. */
  function on(instrument, fn) {
    if (!instrument) return () => {};
    return onPref(entryKey(base, instrument), (v) => fn(Array.isArray(v) ? v : []));
  }

  return { load, save, on };
}
