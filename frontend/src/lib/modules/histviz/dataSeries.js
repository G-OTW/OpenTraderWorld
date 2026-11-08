/** Data series on the chart: a Fundamentals series (an economic release, a custom value
 *  source) drawn as a step line, and read by the custom-indicator `series` node.
 *
 *  An instance is `{ type: 'data', pane: 'pane'|'overlay', data: { id, title, code, unit, at } }`.
 *  `at` places each observation: `released` (default) at its release time when the source
 *  gives one, else at the end of its period; `period` always at the period start. The chart's
 *  timeframe plays no part: each bar reads the last value placed at or before its stamp, held
 *  until the next one, never interpolated (`stepFill`, the same rule as the engine's
 *  `align::fill_points`).
 *
 *  Observations are `[period_start_ms, known_ms|null, value]`, the shape `evalDef` reads:
 *  `known_ms` is the server's `known` (release, else period end).
 */
import { placeObservation, stepFill } from '$lib/indicators/evaluate.js';

export const DATA_TYPE = 'data';
export const isData = (inst) => inst?.type === DATA_TYPE;

async function req(path) {
  const res = await fetch(`/api/fundamentals${path}`);
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* no JSON body */
  }
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

/** Every stored Fundamentals series, for the picker. */
export async function listSeries() {
  const { series } = await req('/series');
  return (series ?? []).map((s) => ({
    id: s.id,
    title: s.title,
    code: s.code,
    unit: s.unit ?? '',
    freq: s.freq ?? '',
    provider: s.provider ?? ''
  }));
}

/** A series' observations with the instant each can be read. */
export async function loadObservations(id) {
  const body = await req(`/series/${encodeURIComponent(id)}/observations`);
  const known = body.known ?? [];
  return (body.observations ?? []).map(([t, v], i) => [t, known[i] ?? null, v]);
}

/** Filter the picker by title or code, case-insensitive. */
export function searchSeries(list, q) {
  const s = (q ?? '').trim().toLowerCase();
  if (!s) return list;
  return list.filter((r) => r.title.toLowerCase().includes(s) || r.code.toLowerCase().includes(s));
}

/** Merge one live `point` event into a series' observations: a revision of a period replaces
 *  it (latest vintage wins), a new period is inserted in order. A point that changes nothing
 *  returns the same array, so a caller can skip a redraw. */
export function upsertPoint(obs, p) {
  const row = [p.period_start, p.known_at ?? p.released_at ?? null, p.value];
  const list = obs ?? [];
  const at = list.findIndex((o) => o[0] === row[0]);
  if (at >= 0) {
    const cur = list[at];
    const released = row[1] ?? cur[1];
    if (cur[2] === row[2] && cur[1] === released) return list;
    const out = [...list];
    out[at] = [row[0], released, row[2]];
    return out;
  }
  const out = [...list, row];
  out.sort((a, b) => a[0] - b[0]);
  return out;
}

/** The series ids a chart needs loaded: its data panes and every `series` node of its custom
 *  indicators. */
export function seriesIdsOf(instances) {
  const ids = new Set();
  for (const i of instances ?? []) {
    if (isData(i) && i.data?.id) ids.add(i.data.id);
    if (i?.type === 'custom') for (const n of i.custom?.def?.nodes ?? []) if (n?.op === 'series' && n.series) ids.add(n.series);
  }
  return [...ids];
}

/** Bar stamps as epoch ms. */
export function barTimes(bars) {
  return (bars?.ts ?? []).map((s) => (typeof s === 'number' ? s : Date.parse(s)));
}

/** A data instance's outputs in the chart's shape: one step line on its own scale. */
export function dataOutputs(inst, bars, obsById) {
  const obs = obsById?.[inst.data?.id];
  const n = bars?.c?.length ?? 0;
  const t = bars?.t ?? barTimes(bars);
  const data = obs ? stepFill(t, obs.map((o) => [placeObservation(o, inst.data?.at), o[2]])) : new Array(n).fill(null);
  return [{ name: inst.data?.title || inst.data?.code || 'data', kind: 'line', step: true, data, color: 'accent' }];
}

// One stream for every series on screen, whichever charts watch them: a browser holds six
// connections per host over HTTP/1.1, and the bar streams need theirs.
const watchers = new Map(); // series id -> Set of callbacks
let source = null;
let sourceKey = '';
let reopenQueued = false;

function reopen() {
  reopenQueued = false;
  const ids = [...watchers.keys()].sort();
  const key = ids.join(',');
  if (key === sourceKey) return;
  source?.close();
  source = null;
  sourceKey = key;
  if (!ids.length || typeof EventSource === 'undefined') return;
  source = new EventSource(`/api/fundamentals/stream?series=${encodeURIComponent(key)}`);
  source.addEventListener('point', (e) => {
    let p;
    try {
      p = JSON.parse(e.data);
    } catch {
      return; // a malformed frame is skipped
    }
    for (const cb of watchers.get(p.series_id) ?? []) cb(p);
  });
}

function queueReopen() {
  if (reopenQueued) return;
  reopenQueued = true;
  queueMicrotask(reopen);
}

/** A live subscription to a series' new and revised observations (`point` events). Returns
 *  the closer. */
export function streamPoints(id, onPoint) {
  if (!watchers.has(id)) watchers.set(id, new Set());
  watchers.get(id).add(onPoint);
  queueReopen();
  return () => {
    const set = watchers.get(id);
    set?.delete(onPoint);
    if (set && !set.size) watchers.delete(id);
    queueReopen();
  };
}
