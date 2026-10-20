/** Script runs of one chart pane: whenever a script instance, its inputs, the bars, the chart
 *  type or the instrument facts change, every script instance reruns in the worker (250 ms
 *  after the last change). Call during component init: it owns an `$effect`.
 *
 *  Chart-type rule (spec §4.5): time-based types (candles, bars, line, Heikin Ashi, …) run
 *  on the real OHLCV, so a Heikin Ashi chart shows what the backtest computes; Renko, Kagi,
 *  P&F, range and line break have no time axis and run on the displayed bricks, which carry
 *  no volume.
 *
 *  Realtime model (spec §4.2), time types only: a script first runs over up to
 *  `SCRIPT_WARMUP_BARS` stored bars before the first drawn one (the backtest's first-bar
 *  rule), then lives in a worker session: a tick sends only the bars from the last committed
 *  one, the forming bar rolls back and reruns, and anything else reopens the session. Bricks
 *  rerun whole, without warmup. */
import { untrack } from 'svelte';
import { compileScript, runScript, openSession, advanceSession, closeSession } from './client.js';
import {
  barColumns,
  syminfoFor,
  inputValues,
  withWarm,
  sliceOutput,
  mergeTail,
  planRun,
  isForming,
  SCRIPT_WARMUP_BARS
} from './host.js';
import { chartView, isScript } from '$lib/modules/histviz/indicators.js';

export const BRICK_TYPES = ['renko', 'kagi', 'pnf', 'range', 'linebreak'];
const DEBOUNCE_MS = 250;
const OHLCV = ['o', 'h', 'l', 'c', 'v'];

/** Bars `from..` of `bars`. */
function tailOf(bars, from) {
  const out = { ts: bars.ts.slice(from) };
  for (const k of OHLCV) out[k] = bars[k]?.slice(from) ?? [];
  return out;
}

/** `get = { instances, bars, type, brick, coords, meta, tfSecs, fetchWarm, serverRun? }`, each
 *  a function read inside the effect but `fetchWarm(coords, beforeTs, count) -> Promise<bars>`,
 *  which loads stored bars strictly before `beforeTs`, and `serverRun(body) -> [{key, answer}]`
 *  (`scriptServer.run`): when given, a time-based chart whose last bar is closed runs on the
 *  server, which reads the warmup itself and caches the answer (the worker when it cannot). `results` maps an instance id to
 *  `{ meta, output, warnings, error, diagnostics, inputErrors, onBricks }`. */
export function createScriptRuns(get) {
  const state = $state({ results: new Map() });
  /** The last session answer per instance: `{ sig, ts, committed, lastBar, full }`. */
  const live = new Map();
  /** Warmup bars per instrument and first drawn bar, loaded once. */
  const warms = new Map();

  /** The server's answer for one script over `view`, or null when the server cannot give one
   *  (unreachable, a bar the JSON cannot carry): the worker then runs it. */
  async function serverAnswer(inst, view, coords, ctx) {
    if (!get.serverRun) return null;
    const cols = barColumns(view, { volume: true });
    const prices = [cols.open, cols.high, cols.low, cols.close, cols.time];
    if (!prices.every((a) => a.every(Number.isFinite))) return null;
    try {
      const [r] = await get.serverRun({
        provider: coords?.provider ?? '',
        asset_type: coords?.asset_type ?? '',
        ticker: coords?.ticker ?? '',
        timeframe: coords?.timeframe ?? '',
        time: Array.from(cols.time),
        open: Array.from(cols.open),
        high: Array.from(cols.high),
        low: Array.from(cols.low),
        close: Array.from(cols.close),
        volume: cols.volume.every(Number.isFinite) ? Array.from(cols.volume) : [],
        scripts: [{ key: String(inst.id), source: inst.script.source, ctx }]
      });
      const a = r?.answer;
      // A refused context (or bars) is the worker's to explain: it gives the same answer.
      return a && (a.ok || a.error) ? a : null;
    } catch {
      return null;
    }
  }

  function warmFor(coords, firstTs) {
    if (!get.fetchWarm || !firstTs) return Promise.resolve(null);
    const key = JSON.stringify([
      coords?.provider,
      coords?.connector_id,
      coords?.asset_type,
      coords?.ticker,
      coords?.timeframe,
      firstTs
    ]);
    let p = warms.get(key);
    if (!p) {
      p = get.fetchWarm(coords, firstTs, SCRIPT_WARMUP_BARS).catch(() => null);
      warms.set(key, p);
      // A chart scrolled back many times would keep every window: keep the recent ones.
      if (warms.size > 8) warms.delete(warms.keys().next().value);
    }
    return p;
  }

  function forget(id) {
    if (live.delete(id)) closeSession(`inst:${id}`);
  }

  /** Bring instance `id`'s session up to `all`; the answer's output covers every bar of `all`. */
  async function runLive(id, source, all, ctx, sig, forming) {
    const key = `inst:${id}`;
    const prev = live.get(id);
    const plan = planRun(prev, { sig, ts: all.ts, bars: all });
    let res =
      plan.op === 'advance'
        ? await advanceSession(key, {
            cols: barColumns(tailOf(all, plan.from), { volume: true }),
            forming,
            from: plan.from
          })
        : await openSession(key, {
            source,
            cols: barColumns(all, { volume: true }),
            ctx,
            forming
          });
    if (res.stale) return res;
    let opened = plan.op === 'open';
    if (!res.ok && res.context === 'stale session') {
      res = await openSession(key, {
        source,
        cols: barColumns(all, { volume: true }),
        ctx,
        forming
      });
      if (res.stale) return res;
      opened = true;
    }
    if (!res.ok) {
      // A failed open leaves no session; a failed advance leaves the last one as it was.
      if (opened) live.delete(id);
      return res;
    }
    const full = opened ? res.output : mergeTail(prev.full, res.from, res.output);
    const k = res.committed;
    const lastBar = Object.fromEntries(OHLCV.map((f) => [f, all[f][k - 1]]));
    // `mergeLiveBar` edits the chart's arrays in place: keep our own copy of the stamps.
    live.set(id, { sig, ts: all.ts.slice(), committed: k, lastBar, full });
    return { ok: true, output: full };
  }

  async function runOne(inst, bars, type, brick, coords, meta, tfSecs) {
    const onBricks = BRICK_TYPES.includes(type);
    const comp = await compileScript(inst.script.source);
    if (!comp.ok) return { diagnostics: comp.diagnostics ?? [], warnings: [], onBricks };
    const { values, errors } = inputValues(comp.meta.inputs, inst.script.inputs ?? {});
    const base = {
      meta: comp.meta,
      warnings: comp.warnings ?? [],
      onBricks,
      inputErrors: errors
    };
    if (errors.length) return base;
    const view = onBricks ? chartView(type, bars, brick) : bars;
    if (!view?.c?.length) return { ...base, output: null };
    const ctx = JSON.stringify({
      syminfo: syminfoFor(coords, meta),
      timeframe: coords?.timeframe ?? '',
      inputs: values
    });
    let res;
    let warmCount = 0;
    if (onBricks) {
      forget(inst.id);
      res = await runScript(`inst:${inst.id}`, {
        source: inst.script.source,
        cols: barColumns(view, { volume: false }),
        ctx
      });
    } else {
      // A chart that is not forming a bar runs on the server, which reads the warmup itself
      // and keeps the answer. A forming one needs the worker's session for its ticks anyway,
      // and running it twice only delays it.
      if (get.serverRun && !isForming(view.ts.at(-1), tfSecs, Date.now())) {
        forget(inst.id);
        const a = await serverAnswer(inst, view, coords, ctx);
        if (a?.ok) return { ...base, output: a.output };
        if (a) return { ...base, error: a.error };
      }
      const warm = await warmFor(coords, view.ts[0]);
      const all = withWarm(warm, view);
      warmCount = all.ts.length - view.ts.length;
      const sig = JSON.stringify([inst.script.source, ctx, all.ts[0], type]);
      res = await runLive(inst.id, inst.script.source, all, ctx, sig, isForming(all.ts.at(-1), tfSecs, Date.now()));
    }
    if (res.stale) return null;
    if (res.ok) return { ...base, output: sliceOutput(res.output, warmCount) };
    return {
      ...base,
      error: res.error ?? {
        code: 'BadContext',
        args: [res.context ?? ''],
        span: null,
        bar: 0
      }
    };
  }

  $effect(() => {
    const instances = get.instances().filter(isScript);
    const bars = get.bars();
    const type = get.type();
    const brick = get.brick();
    const coords = get.coords();
    const meta = get.meta();
    const tfSecs = get.tfSecs?.() ?? 3600;
    // Track what a run depends on: the scripts themselves and the newest bar (live ticks
    // replace the last bar in place).
    const n = bars?.c?.length ?? 0;
    void [n, bars?.c?.[n - 1], bars?.ts?.[n - 1], JSON.stringify(instances.map((i) => i.script))];
    // Nothing to run on yet: the first run waits for the bars rather than running twice.
    if (!n && instances.length) return;
    const snapshot = $state.snapshot({ instances, coords, meta });
    // The first run of a script goes at once; later changes wait for the user to settle.
    // Read untracked: the run publishes into `state.results`, which must not rerun it.
    const fresh = untrack(() => snapshot.instances.some((i) => !state.results.has(i.id)));
    const timer = setTimeout(() => request({ snapshot, bars, type, brick, tfSecs }), fresh ? 0 : DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  // One cycle at a time: changes that land while one runs (bars, then the instrument facts,
  // then the library refresh on load) wait for it and run once, with the latest of them.
  let running = false;
  let queued = null;
  async function request(job) {
    if (running) {
      queued = job;
      return;
    }
    running = true;
    try {
      await cycle(job);
    } finally {
      running = false;
      if (queued) {
        const next = queued;
        queued = null;
        request(next);
      }
    }
  }

  async function cycle({ snapshot, bars, type, brick, tfSecs }) {
    const ids = new Set(snapshot.instances.map((i) => i.id));
    for (const id of [...live.keys()]) if (!ids.has(id)) forget(id);
    // Each script shows as soon as it has run, not when the slowest one has.
    state.results = new Map([...untrack(() => state.results)].filter(([id]) => ids.has(id)));
    for (const inst of snapshot.instances) {
      const r = await runOne(inst, bars, type, brick, snapshot.coords, snapshot.meta, tfSecs);
      if (r) state.results = new Map(untrack(() => state.results)).set(inst.id, r);
    }
  }

  // The pane is gone: free its sessions in the worker.
  $effect(() => () => {
    for (const id of [...live.keys()]) forget(id);
  });

  return state;
}
