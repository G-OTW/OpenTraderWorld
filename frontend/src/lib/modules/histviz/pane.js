/** Shared constants and per-instrument scratch storage for a chart pane.
 *
 * The drawings and the quick-backtest fills are **per instrument, not per pane and not per
 * timeframe**: an anchor is an instant and a price, so a trend line drawn on the 1h is the
 * same line on the 15m, and two panes showing one symbol show one board. They live in this
 * browser (a click-through session is a scratchpad, never journal data); what does travel to
 * the server is the instrument's *layout*, which the pane saves through `histvizApi`. */

/** Bars per "load more" click, and per first paint. */
export const SLICE_BARS = 1500;

/** Seconds in one canonical timeframe (mirrors the Rust `timeframe_secs`). */
export const TF_SECS = {
  '1m': 60,
  '5m': 300,
  '15m': 900,
  '1h': 3600,
  '4h': 14400,
  '1d': 86400,
  '1w': 604800
};

/** Timeframe strip order, by duration, the way every market tool orders it. */
export const TF_ORDER = [
  '1m',
  '2m',
  '5m',
  '10m',
  '15m',
  '30m',
  '45m',
  '1h',
  '2h',
  '3h',
  '4h',
  '6h',
  '8h',
  '12h',
  '1d',
  '3d',
  '1w'
];

/** An instrument, ignoring the timeframe: what a drawing board belongs to. */
export const instrumentKeyOf = (c) =>
  c ? `${c.provider}|${c.asset_type}|${c.ticker}` : '';

/** An instrument at a timeframe: what a layout and a live subscription belong to. */
export const coordKeyOf = (c) =>
  c ? `${c.provider}|${c.asset_type}|${c.ticker}|${c.timeframe}` : '';

const RECENTS_KEY = 'otw.histviz.recents.v1';
const DRAW_KEY = 'otw.histviz.drawings.v1';
const QUICK_KEY = 'otw.histviz.quick.v1';
/** Instruments kept in either scratch store, so it cannot grow forever. */
const MAX_INSTRUMENTS = 60;

function readStore(key) {
  try {
    const raw = JSON.parse(localStorage.getItem(key) || '{}');
    return raw && typeof raw === 'object' ? raw : {};
  } catch {
    return {};
  }
}

function writeStore(key, store) {
  const keys = Object.keys(store);
  for (const k of keys.slice(0, Math.max(0, keys.length - MAX_INSTRUMENTS))) delete store[k];
  try {
    localStorage.setItem(key, JSON.stringify(store));
  } catch {
    /* quota: the board stays in memory for this session */
  }
}

function readList(key, instrument) {
  if (!instrument) return [];
  return readStore(key)[instrument] ?? [];
}

function writeList(key, instrument, list) {
  if (!instrument) return;
  const store = readStore(key);
  if (list.length) store[instrument] = list;
  else delete store[instrument];
  writeStore(key, store);
}

export const loadDrawings = (instrument) => readList(DRAW_KEY, instrument);
export const saveDrawings = (instrument, list) => writeList(DRAW_KEY, instrument, list);
export const loadQuick = (instrument) => readList(QUICK_KEY, instrument);
export const saveQuick = (instrument, list) => writeList(QUICK_KEY, instrument, list);

/** Instruments charted lately, newest first. Shared by the rail and by each pane's picker,
 *  so "recent" means the same thing wherever it is shown. */
export function loadRecents() {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENTS_KEY) || '[]');
    return Array.isArray(raw) ? raw : [];
  } catch {
    return [];
  }
}

export function rememberRecent(coords) {
  if (!coords?.ticker) return loadRecents();
  const key = instrumentKeyOf(coords);
  const next = [
    { ...coords },
    ...loadRecents().filter((r) => instrumentKeyOf(r) !== key)
  ].slice(0, 12);
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  } catch {
    /* non-fatal */
  }
  return next;
}
