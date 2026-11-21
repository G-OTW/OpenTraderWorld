/** Entry presets: a market entry with its take profit and stop loss set at a distance, kept in
 *  the global chart settings (`tradePresets`) so every workspace offers the same list.
 *
 *  A preset is `{ id, name, side, qty, tp, sl, key }`: `side` 'buy' | 'sell' | 'both' (the
 *  ticket keeps the side it shows); `qty` a size,
 *  or null to keep the size the chart shows; `tp` / `sl` `{ value, unit }` or null, the unit
 *  'ticks' | 'points' | 'money' | 'pct'; `key` 1 to 9 or null. The quick bar applies one to
 *  both its buttons (the side is the button's); a key fills the order ticket with it, and the
 *  user sends it from there. */

export const UNITS = ['ticks', 'points', 'money', 'pct'];
export const SIDES = ['buy', 'sell', 'both'];
export const KEYS = [1, 2, 3, 4, 5, 6, 7, 8, 9];

export const presetsIn = (settings) =>
  Array.isArray(settings?.tradePresets) ? settings.tradePresets.filter((p) => p?.id && p?.name) : [];

/** The list to show: by key, 1 first, presets without a key after, by name. */
export const byKey = (list) =>
  [...list].sort((a, b) => (a.key ?? 99) - (b.key ?? 99) || String(a.name).localeCompare(String(b.name)));

/** Whether the keys 1 to 9 fill the ticket. */
export const keysOn = (settings) => settings?.presetKeys === true;

export function blankPreset() {
  return {
    id: '',
    name: '',
    side: 'buy',
    qty: null,
    tp: { value: 20, unit: 'ticks' },
    sl: { value: 10, unit: 'ticks' },
    key: null
  };
}

const positive = (v) => Number.isFinite(Number(v)) && Number(v) > 0;

/** A preset as stored: sizes and distances as numbers, an exit without a distance dropped. */
export function cleanPreset(p) {
  const exit = (x) => (x && positive(x.value) && UNITS.includes(x.unit) ? { value: Number(x.value), unit: x.unit } : null);
  return {
    id: p.id || `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
    name: String(p.name ?? '').trim(),
    side: SIDES.includes(p.side) ? p.side : 'buy',
    qty: positive(p.qty) ? Number(p.qty) : null,
    tp: exit(p.tp),
    sl: exit(p.sl),
    key: KEYS.includes(Number(p.key)) ? Number(p.key) : null
  };
}

/** The list with `p` saved: replacing the one with its id, and taking its key from any other. */
export function savePreset(list, p) {
  const c = cleanPreset(p);
  const rest = list.filter((x) => x.id !== c.id).map((x) => (c.key != null && x.key === c.key ? { ...x, key: null } : x));
  const i = list.findIndex((x) => x.id === c.id);
  if (i < 0) return [...rest, c];
  const out = [...rest];
  out.splice(Math.min(i, out.length), 0, c);
  return out;
}

/** How far from the entry an exit sits, in price. `null` when it cannot be told (no tick for
 *  ticks, no size for an amount of money). */
export function distanceOf(exit, { ref, tick = null, pointValue = null, qty = null }) {
  if (!exit || !positive(exit.value) || !positive(ref)) return null;
  const v = Number(exit.value);
  switch (exit.unit) {
    case 'ticks':
      return positive(tick) ? v * Number(tick) : null;
    case 'points':
      return v;
    case 'money': {
      const per = (positive(pointValue) ? Number(pointValue) : 1) * Number(qty);
      return positive(per) ? v / per : null;
    }
    case 'pct':
      return (Number(ref) * v) / 100;
    default:
      return null;
  }
}

/** A price on the instrument's grid: its tick when known, else its digits. */
function snap(price, tick, digits) {
  const d = Math.max(0, Math.min(10, Math.round(Number(digits) || 0)));
  const f = 10 ** d;
  const onTick = positive(tick) ? Math.round(price / Number(tick)) * Number(tick) : price;
  return Math.round(onTick * f) / f;
}

/** The take profit and stop loss prices of `preset` for an entry on `side` at `ref`.
 *  Answers `{ tp, sl }` (null where the preset has none) or `{ error }` naming what is
 *  missing: 'ref' (no price), 'tick', 'qty' (an amount needs a size), 'side' (an exit on the
 *  wrong side of zero). */
export function exitPrices(preset, { side, ref, tick = null, pointValue = null, qty = null, digits = 2 }) {
  if (!positive(ref)) return { error: 'ref' };
  const dir = side === 'sell' ? -1 : 1;
  const out = { tp: null, sl: null };
  for (const [k, sign] of [
    ['tp', 1],
    ['sl', -1]
  ]) {
    const exit = preset?.[k];
    if (!exit) continue;
    const d = distanceOf(exit, { ref, tick, pointValue, qty });
    if (d == null) return { error: exit.unit === 'ticks' ? 'tick' : exit.unit === 'money' ? 'qty' : 'ref' };
    const p = snap(Number(ref) + dir * sign * d, tick, digits);
    if (!(p > 0)) return { error: 'side' };
    out[k] = p;
  }
  return out;
}
