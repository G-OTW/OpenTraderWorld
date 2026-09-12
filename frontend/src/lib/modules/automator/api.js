/** Automator API client, plus the graph model the editor works on.
 *
 * The graph is `{ nodes: [], edges: [] }`. A node is `{ id, kind, name, config, pos,
 * on_error, retry, timeout_secs }` and an edge is `{ from, to, port }`.
 *
 * The editor is a grid, not a free canvas: `pos` holds a row and a column, rows run top to
 * bottom and the tasks inside a row run left to right, so a workflow is one chain. The
 * links are **derived** from that order by `fromRows` and never drawn by hand. The backend
 * validates the same shape on save, so anything this file lets through is refused there
 * with a message worth showing. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

export const automatorApi = {
  /** → { workflows, schedules, last_runs } */
  list: () => req('/automator/workflows'),
  create: (input) =>
    req('/automator/workflows', { method: 'POST', body: JSON.stringify(input) }).then(
      (r) => r.workflow
    ),
  /** → { workflow, schedules, runs, tokens } */
  get: (id) => req(`/automator/workflows/${id}`),
  /** Save the graph. `draft: true` (autosave) parks a graph that does not validate yet
   *  instead of refusing it, and the reply says so: `{ workflow, draft: true, reason }`. */
  saveGraph: (id, graph, { note = '', repointSchedules = false, draft = false } = {}) =>
    req(`/automator/workflows/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ graph, note, repoint_schedules: repointSchedules, draft })
    }),
  patch: (id, input) =>
    req(`/automator/workflows/${id}`, { method: 'PATCH', body: JSON.stringify(input) }).then(
      (r) => r.workflow
    ),
  remove: (id) => req(`/automator/workflows/${id}`, { method: 'DELETE' }),
  versions: (id) => req(`/automator/workflows/${id}/versions`).then((r) => r.versions),
  rollback: (id, versionId) =>
    req(`/automator/workflows/${id}/rollback`, {
      method: 'POST',
      body: JSON.stringify({ version_id: versionId })
    }),
  /** Start a live run. Rejects with "already running" while one is in flight. */
  run: (id, input = {}) =>
    req(`/automator/workflows/${id}/run`, { method: 'POST', body: JSON.stringify({ input }) }).then(
      (r) => r.run
    ),
  /** Run every block in test mode and get the whole trace back. */
  test: (id, opts = {}) =>
    req(`/automator/workflows/${id}/test`, { method: 'POST', body: JSON.stringify(opts) }),
  /** Run one block on its own, from the block editor. */
  testNode: (body) =>
    req('/automator/nodes/test', { method: 'POST', body: JSON.stringify(body) }),

  /** The block palette: native kinds + every internal endpoint a workflow may call. */
  catalog: () => req('/automator/catalog'),
  /** One endpoint's request-body schema (fetched when a block is opened, not up front). */
  schema: (method, path) =>
    req(`/automator/catalog/schema?method=${encodeURIComponent(method)}&path=${encodeURIComponent(path)}`),
  timezones: () => req('/automator/timezones').then((r) => r.timezones),

  schedules: (workflow) =>
    req(`/automator/schedules${workflow ? `?workflow=${workflow}` : ''}`),
  createSchedule: (input) =>
    req('/automator/schedules', { method: 'POST', body: JSON.stringify(input) }).then(
      (r) => r.schedule
    ),
  updateSchedule: (id, input) =>
    req(`/automator/schedules/${id}`, { method: 'PATCH', body: JSON.stringify(input) }).then(
      (r) => r.schedule
    ),
  deleteSchedule: (id) => req(`/automator/schedules/${id}`, { method: 'DELETE' }),
  agenda: (from, to, workflow) =>
    req(
      `/automator/agenda?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}${
        workflow ? `&workflow=${workflow}` : ''
      }`
    ),

  runs: (params = {}) => {
    const q = new URLSearchParams();
    if (params.workflow) q.set('workflow', params.workflow);
    if (params.status) q.set('status', params.status);
    if (params.limit) q.set('limit', String(params.limit));
    const qs = q.toString();
    return req(`/automator/runs${qs ? `?${qs}` : ''}`);
  },
  runDetail: (id) => req(`/automator/runs/${id}`),
  cancel: (id) => req(`/automator/runs/${id}/cancel`, { method: 'POST' }),
  blobUrl: (runId, blobId) => `/api/automator/runs/${runId}/blob/${blobId}`
};

// ── Module navigation ────────────────────────────────────────────────────────

export const NAV_LINKS = [
  { href: '/automator', icon: 'zap', key: 'automator.nav.workflows' },
  { href: '/automator/schedule', icon: 'clock', key: 'automator.nav.schedules' },
  { href: '/automator/agenda', icon: 'calendar', key: 'automator.nav.agenda' },
  { href: '/automator/runs', icon: 'list', key: 'automator.nav.runs' }
];

// ── Block model ──────────────────────────────────────────────────────────────

/** One entry per node kind: its icon, and the config a fresh block opens with. */
export const KIND_META = {
  api: { icon: 'plug', defaults: () => ({ method: 'GET', path: '', body: null }) },
  http: {
    icon: 'globe',
    defaults: () => ({
      method: 'GET',
      url: '',
      headers: [],
      query: [],
      body_kind: 'none',
      body: '',
      parse: 'json',
      allow_internal: false,
      max_bytes: null
    })
  },
  agent: {
    icon: 'sparkles',
    defaults: () => ({ agent_id: '', prompt_id: '', system: '', input: '', output: 'text', schema: '' })
  },
  notify: { icon: 'bell', defaults: () => ({ channels: [], in_app: true, title: '', body: '' }) },
  // Kept so a graph written before the grid still opens; not offered in the palette, since
  // a grid has no branch to draw. Error handling is per task (`on_error`).
  if: {
    icon: 'share-2',
    defaults: () => ({ match: 'all', conditions: [{ left: '', op: 'eq', right: '' }] })
  },
  // Every field the editor binds to must exist here: `bind:value={config.missing}` is a
  // fatal Svelte error (`props_invalid_value`), not an empty input, and it takes the whole
  // page down with the modal still on top of it.
  transform: {
    icon: 'repeat',
    defaults: () => ({
      op: 'pick',
      source: '',
      template: '',
      delimiter: ',',
      separator: ', ',
      field: '',
      fields: {},
      shape: 'objects',
      key_col: '',
      value_col: ''
    })
  },
  delay: { icon: 'timer', defaults: () => ({ seconds: 30 }) }
};

export const KIND_ORDER = ['http', 'agent', 'transform', 'delay', 'notify'];

export function defaultGraph() {
  return { nodes: [], edges: [] };
}

/** A fresh block. Ids are short and stable: they are what `{{steps.<id>}}` names. */
export function newNode(kind, pos = null, existing = []) {
  const used = new Set(existing.map((n) => n.id));
  let i = 1;
  while (used.has(`${kind}${i}`)) i += 1;
  return {
    id: `${kind}${i}`,
    kind,
    name: '',
    config: KIND_META[kind]?.defaults() ?? {},
    pos: { x: Math.round(pos?.x ?? 0), y: Math.round(pos?.y ?? 0) },
    on_error: 'stop',
    retry: { count: 0, backoff_secs: 5 },
    timeout_secs: null
  };
}

// ── Layout: the grid is the graph ────────────────────────────────────────────

/** Nodes grouped into rows, rows top to bottom and tasks left to right inside one. */
export function toRows(nodes) {
  const rows = new Map();
  for (const n of nodes) {
    const y = Math.max(0, Math.round(n.pos?.y ?? 0));
    if (!rows.has(y)) rows.set(y, []);
    rows.get(y).push(n);
  }
  return [...rows.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([, list]) => list.sort((a, b) => (a.pos?.x ?? 0) - (b.pos?.x ?? 0)));
}

/** Rows back to a graph: empty rows dropped, positions renumbered, nodes stored in run
 *  order, and one link per step so the engine walks exactly what the grid shows. */
export function fromRows(rows) {
  const nodes = [];
  rows
    .filter((row) => row.length)
    .forEach((row, y) => row.forEach((n, x) => nodes.push({ ...n, pos: { x, y } })));
  const edges = nodes.slice(1).map((n, i) => ({ from: nodes[i].id, to: n.id, port: 'out' }));
  return { nodes, edges };
}

/** Blocks whose output a given block can read: its ancestors in the graph. */
export function ancestorsOf(graph, id) {
  const seen = new Set();
  const stack = [id];
  while (stack.length) {
    const current = stack.pop();
    for (const e of graph.edges) {
      if (e.to === current && !seen.has(e.from)) {
        seen.add(e.from);
        stack.push(e.from);
      }
    }
  }
  return [...seen];
}

export const label = (node) => (node?.name?.trim() ? node.name.trim() : node?.id ?? '');

// ── Schedules ────────────────────────────────────────────────────────────────

export const SCHEDULE_KINDS = ['interval', 'daily', 'weekly', 'monthly', 'once'];
/** Monday = bit 0, matching RemindMe and Routines. */
export const WEEKDAY_BITS = [0, 1, 2, 3, 4, 5, 6];

export function defaultSchedule(workflowId) {
  return {
    workflow_id: workflowId,
    kind: 'daily',
    timezone: guessTimezone(),
    every_minutes: 60,
    at_hour: 8,
    at_minute: 0,
    weekdays: 31,
    day_of_month: 1,
    run_at: null,
    catch_up: false,
    active: true
  };
}

export function guessTimezone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

const pad = (n) => String(n).padStart(2, '0');

/** A rule in one line. `tr` is the store's translate function. */
export function describeSchedule(s, tr, weekdayNames) {
  if (!s) return '';
  const at = `${pad(s.at_hour ?? 0)}:${pad(s.at_minute ?? 0)}`;
  switch (s.kind) {
    case 'interval': {
      const m = s.every_minutes ?? 60;
      return m % 60 === 0
        ? tr('automator.sched.everyHours', { n: m / 60 })
        : tr('automator.sched.everyMinutes', { n: m });
    }
    case 'daily':
      return tr('automator.sched.daily', { at });
    case 'weekly': {
      const mask = s.weekdays ?? 0;
      const days = WEEKDAY_BITS.filter((b) => mask & (1 << b))
        .map((b) => weekdayNames[b])
        .join(', ');
      return tr('automator.sched.weekly', { days: days || '-', at });
    }
    case 'monthly':
      return tr('automator.sched.monthly', { day: s.day_of_month ?? 1, at });
    case 'once':
      return tr('automator.sched.once', { at: s.run_at ? fmtLocal(s.run_at) : '-' });
    default:
      return s.kind;
  }
}

/** An ISO instant in the browser's own timezone, for display only. */
export function fmtLocal(iso) {
  if (!iso) return '';
  try {
    return new Date(iso).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short'
    });
  } catch {
    return iso;
  }
}

// ── Status ───────────────────────────────────────────────────────────────────

export const STATUS_TONE = {
  ok: 'success',
  running: 'accent',
  queued: 'accent',
  failed: 'danger',
  timeout: 'danger',
  cancelled: 'neutral',
  skipped: 'neutral',
  simulated: 'warn'
};

/** Duration in the shortest honest unit. */
export function fmtDuration(ms) {
  if (ms == null) return '';
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  const m = Math.floor(ms / 60000);
  const s = Math.round((ms % 60000) / 1000);
  return `${m} min ${s} s`;
}
