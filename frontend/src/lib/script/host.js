/** What the chart hands the script engine: bar columns, instrument facts, input values.
 *  Pure, so the rules (crypto facts, input refusals) are testable without a worker. */

import { shiftDrawings } from './drawings.js';

/** Bars a script runs before the first bar drawn (spec §4.2); the engine's
 *  `host::FIRST_BAR_WINDOW`, pinned by a test. */
export const SCRIPT_WARMUP_BARS = 5000;

/** Price series an `input.source` may name. */
export const SOURCES = ['open', 'high', 'low', 'close', 'volume', 'hl2', 'hlc3', 'ohlc4', 'hlcc4'];

/** Bars `{ts, o, h, l, c, v}` to Float64Array columns. `volume:false` sends an empty column:
 *  the engine then refuses a script that reads volume instead of reading zeros. */
export function barColumns(view, { volume }) {
  const n = view?.c?.length ?? 0;
  const col = (a) => Float64Array.from({ length: n }, (_, i) => (a?.[i] == null ? NaN : a[i]));
  return {
    time: Float64Array.from({ length: n }, (_, i) => Date.parse(view.ts[i])),
    open: col(view?.o),
    high: col(view?.h),
    low: col(view?.l),
    close: col(view?.c),
    volume: volume ? col(view?.v) : new Float64Array(0)
  };
}

/** Crypto venues trade around the clock and stamp in UTC: a rule, not a guess. Applied only
 *  to fields the stored metadata leaves unset. */
const CRYPTO = { timezone: 'Etc/UTC', session: '24x7', kind: 'crypto' };
const FIELDS = ['mintick', 'pointvalue', 'timezone', 'session', 'kind', 'currency', 'basecurrency', 'description'];

/** `SymInfo` for the engine: ticker from the chart, facts from `meta` (instrument_meta row),
 *  `null` = unknown (a script reading it is refused with the field named). */
export function syminfoFor(coords, meta) {
  const out = {
    ticker: coords?.ticker ?? '',
    tickerid: `${String(coords?.provider ?? '').toUpperCase()}:${String(coords?.ticker ?? '').toUpperCase()}`
  };
  for (const f of FIELDS) out[f] = meta?.[f] ?? null;
  if (coords?.asset_type === 'crypto') for (const [k, v] of Object.entries(CRYPTO)) out[k] ??= v;
  return out;
}

function hexColor(s) {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(String(s ?? '').trim());
  if (!m) return null;
  const x = parseInt(m[1], 16);
  return { r: (x >> 16) & 255, g: (x >> 8) & 255, b: x & 255, a: m[2] ? parseInt(m[2], 16) : 255 };
}

/** One raw form value to the engine's `InputValue`, or the reason it is refused. */
function convert(meta, raw) {
  const inRange = (x) => (meta.minval == null || x >= meta.minval) && (meta.maxval == null || x <= meta.maxval);
  switch (meta.kind) {
    case 'Int': {
      const x = typeof raw === 'number' ? raw : Number(String(raw).trim());
      if (String(raw).trim() === '' || !Number.isInteger(x)) return { reason: 'type' };
      return inRange(x) ? { value: { Int: x } } : { reason: 'range' };
    }
    case 'Float': {
      const x = typeof raw === 'number' ? raw : Number(String(raw).trim());
      if (String(raw).trim() === '' || !Number.isFinite(x)) return { reason: 'type' };
      return inRange(x) ? { value: { Float: x } } : { reason: 'range' };
    }
    case 'Bool':
      return typeof raw === 'boolean' ? { value: { Bool: raw } } : { reason: 'type' };
    case 'Source':
      return SOURCES.includes(raw) ? { value: { Source: raw } } : { reason: 'option' };
    case 'Color': {
      const c = hexColor(raw);
      return c ? { value: { Color: c } } : { reason: 'type' };
    }
    case 'Str':
    case 'Session': {
      if (typeof raw !== 'string') return { reason: 'type' };
      if (meta.options?.length && !meta.options.includes(raw)) return { reason: 'option' };
      return { value: { Str: raw } };
    }
    default:
      return { reason: 'type' };
  }
}

/** Raw form values (keyed by `InputMeta.key`) to `{key: InputValue}`. Keys absent from `raw`
 *  are left out so the script's default applies; a refused value is reported, never clamped. */
export function inputValues(metaInputs, raw) {
  const values = {};
  const errors = [];
  for (const m of metaInputs ?? []) {
    if (raw?.[m.key] === undefined) continue;
    const r = convert(m, raw[m.key]);
    if (r.value) values[m.key] = r.value;
    else errors.push({ key: m.key, reason: r.reason });
  }
  return { values, errors };
}

const hex2 = (x) => Math.round(x).toString(16).padStart(2, '0');

/** `{r,g,b,a}` to `#rrggbb`, or `#rrggbbaa` when not opaque. */
export const rgbaHex = (c) => `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}${c.a === 255 ? '' : hex2(c.a)}`;

/** An input's default (`InputValue`) as the raw form value `inputValues` takes back. */
export function defaultRaw(meta) {
  const [tag, v] = Object.entries(meta?.default ?? {})[0] ?? [];
  return tag === 'Color' ? rgbaHex(v) : v;
}

const HHMM = /^([01]\d|2[0-3])[0-5]\d$/;

/** Same rules as the server's `validate_instrument_meta`: `{field: i18n reason}`, empty when
 *  valid. A null field is unknown, which is allowed. */
export function instrumentErrors(m) {
  const errors = {};
  for (const f of ['mintick', 'pointvalue']) {
    if (m[f] != null && !(Number.isFinite(m[f]) && m[f] > 0)) errors[f] = 'badPositive';
  }
  if (m.timezone != null) {
    try {
      new Intl.DateTimeFormat('en', { timeZone: m.timezone });
    } catch {
      errors.timezone = 'badTz';
    }
  }
  if (m.session != null) {
    const s = String(m.session).trim();
    const part = (p) => {
      const [range, days = '1', ...rest] = p.split(':');
      const [a, b, ...more] = range.split('-');
      return !rest.length && !more.length && /^[1-7]+$/.test(days) && HHMM.test(a ?? '') && HHMM.test(b ?? '');
    };
    if (!(s.toLowerCase() === '24x7' || (s && s.split(',').every(part)))) errors.session = 'badSession';
  }
  return errors;
}

// ── Live sessions (spec §4.2): warmup and rollback planning ─────────────────────────────────

const OHLCV = ['o', 'h', 'l', 'c', 'v'];

/** `warm` bars strictly older than `bars.ts[0]`, then `bars`: what a script runs over so its
 *  first drawn bar already has the history the backtest gives it. */
export function withWarm(warm, bars) {
  const keep = warmKeep(warm, bars?.ts?.[0]);
  if (!keep) return bars;
  const out = { ...bars, ts: [...warm.ts.slice(0, keep), ...bars.ts] };
  for (const k of OHLCV) out[k] = [...warm[k].slice(0, keep), ...(bars[k] ?? [])];
  return out;
}

/** How many of `warm`'s bars are strictly older than `firstTs`. The stamps are sorted, so a
 *  binary search; a live chart asks this on every tick with the same answer, so it is kept
 *  per warmup. */
const keeps = new WeakMap();
function warmKeep(warm, firstTs) {
  const ts = warm?.ts;
  if (!ts?.length) return 0;
  const hit = keeps.get(warm);
  if (hit && hit.first === firstTs) return hit.keep;
  const first = Date.parse(firstTs);
  let lo = 0;
  let hi = ts.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (Date.parse(ts[mid]) < first) lo = mid + 1;
    else hi = mid;
  }
  keeps.set(warm, { first: firstTs, keep: lo });
  return lo;
}

/** Every per-bar column of a run output, as `[holder, key]` pairs (`output.rs each_column`). */
function columns(o) {
  const out = [];
  for (const p of o.plots ?? []) out.push([p, 'values'], [p, 'colors']);
  for (const f of o.fills ?? []) out.push([f, 'colors']);
  for (const b of [...(o.bgcolor ?? []), ...(o.barcolor ?? [])]) out.push([b, 'colors']);
  for (const s of o.shapes ?? []) out.push([s, 'when'], [s, 'color']);
  for (const c of o.candles ?? []) out.push([c, 'o'], [c, 'h'], [c, 'l'], [c, 'c'], [c, 'colors']);
  return out;
}

/** `output` with every per-bar column cut to bars `from..`; the other fields as is. */
export function sliceOutput(output, from) {
  if (!output || !from) return output;
  // A shallow copy of each column holder, never of the columns: cloning the whole output
  // copied every warmup bar of every column on each live tick only to throw them away.
  const each = (list) => list?.map((h) => ({ ...h }));
  const o = {
    ...output,
    plots: each(output.plots),
    fills: each(output.fills),
    bgcolor: each(output.bgcolor),
    barcolor: each(output.barcolor),
    shapes: each(output.shapes),
    candles: each(output.candles)
  };
  for (const [h, k] of columns(o)) h[k] = h[k].slice(from);
  // Drawings placed by bar index count from the first bar run: move them with the cut.
  if (o.drawings) o.drawings = shiftDrawings(o.drawings, from);
  return o;
}

/** `full` with every per-bar column replaced from bar `from` by `tail`'s (a session answer
 *  cut at `from`); the other fields are `tail`'s. */
export function mergeTail(full, from, tail) {
  const o = structuredClone(tail);
  const head = columns(full);
  columns(o).forEach(([h, k], i) => {
    const before = head[i] ? head[i][0][head[i][1]].slice(0, from) : Array(from).fill(null);
    h[k] = [...before, ...h[k]];
  });
  return o;
}

/** At most this many new bars go through a session advance; past it a reopen is cheaper
 *  than the bookkeeping of a long catch-up. */
const ADVANCE_MAX = 50;

/** How to bring a live script up to `next = {sig, ts, bars}` from the last answer
 *  `prev = {sig, ts, committed, lastBar}`: advance the session when only bars from
 *  `committed` on moved, reopen otherwise (another script, inputs or history, a bar
 *  inserted or rewritten before them, a long catch-up). */
export function planRun(prev, next) {
  const k = prev?.committed ?? 0;
  if (!prev || !k || prev.sig !== next.sig || prev.ts[0] !== next.ts[0]) return { op: 'open' };
  // The session holds bars `..prev.ts.length`: a chart that lost one of them reopens.
  if (next.ts.length < prev.ts.length && next.ts.length <= k) return { op: 'open' };
  if (next.ts[k - 1] !== prev.ts[k - 1]) return { op: 'open' };
  if (OHLCV.some((f) => next.bars[f]?.[k - 1] !== prev.lastBar?.[f])) return { op: 'open' };
  if (next.ts.length - k > ADVANCE_MAX) return { op: 'open' };
  return { op: 'advance', from: k };
}

/** Whether the bar opened at `lastTs` is still forming at `now` (its close is later). */
export const isForming = (lastTs, tfSecs, now) => Date.parse(lastTs) + tfSecs * 1000 > now;
