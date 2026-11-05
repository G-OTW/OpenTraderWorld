/** A script's warmup (spec §4.2): stored bars read back before the first drawn one. */
import { histvizApi } from '$lib/modules/histviz/api.js';

/** Up to `count` stored bars strictly before `beforeTs` for `coords`
 *  (`{provider, connector_id?, asset_type, ticker, timeframe}`): stored only, so a warmup never
 *  spends provider quota, read back in 5000-bar slices. */
export async function fetchWarm(coords, beforeTs, count) {
  const out = { ts: [], o: [], h: [], l: [], c: [], v: [] };
  let to = beforeTs;
  while (out.ts.length < count) {
    const want = Math.min(5000, count - out.ts.length);
    const r = await histvizApi.series({ ...coords, to, bars: want, stored_only: true });
    const got = r.ts?.length ?? 0;
    // The window is in calendar time: a session market or a hole returns fewer bars than
    // asked, so only an empty window (the start of the stored history) ends the warmup.
    if (!got) break;
    for (const k of Object.keys(out)) out[k] = [...(r[k] ?? []), ...out[k]];
    to = r.ts[0];
  }
  return out;
}
