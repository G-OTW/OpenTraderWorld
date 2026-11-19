/** Series a file opened on the chart without keeping it.
 *
 * The bars live in this tab's memory and nowhere else: the server built them from the file
 * (`/histdata/import/series`) and stored nothing. `histvizApi.series` answers these
 * coordinates from here, in the same payload and the same window rules as an imported
 * dataset (the newest `bars` before `to`, counted in bars), so the pane, the indicators and
 * script warmups cannot tell the difference. A reload forgets them: the file is imported
 * again, or kept in Historical Data. */

/** The provider id a temporary series is filed under. Never sent to the server. */
export const TEMP_PROVIDER = 'file';

const store = new Map(); // key → { series, source }
const keyOf = (c) => `${c.asset_type}|${c.ticker}|${c.timeframe}`;

export const isTemp = (coords) => coords?.provider === TEMP_PROVIDER;

/** Hold a file's bars (`{ts, o, h, l, c, v}`) under its coordinates, with the import
 *  request that built them (`source`), so the chart can still keep the file later. */
export function putTemp(coords, series, source = null) {
  store.set(keyOf(coords), { series, source });
}

/** The import request a temporary series came from, and forget the series: once kept, the
 *  same coordinates are served by the stored dataset. */
export function takeTempSource(coords) {
  const e = store.get(keyOf(coords));
  if (!e?.source) throw new Error(`${coords.ticker} ${coords.timeframe}: import the file again to keep it`);
  return e.source;
}

export function dropTemp(coords) {
  store.delete(keyOf(coords));
}

/** One window of a temporary series, shaped like `/histviz/series`. Throws when this tab no
 *  longer holds it (a reload, a restored layout) or when another timeframe is asked. */
export function tempWindow(coords) {
  const s = store.get(keyOf(coords))?.series;
  if (!s) {
    throw new Error(
      `${coords.ticker} ${coords.timeframe} was opened from a file without being kept: import the file again`
    );
  }
  const toMs = coords.to ? Date.parse(coords.to) : Infinity;
  let end = s.ts.length;
  while (end > 0 && Date.parse(s.ts[end - 1]) >= toMs) end--;
  const want = Math.max(1, coords.bars ?? 1500);
  const start = Math.max(0, end - want);
  const cut = (a) => a.slice(start, end);
  const ts = cut(s.ts);
  return {
    count: ts.length,
    ts,
    o: cut(s.o),
    h: cut(s.h),
    l: cut(s.l),
    c: cut(s.c),
    v: cut(s.v),
    provider: TEMP_PROVIDER,
    connector_id: null,
    asset_type: coords.asset_type,
    ticker: coords.ticker,
    timeframe: coords.timeframe,
    from: ts[0] ?? null,
    to: coords.to ?? null,
    from_store: ts.length,
    from_provider: 0,
    history_start: start === 0,
    stream: false,
    stream_timeframes: [],
    stream_note: null,
    stored: null,
    notice: null,
    holes: []
  };
}

/** A batch of windows in the order asked: temporary series from this tab, the rest through
 *  `remote` (one server round trip, skipped when every pane is a file). A temporary series
 *  this tab no longer holds carries `error`, like a pane the server could not serve. */
export async function batchWithTemp(items, remote) {
  const rest = items.filter((c) => !isTemp(c));
  const results = rest.length ? await remote(rest) : [];
  let k = 0;
  return items.map((c) => {
    if (!isTemp(c)) return results[k++];
    try {
      return tempWindow(c);
    } catch (e) {
      return { error: e.message };
    }
  });
}
