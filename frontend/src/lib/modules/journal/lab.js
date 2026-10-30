// What-if lab helpers. Pure: no Svelte, no fetch, so tests/journal-lab.test.js can run
// them under Node. The server runs the scenarios and decides whether a statement fires;
// this file turns a form into a scenario and an outcome into table rows.

/** The lab's tabs, in display order. `exits` is the exit-signal lab. */
export const LAB_TABS = ['exits', 'size', 'filter', 'reverse', 'fees'];

/** Weekday numbers the server reads (1 = Monday to 7 = Sunday). */
export const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7];

/**
 * Which journal view and lab tab a requested view name opens. The exit lab used to be
 * its own screen (`exits`): an old link or a saved preference still lands on it.
 */
export function resolveView(asked, navIds) {
  if (asked === 'exits') return { view: 'lab', tab: 'exits' };
  if (navIds.includes(asked)) return { view: asked, tab: null };
  return null;
}

/**
 * Parse "9, 10, 14-16" into [9, 10, 14, 15, 16], sorted and without repeats. Returns
 * null when an entry is not a whole number inside [min, max], so the form can refuse it
 * instead of sending a guess.
 */
export function parseList(text, min, max) {
  const out = new Set();
  for (const raw of String(text ?? '').split(',')) {
    const part = raw.trim();
    if (!part) continue;
    const m = part.match(/^(\d{1,2})(?:\s*-\s*(\d{1,2}))?$/);
    if (!m) return null;
    const a = Number(m[1]);
    const b = m[2] == null ? a : Number(m[2]);
    if (a < min || b > max || a > b) return null;
    for (let i = a; i <= b; i++) out.add(i);
  }
  return [...out].sort((x, y) => x - y);
}

/** [9, 10, 11, 14] into "9-11, 14". */
export function formatList(nums) {
  const s = [...new Set(nums ?? [])].sort((a, b) => a - b);
  const parts = [];
  for (let i = 0; i < s.length; i++) {
    let j = i;
    while (j + 1 < s.length && s[j + 1] === s[j] + 1) j++;
    parts.push(j > i ? `${s[i]}-${s[j]}` : String(s[i]));
    i = j;
  }
  return parts.join(', ');
}

/** An empty form, per kind. */
export function emptyForm() {
  return {
    factor: 1.5,
    cap: '',
    useSignal: false,
    signal: { logic: 'all', conditions: [] },
    useWindow: true,
    hours: '',
    weekdays: [],
    exclude: true,
    onlyMatching: false,
    scheduleId: '',
    family: '',
    to: 'large'
  };
}

/** The filter part of a form, or an error key when it cannot be sent. */
function filterOf(form) {
  const f = {};
  if (form.useSignal) {
    if (!form.signal?.conditions?.length) return { error: 'journal.lab.error.emptySignal' };
    f.signal = form.signal;
  }
  if (form.useWindow) {
    const hours = parseList(form.hours, 0, 23);
    if (hours == null) return { error: 'journal.lab.error.hours' };
    const weekdays = [...(form.weekdays ?? [])].sort((a, b) => a - b);
    if (!hours.length && !weekdays.length) return { error: 'journal.lab.error.emptyWindow' };
    f.window = { hours, weekdays, exclude: !!form.exclude };
  }
  if (!f.signal && !f.window) return { error: 'journal.lab.error.emptyFilter' };
  return { filter: f };
}

/**
 * The scenario a form describes: `{ scenario }`, or `{ error }` with an i18n key when a
 * field is missing or malformed. The server checks again; this only saves a round trip.
 */
export function buildScenario(kind, form) {
  switch (kind) {
    case 'size': {
      const factor = Number(form.factor);
      if (!Number.isFinite(factor) || factor < 0.5 || factor > 3) return { error: 'journal.lab.error.factor' };
      const scenario = { kind, factor };
      if (String(form.cap ?? '').trim() !== '') {
        const cap = Number(form.cap);
        if (!Number.isFinite(cap) || cap <= 0) return { error: 'journal.lab.error.cap' };
        scenario.daily_loss_cap = cap;
      }
      return { scenario };
    }
    case 'filter': {
      const r = filterOf(form);
      return r.error ? r : { scenario: { kind, ...r.filter } };
    }
    case 'reverse': {
      if (!form.onlyMatching) return { scenario: { kind } };
      const r = filterOf(form);
      return r.error ? r : { scenario: { kind, only: r.filter } };
    }
    case 'fees': {
      if (!form.scheduleId) return { error: 'journal.lab.error.schedule' };
      const scenario = { kind, schedule_id: form.scheduleId };
      if (form.family) {
        scenario.family = form.family;
        scenario.to = form.to === 'small' ? 'small' : 'large';
      }
      return { scenario };
    }
    default:
      return { error: 'journal.lab.error.kind' };
  }
}

/** The form a scenario fills (the reverse of `buildScenario`), to open one in the lab. */
export function formOf(scenario) {
  const f = emptyForm();
  if (!scenario) return f;
  const fillFilter = (src) => {
    f.useSignal = !!src?.signal;
    if (src?.signal) f.signal = structuredClone(src.signal);
    f.useWindow = !!src?.window;
    if (src?.window) {
      f.hours = formatList(src.window.hours);
      f.weekdays = [...(src.window.weekdays ?? [])];
      f.exclude = !!src.window.exclude;
    }
  };
  switch (scenario.kind) {
    case 'size':
      f.factor = scenario.factor;
      f.cap = scenario.daily_loss_cap ?? '';
      break;
    case 'filter':
      fillFilter(scenario);
      break;
    case 'reverse':
      f.onlyMatching = !!scenario.only;
      if (scenario.only) fillFilter(scenario.only);
      break;
    case 'fees':
      f.scheduleId = scenario.schedule_id ?? '';
      f.family = scenario.family ?? '';
      f.to = scenario.to ?? 'large';
      break;
  }
  return f;
}

/**
 * Rows of the before and after table. `better` says which way is good for the metric
 * (+1 higher, -1 lower, 0 neutral), so the delta can be coloured without a guess.
 */
export const SUMMARY_METRICS = [
  { key: 'n', kind: 'int', better: 0 },
  { key: 'net', kind: 'money', better: 1 },
  { key: 'fees', kind: 'money', better: -1 },
  { key: 'win_rate', kind: 'pct', better: 1 },
  { key: 'expectancy', kind: 'money', better: 1 },
  { key: 'profit_factor', kind: 'num', better: 1 },
  { key: 'max_drawdown', kind: 'money', better: -1 },
  { key: 'worst_day', kind: 'money', better: 1 }
];

export function summaryRows(before, after) {
  return SUMMARY_METRICS.map((m) => {
    const b = before?.[m.key] ?? null;
    const a = after?.[m.key] ?? null;
    const delta = a != null && b != null ? a - b : null;
    const tone = delta == null || Math.abs(delta) < 1e-9 || m.better === 0 ? '' : delta * m.better > 0 ? 'pos' : 'neg';
    return { ...m, before: b, after: a, delta, tone };
  });
}

/** Counts per value of `key`, most frequent first: `[[value, n], ...]`. */
export function countBy(list, key) {
  const m = new Map();
  for (const x of list ?? []) m.set(x[key], (m.get(x[key]) ?? 0) + 1);
  return [...m.entries()].sort((a, b) => b[1] - a[1]);
}

/** How each statement value is formatted: the server sends raw numbers. */
export const STATEMENT_FMT = {
  n: 'int',
  delta: 'money',
  dd_delta: 'money',
  saving: 'money',
  cap: 'money',
  factor: 'factor',
  removed: 'int',
  flipped: 'int',
  skipped: 'int',
  hour: 'int',
  pct: 'pct'
};

/** Statement parameters, formatted with the reader's formatters. */
export function statementParams(values, fmt) {
  const out = {};
  for (const [k, v] of Object.entries(values ?? {})) {
    const f = fmt[STATEMENT_FMT[k] ?? 'num'] ?? String;
    out[k] = f(v);
  }
  return out;
}
