/** Merge a stream candle by timestamp, preserving order across snapshots and catch-up.
 * Mutates existing arrays so chart navigation stays anchored to the same window. */
export function mergeLiveBar(bars, d) {
    if (!bars?.ts?.length) {
      bars = { ...(bars ?? {}), ts: [d.ts], o: [d.o], h: [d.h], l: [d.l], c: [d.c], v: [d.v] };
      return bars;
    }
    const evMs = Date.parse(d.ts);
    const n = bars.ts.length;
    let i = n - 1;
    while (i >= 0 && Date.parse(bars.ts[i]) > evMs) i--;
    if (i >= 0 && Date.parse(bars.ts[i]) === evMs) {
      bars.o[i] = d.o;
      bars.h[i] = d.h;
      bars.l[i] = d.l;
      bars.c[i] = d.c;
      bars.v[i] = d.v;
      return bars;
    }
    const at = i + 1;
    bars.ts.splice(at, 0, d.ts);
    bars.o.splice(at, 0, d.o);
    bars.h.splice(at, 0, d.h);
    bars.l.splice(at, 0, d.l);
    bars.c.splice(at, 0, d.c);
    bars.v.splice(at, 0, d.v);
    return bars;
}

/** Put a series answer's bars into the chart's, only where the chart has none: strictly
 *  between `fromMs` and `toMs`, at timestamps it does not hold yet. A bar already there is the
 *  live feed's, newer than any read, and is left alone. Returns a new series, or `bars` itself
 *  when nothing was added. */
export function fillBars(bars, r, fromMs, toMs) {
  const add = [];
  for (let i = 0; i < (r?.ts?.length ?? 0); i++) {
    const ms = Date.parse(r.ts[i]);
    if (ms > fromMs && ms < toMs) add.push(i);
  }
  if (!add.length) return bars;
  const have = new Set(bars.ts.map((s) => Date.parse(s)));
  const fresh = add.filter((i) => !have.has(Date.parse(r.ts[i])));
  if (!fresh.length) return bars;
  const out = { ...bars, ts: [], o: [], h: [], l: [], c: [], v: [] };
  const push = (src, i) => {
    out.ts.push(src.ts[i]);
    out.o.push(src.o[i]);
    out.h.push(src.h[i]);
    out.l.push(src.l[i]);
    out.c.push(src.c[i]);
    out.v.push(src.v?.[i] ?? 0);
  };
  let a = 0;
  let b = 0;
  while (a < bars.ts.length || b < fresh.length) {
    const takeOld =
      b >= fresh.length ||
      (a < bars.ts.length && Date.parse(bars.ts[a]) < Date.parse(r.ts[fresh[b]]));
    if (takeOld) push(bars, a++);
    else push(r, fresh[b++]);
  }
  out.count = out.ts.length;
  return out;
}

/** The chart's bars before `fromMs`, then `list` (oldest first, same columns as a stream
 *  candle): how a custom timeframe's day is redrawn when its grid moves. Returns a new
 *  series. */
export function replaceTail(bars, fromMs, list) {
  const out = { ...(bars ?? {}), ts: [], o: [], h: [], l: [], c: [], v: [] };
  for (let i = 0; i < (bars?.ts?.length ?? 0); i++) {
    if (Date.parse(bars.ts[i]) >= fromMs) break;
    out.ts.push(bars.ts[i]);
    out.o.push(bars.o[i]);
    out.h.push(bars.h[i]);
    out.l.push(bars.l[i]);
    out.c.push(bars.c[i]);
    out.v.push(bars.v?.[i] ?? 0);
  }
  for (const d of list) {
    out.ts.push(d.ts);
    out.o.push(d.o);
    out.h.push(d.h);
    out.l.push(d.l);
    out.c.push(d.c);
    out.v.push(d.v);
  }
  out.count = out.ts.length;
  return out;
}
