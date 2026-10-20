/** Main-thread side of the script worker. One shared worker; runs are coalesced per key:
 *  at most one in flight and one pending, a newer request replaces the pending one (whose
 *  promise resolves `{stale:true}`), so a burst of live ticks never queues stale work. */

let factory = () => new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
let worker = null;
let nextId = 1;
const waiting = new Map(); // id → resolve
const lanes = new Map(); // key → { busy, pending: {req, resolve} | null }

/** Tests swap the worker for a fake. */
export function __setWorkerFactory(f) {
  factory = f;
  worker = null;
  waiting.clear();
  lanes.clear();
}

function post(msg, transfer = []) {
  if (!worker) {
    worker = factory();
    worker.onmessage = (e) => {
      const resolve = waiting.get(e.data.id);
      waiting.delete(e.data.id);
      resolve?.(e.data.result);
    };
  }
  const id = nextId++;
  return new Promise((resolve) => {
    waiting.set(id, resolve);
    worker.postMessage({ id, ...msg }, transfer);
  });
}

/** `{ok:true, meta, warnings}` or `{ok:false, diagnostics}` (the engine's compile JSON). */
export const compileScript = (source) => post({ op: 'compile', source });

let docs = null;
/** Every builtin (`BuiltinDoc[]`), fetched once. */
export const registryDocs = () => (docs ??= post({ op: 'registry' }));

/** Run `req = {source, cols, ctx}` on lane `key`. Resolves the run JSON or `{stale:true}`. */
export function runScript(key, req) {
  let lane = lanes.get(key);
  if (!lane) lanes.set(key, (lane = { busy: false, pending: null }));
  return new Promise((resolve) => {
    if (!lane.busy) return start(lane, req, resolve);
    lane.pending?.resolve({ stale: true });
    lane.pending = { req, resolve };
  });
}

/** Open the live session of lane `key`: `req = {source, cols, ctx, forming}`. Resolves
 *  `{ok, from: 0, committed, output}` (or a refusal) or `{stale:true}`. */
export const openSession = (key, req) => runScript(key, { ...req, op: 'open', key });

/** Run `req = {cols, forming, from}` on lane `key`'s session: `cols` are the bars from `from`,
 *  the session's committed count. `{ok:false, context:'stale session'}` asks for a reopen. */
export const advanceSession = (key, req) => runScript(key, { ...req, op: 'advance', key });

/** Free lane `key`'s session (the instance is gone). */
export function closeSession(key) {
  if (worker) post({ op: 'close', key });
}

function start(lane, req, resolve) {
  lane.busy = true;
  // The columns are built per run: hand them over instead of copying them.
  const transfer = Object.values(req.cols ?? {}).map((a) => a?.buffer).filter(Boolean);
  post({ op: 'run', ...req }, transfer).then((result) => {
    resolve(result);
    const next = lane.pending;
    lane.pending = null;
    lane.busy = false;
    if (next) queueMicrotask(() => start(lane, next.req, next.resolve));
  });
}
