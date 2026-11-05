/** Script engine worker: the WASM build of `otw-script`, off the main thread so a long run
 *  never freezes the chart. Messages: compile | registry | run | open | advance | close (see
 *  client.js). */
import init, { compile, registry, Script, Session } from './wasm/otw_script_wasm.js';

const ready = init();
/** Compiled scripts by source, oldest freed first. */
const cache = new Map();
const CACHE_MAX = 8;
/** Live sessions by lane key (one per chart script instance). */
const sessions = new Map();

function closeSession(key) {
  sessions.get(key)?.free();
  sessions.delete(key);
}

const colsOf = (c) => [c.time, c.open, c.high, c.low, c.close, c.volume];

function script(source) {
  let s = cache.get(source);
  if (s) {
    cache.delete(source);
    cache.set(source, s);
    return s;
  }
  s = new Script(source);
  cache.set(source, s);
  if (cache.size > CACHE_MAX) {
    const [oldest, old] = cache.entries().next().value;
    cache.delete(oldest);
    old.free();
  }
  return s;
}

function handle(m) {
  switch (m.op) {
    case 'compile':
      return JSON.parse(compile(m.source));
    case 'registry':
      return JSON.parse(registry());
    case 'run': {
      let s;
      try {
        s = script(m.source);
      } catch (e) {
        // The constructor throws the compile JSON when the source has errors.
        return JSON.parse(String(e));
      }
      const c = m.cols;
      return JSON.parse(s.run(c.time, c.open, c.high, c.low, c.close, c.volume, m.ctx));
    }
    case 'open': {
      closeSession(m.key);
      let s;
      try {
        s = new Session(m.source, ...colsOf(m.cols), m.ctx, !!m.forming);
      } catch (e) {
        return JSON.parse(String(e));
      }
      sessions.set(m.key, s);
      return JSON.parse(s.output(0));
    }
    case 'advance': {
      const s = sessions.get(m.key);
      // The caller planned against a state this session no longer has: it must reopen.
      if (!s || s.committed() !== m.from) return { ok: false, context: 'stale session' };
      return JSON.parse(s.advance(...colsOf(m.cols), !!m.forming));
    }
    case 'close':
      closeSession(m.key);
      return { ok: true };
    default:
      return { ok: false, context: `unknown op ${m.op}` };
  }
}

self.onmessage = async (e) => {
  await ready;
  let result;
  try {
    result = handle(e.data);
  } catch (err) {
    result = { ok: false, context: String(err?.message ?? err) };
  }
  self.postMessage({ id: e.data.id, result });
};
