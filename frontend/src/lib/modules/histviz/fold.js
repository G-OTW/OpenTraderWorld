/** Live candles of a custom timeframe, folded in the browser from the base timeframe the feed
 *  streams (a 7m chart streams 1m and folds it here).
 *
 *  The grid is the server's (`consolidate` in `histdata_api.rs`), so a live candle and the
 *  history it continues carry the same timestamps:
 *
 *  - under a day, each UTC day's grid starts at that day's **first base bar** (09:30 New York
 *    for a stock, 00:00 UTC for a 24/7 market), and the day's last candle may be short;
 *  - a day and longer use the plain period grid (`Timeframe::bucket`): the epoch, weeks on
 *    the ISO Monday.
 *
 *  Base bars are held by timestamp, so a repeat (the snapshot, a catch-up, the server's
 *  backfill arriving late) replaces rather than adds, and every candle is recomputed from the
 *  base bars it covers. Nothing here is stored: the feed records the base bars. */

import { tfSeconds } from './pane.js';

const DAY = 86_400_000;
/** 1970-01-05, the first ISO Monday after the epoch (the Rust `FIRST_MONDAY`). */
const FIRST_MONDAY = 4 * DAY;

/** A series answer's `{ts, o, h, l, c, v}` columns as rows. */
const rowsOf = (p) =>
  (p?.ts ?? []).map((ts, i) => ({ ms: Date.parse(ts), o: p.o[i], h: p.h[i], l: p.l[i], c: p.c[i], v: p.v?.[i] ?? 0 }));

/**
 * @param {string} tf the custom timeframe on screen (`7m`, `3h`, `2d`, `2w`)
 * @param {string} base the served timeframe it is consolidated from (`consolidated_from`)
 * @param {object} [tail] the base bars of the newest candle (`consolidated_tail`)
 * @param {string} [start] that candle's own timestamp: the grid point, which its first base
 *   bar is not when the period opened quiet
 */
export function createFold(tf, base, tail, start) {
  const step = tfSeconds(tf) * 1000;
  const baseStep = tfSeconds(base) * 1000;
  const intraday = step < DAY;
  const weeks = /w$/.test(tf);
  /** Start of the stretch the held bars belong to: the UTC day, or the period itself. */
  const segOf = (ms) => {
    if (intraday) return Math.floor(ms / DAY) * DAY;
    const off = weeks ? FIRST_MONDAY : 0;
    return Math.floor((ms - off) / step) * step + off;
  };

  let seg = null;
  /** Oldest base bar this fold may hold: the newest history candle, or the segment's start. */
  let horizon = null;
  /** Intraday grid origin, and whether the server gave it (otherwise it is the day's first
   *  bar this fold has seen, and moves back if an older one arrives). */
  let anchor = null;
  let fixed = false;
  /** Base bars of the segment, by timestamp. */
  let held = new Map();
  let lastMs = null;
  /** The segment already handed back for a server read: once per day, never a loop. */
  let asked = null;

  const startOf = (ms) => {
    if (!intraday) return seg;
    return ms >= anchor ? anchor + Math.floor((ms - anchor) / step) * step : Math.floor(ms / step) * step;
  };

  /** Every candle the held bars make, oldest first. */
  function candles() {
    const out = [];
    for (const ms of [...held.keys()].sort((a, b) => a - b)) {
      const b = held.get(ms);
      const at = startOf(ms);
      const cur = out[out.length - 1];
      if (cur && cur.ms === at) {
        cur.h = Math.max(cur.h, b.h);
        cur.l = Math.min(cur.l, b.l);
        cur.c = b.c;
        cur.v += b.v;
      } else out.push({ ms: at, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v });
    }
    return out.map(({ ms, ...k }) => ({ ts: new Date(ms).toISOString(), ...k }));
  }

  /** Start again from a series answer's newest candle (`tail` its base bars, `startTs` its
   *  timestamp). Live bars newer than it are kept: they arrived while the read was out.
   *  Returns false when the answer is older than what the fold already holds (the server has
   *  nothing of today yet). */
  function seed(p, startTs) {
    const rows = rowsOf(p);
    if (!rows.length) return false;
    const first = Number.isFinite(Date.parse(startTs)) ? Date.parse(startTs) : rows[0].ms;
    const s = segOf(first);
    if (seg != null && s < seg) return false;
    const newest = rows[rows.length - 1].ms;
    const keep = s === seg ? [...held.values()].filter((b) => b.ms > newest) : [];
    seg = s;
    horizon = intraday ? first : s;
    anchor = intraday ? first : null;
    fixed = true;
    held = new Map([...rows, ...keep].map((b) => [b.ms, b]));
    lastMs = Math.max(newest, lastMs ?? -Infinity);
    return true;
  }

  /**
   * Fold one base bar from the feed.
   * @returns {null | {bars: object[], from: number|null, resync: number|null}} `bars` are
   *   the candles to put on the chart; `from` set means every chart candle from that instant
   *   on is replaced by `bars` (the grid moved), otherwise `bars` is the one candle the base
   *   bar landed in. `resync` is a day whose first bar the fold may have missed: read it back
   *   from the server and `seed` the answer.
   */
  function push(d) {
    const ms = Date.parse(d.ts);
    if (!Number.isFinite(ms)) return null;
    const bar = { ms, o: d.o, h: d.h, l: d.l, c: d.c, v: d.v ?? 0 };
    const s = segOf(ms);
    if (seg != null && s < seg) return null;
    let from = null;
    let resync = null;
    if (seg == null || s > seg) {
      // A new day opens the grid at its first bar. Contiguous with the last one (a 24/7
      // market at midnight) it is the first; after a gap (a night, a sleeping tab) the server
      // says whether bars came before it.
      if (intraday && (lastMs == null || ms - lastMs !== baseStep) && asked !== s) resync = asked = s;
      seg = s;
      horizon = s;
      anchor = ms;
      fixed = false;
      held = new Map();
      from = s;
    } else if (ms < horizon) return null;
    held.set(ms, bar);
    if (ms > (lastMs ?? -Infinity)) lastMs = ms;
    if (intraday && !fixed && ms < anchor) {
      anchor = ms;
      from = seg;
    }
    const all = candles();
    if (from != null) return { bars: all, from, resync };
    const at = startOf(ms);
    const one = all.find((c) => Date.parse(c.ts) === at);
    return one ? { bars: [one], from: null, resync } : null;
  }

  /** The newest base bar held, for the stream's catch-up (`since`). */
  const since = () => (lastMs == null ? null : new Date(lastMs).toISOString());

  if (tail) seed(tail, start);
  return { push, seed, candles, since, segOf };
}
