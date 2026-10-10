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
