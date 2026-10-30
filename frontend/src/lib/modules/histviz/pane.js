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

const UNIT_SECS = { m: 60, h: 3600, d: 86400, w: 604800 };

/** Seconds in a timeframe label (`7m`, `3h`, `2w`), canonical or custom; null if unknown. */
export function tfSeconds(tf) {
  if (TF_SECS[tf]) return TF_SECS[tf];
  const m = /^(\d+)([mhdw])$/.exec(tf ?? '');
  return m && +m[1] > 0 ? +m[1] * UNIT_SECS[m[2]] : null;
}

/** The coarsest served timeframe that divides `tf`, when `tf` is not served itself: what a
 *  custom timeframe is consolidated from. Mirrors the Rust `consolidation_base`. */
export function consolidationBase(served, tf) {
  if (!served?.length || served.includes(tf)) return null;
  const t = tfSeconds(tf);
  if (!t) return null;
  let best = null;
  for (const s of served) {
    const b = tfSeconds(s);
    if (b && t > b && t % b === 0 && (!best || t / b < best.ratio)) best = { tf: s, ratio: t / b };
  }
  return best?.tf ?? null;
}

/** Order timeframes by duration (unknown ones last). */
export const byDuration = (a, b) => (tfSeconds(a) ?? Infinity) - (tfSeconds(b) ?? Infinity);

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
const TRADE_SIZE_KEY = 'otw.histviz.tradesize.v1';
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

/** The quick bar's size on an instrument: the unit it is typed in and the value, kept
 *  together since a value means nothing in another unit. Null when none was set. */
export function loadTradeSize(instrument) {
  const v = instrument ? readStore(TRADE_SIZE_KEY)[instrument] : null;
  return v && typeof v.unit === 'string' && Number(v.qty) > 0 ? { unit: v.unit, qty: Number(v.qty) } : null;
}

export function saveTradeSize(instrument, { unit, qty }) {
  if (!instrument || !(Number(qty) > 0)) return;
  const store = readStore(TRADE_SIZE_KEY);
  // Newest last, so the trim drops the instruments untouched the longest.
  delete store[instrument];
  store[instrument] = { unit, qty: Number(qty) };
  writeStore(TRADE_SIZE_KEY, store);
}

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
