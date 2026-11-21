/** Chain columns: what each one is, how it prints, whether it shows by default. Order and
 * visibility are the user's, a preference (`$lib/prefs.svelte.js`) under `options.chain.columns`. */
import { fmtNum, fmtFixed } from '$lib/format.js';
import { getPref, onPref, setPref } from '$lib/prefs.svelte.js';

/** What a missing value prints as: quiet, never NaN. */
export const BLANK = '·';

const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

export const price = (v) => (isNum(v) ? fmtFixed(v, 2) : BLANK);
export const greek = (v) => (isNum(v) ? fmtFixed(v, 4) : BLANK);
export const pct = (v, d = 2) => (isNum(v) ? `${fmtFixed(v * 100, d)}%` : BLANK);
export const count = (v) => (isNum(v) ? fmtNum(v, 0) : BLANK);
export const num = (v, d = 2) => (isNum(v) ? fmtNum(v, d) : BLANK);

/** key = Row field, label = i18n key under `options.col.*`. */
export const COLUMNS = [
  { key: 'bid', fmt: price, show: true },
  { key: 'ask', fmt: price, show: true },
  { key: 'mid', fmt: price, show: true },
  { key: 'last', fmt: price, show: false },
  { key: 'spread_pct', fmt: pct, show: false },
  { key: 'theo', fmt: price, show: false },
  { key: 'volume', fmt: count, show: true },
  { key: 'open_interest', fmt: count, show: true },
  { key: 'iv_mid', fmt: pct, show: true },
  { key: 'iv_bid', fmt: pct, show: false },
  { key: 'iv_ask', fmt: pct, show: false },
  { key: 'iv_spread', fmt: pct, show: false },
  { key: 'delta', fmt: greek, show: true },
  { key: 'gamma', fmt: greek, show: false },
  { key: 'vega', fmt: greek, show: false },
  { key: 'theta', fmt: greek, show: true },
  { key: 'rho', fmt: greek, show: false },
  { key: 'distance', fmt: price, show: false },
  { key: 'rel_distance', fmt: pct, show: false },
  { key: 'bid_pct', fmt: pct, show: false },
  { key: 'ask_pct', fmt: pct, show: false },
  { key: 'ann_bid_pct', fmt: pct, show: false },
  { key: 'ann_ask_pct', fmt: pct, show: false },
  { key: 'intrinsic', fmt: price, show: false },
  { key: 'time_value', fmt: price, show: false },
  { key: 'breakeven', fmt: price, show: false },
  { key: 'to_be_pct', fmt: pct, show: false }
];

export const BY_KEY = Object.fromEntries(COLUMNS.map((c) => [c.key, c]));

const STORE_KEY = 'options.chain.columns';

/** Saved layout `{ order: [key], visible: [key] }`, repaired against the current list: an
 * unknown key is dropped, a column added since is appended with its default visibility. */
export function repairLayout(saved) {
  const known = COLUMNS.map((c) => c.key);
  const order = (Array.isArray(saved?.order) ? saved.order : []).filter((k) => known.includes(k));
  for (const k of known) if (!order.includes(k)) order.push(k);
  const visible = Array.isArray(saved?.visible)
    ? saved.visible.filter((k) => known.includes(k))
    : COLUMNS.filter((c) => c.show).map((c) => c.key);
  return { order, visible };
}

export function loadLayout() {
  return repairLayout(getPref(STORE_KEY, null));
}

export function saveLayout(layout) {
  setPref(STORE_KEY, { order: layout.order, visible: layout.visible });
}

/** Calls `fn(layout)` when the saved layout changes (a write here, or the server's answer
 * at load). Returns the unsubscribe. */
export function onLayout(fn) {
  return onPref(STORE_KEY, () => fn(loadLayout()));
}

export function defaultLayout() {
  return { order: COLUMNS.map((c) => c.key), visible: COLUMNS.filter((c) => c.show).map((c) => c.key) };
}
