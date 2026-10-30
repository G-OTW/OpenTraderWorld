// Insights page helpers. Pure: no Svelte, no fetch, so tests/insights.test.js runs them
// under Node. The server decides which statements fire and sends keys and raw numbers;
// this file reads the page address, formats the numbers, and turns a finding's scenario
// into the link that acts on it (the journal lab, or a strategy draft with a filter).

/** The three sources, and the module each one needs. */
export const SOURCES = ['journal', 'backtest', 'paper'];
export const SOURCE_MODULE = { journal: 'journal', backtest: 'backtest', paper: 'backtest' };

/** Journal filters the page address may carry (same names as /api/journal/trades). */
const JOURNAL_FILTERS = ['category_id', 'strategy_id', 'asset_class', 'side', 'ticker', 'tag_id', 'since', 'until'];

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * The source a page address names: `?source=journal&category_id=...`,
 * `?source=backtest&run=<id>`, `?source=paper&session=<id>`. Returns `{ source }` or
 * `{ error }` with an i18n key; nothing is assumed when the address is incomplete.
 */
export function sourceFromQuery(params) {
  const kind = params.get('source') || 'journal';
  if (kind === 'journal') {
    const filters = {};
    for (const k of JOURNAL_FILTERS) {
      const v = params.get(k);
      if (v) filters[k] = v;
    }
    return { source: { kind, filters } };
  }
  if (kind === 'backtest') {
    const run = params.get('run') ?? '';
    return UUID.test(run) ? { source: { kind, run } } : { error: 'insights.error.noRun' };
  }
  if (kind === 'paper') {
    const session = params.get('session') ?? '';
    return UUID.test(session) ? { source: { kind, session } } : { error: 'insights.error.noSession' };
  }
  return { error: 'insights.error.source' };
}

/** The page address of a source (the reverse of `sourceFromQuery`). */
export function hrefOf(source) {
  const q = new URLSearchParams({ source: source.kind });
  if (source.kind === 'journal') {
    for (const [k, v] of Object.entries(source.filters ?? {})) if (v) q.set(k, v);
  }
  if (source.kind === 'backtest') q.set('run', source.run);
  if (source.kind === 'paper') q.set('session', source.session);
  return `/insights?${q}`;
}

/** The POST /api/insights body of a source. */
export function bodyOf(source, tzOffset = 0) {
  if (source.kind === 'backtest') return { source: { backtest: { run_id: source.run } } };
  if (source.kind === 'paper') return { source: { paper: { session_id: source.session } } };
  return { source: { journal: { filters: { ...(source.filters ?? {}) } } }, tz_offset: tzOffset };
}

/**
 * How each value of a statement is formatted. The names are shared across keys on
 * purpose (`net` is always money, `trades` always a count), so one table serves them all.
 */
export const VALUE_FMT = {
  hour: 'hour',
  weekday: 'weekday',
  trades: 'int',
  n: 'int',
  groups: 'int',
  net: 'money',
  saving: 'money',
  paid: 'money',
  alternative: 'money',
  fees: 'money',
  weak: 'money',
  strong: 'money',
  factor: 'factor',
  ratio: 'ratio',
  winners: 'minutes',
  losers: 'minutes',
  stop: 'pct',
  mae: 'pct',
  share: 'pct',
  pct: 'pct',
  escalation: 'signedPct'
};

/**
 * The parameters of a statement: values through the reader's formatters (`fmt[kind]`),
 * positional labels as `{0}`, `{1}` (through `labelOf`, which may translate a side name).
 */
export function insightParams(insight, fmt, labelOf = (l) => l) {
  const out = {};
  for (const [k, v] of Object.entries(insight.values ?? {})) {
    const f = fmt[VALUE_FMT[k] ?? 'num'] ?? fmt.num ?? String;
    out[k] = f(v);
  }
  (insight.labels ?? []).forEach((label, i) => {
    out[i] = labelOf(label, insight.key);
  });
  return out;
}

/**
 * The evidence line, as `[i18nKey, params]` parts in reading order: the sample, then
 * what the change does to the result, the drawdown and the risk taken per trade.
 */
export function evidenceParts(evidence, fmt) {
  if (!evidence) return [];
  const parts = [['insights.evidence.n', { n: fmt.int(evidence.n) }]];
  if (evidence.delta_pnl != null) parts.push(['insights.evidence.pnl', { v: fmt.signedMoney(evidence.delta_pnl) }]);
  if (evidence.delta_dd != null && Math.abs(evidence.delta_dd) > 1e-9) {
    parts.push(['insights.evidence.dd', { v: fmt.signedMoney(evidence.delta_dd) }]);
  }
  if (evidence.delta_risk != null) parts.push(['insights.evidence.risk', { v: fmt.signedPct(evidence.delta_risk) }]);
  return parts;
}

/** The lab kinds a scenario may carry into the journal. */
const LAB_KINDS = ['exits', 'size', 'filter', 'reverse', 'fees'];

/** A time-window-only filter: the one scenario a strategy's trading window can take. */
export function isWindowFilter(scenario) {
  return scenario?.kind === 'filter' && !!scenario.window && !scenario.signal;
}

/** The journal lab with the scenario prefilled, in the same category. */
export function labHref(scenario, filters = {}) {
  const q = new URLSearchParams({ view: 'lab', scenario: JSON.stringify(scenario) });
  if (filters.category_id) q.set('category', filters.category_id);
  return `/journal?${q}`;
}

/**
 * What a finding's button does: `{ kind: 'lab', href }` on the journal, `{ kind: 'filter',
 * href }` on a run or a paper session when the scenario is a time window, else null.
 */
export function actionOf(insight, source) {
  const s = insight?.apply;
  if (!s || !LAB_KINDS.includes(s.kind)) return null;
  if (source.kind === 'journal') return { kind: 'lab', href: labHref(s, source.filters) };
  if (!isWindowFilter(s)) return null;
  const w = encodeURIComponent(JSON.stringify(s.window));
  if (source.kind === 'backtest') return { kind: 'filter', href: `/backtest?run=${source.run}&window=${w}` };
  if (source.kind === 'paper') return { kind: 'filter', href: `/backtest?paper_session=${source.session}&window=${w}` };
  return null;
}

/** A scenario handed over in an address, or null when it is not one the lab runs. */
export function parseScenario(text) {
  try {
    const s = JSON.parse(text);
    return s && typeof s === 'object' && LAB_KINDS.includes(s.kind) ? s : null;
  } catch {
    return null;
  }
}

const ints = (v, lo, hi) => Array.isArray(v) && v.every((x) => Number.isInteger(x) && x >= lo && x <= hi);

/** A time window handed over in an address, or null when malformed. */
export function parseWindow(text) {
  try {
    const w = JSON.parse(text);
    if (!w || typeof w !== 'object') return null;
    const hours = w.hours ?? [];
    const weekdays = w.weekdays ?? [];
    if (!ints(hours, 0, 23) || !ints(weekdays, 1, 7) || (!hours.length && !weekdays.length)) return null;
    return { hours, weekdays, exclude: !!w.exclude };
  } catch {
    return null;
  }
}

const hhmm = (m) => `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
const minutes = (s) => {
  const m = /^(\d{1,2}):(\d{2})$/.exec(String(s ?? '').trim());
  if (!m) return null;
  const v = Number(m[1]) * 60 + Number(m[2]);
  return v >= 0 && v < 1440 ? v : null;
};

/** Minute-of-day mask of a strategy's sessions, read as the engine reads them. */
function sessionMask(sessions) {
  const mask = new Array(1440).fill(!sessions?.length);
  for (const s of sessions ?? []) {
    const a = minutes(s.from);
    const b = minutes(s.to);
    if (a == null || b == null || a === b) continue;
    for (let m = 0; m < 1440; m++) {
      if (a < b ? m >= a && m < b : m >= a || m < b) mask[m] = true;
    }
  }
  return mask;
}

/** Sessions that open exactly the minutes of `mask` (a run over midnight wraps). */
function maskSessions(mask) {
  if (mask.every(Boolean)) return [];
  const runs = [];
  for (let m = 0; m < 1440; m++) {
    if (mask[m] && (m === 0 || !mask[m - 1])) runs.push([m, m + 1]);
    else if (mask[m]) runs[runs.length - 1][1] = m + 1;
  }
  if (runs.length > 1 && runs[0][0] === 0 && runs[runs.length - 1][1] === 1440) {
    const first = runs.shift();
    runs[runs.length - 1] = [runs[runs.length - 1][0], first[1]];
  }
  return runs.map(([a, b]) => ({ from: hhmm(a), to: hhmm(b % 1440) }));
}

/**
 * A strategy's trading-window filters with a found time window applied: the weekdays and
 * the intraday sessions it already had, narrowed (never widened). Hours and weekdays are
 * the strategy's own clock, the one the researcher read the run on. Returns `{ filters }`,
 * or `{ error }` (an i18n key) when nothing would be left to trade.
 */
export function applyWindow(filters, window) {
  const f = structuredClone(filters ?? {});
  if (window.weekdays?.length) {
    const base = f.weekdays?.length ? f.weekdays : [1, 2, 3, 4, 5, 6, 7];
    const next = base.filter((d) => window.weekdays.includes(d) !== !!window.exclude);
    if (!next.length) return { error: 'insights.error.emptyWindow' };
    f.weekdays = next.length === 7 ? [] : next;
  }
  if (window.hours?.length) {
    const mask = sessionMask(f.sessions);
    for (let m = 0; m < 1440; m++) {
      const inside = window.hours.includes(Math.floor(m / 60));
      if (inside === !!window.exclude) mask[m] = false;
    }
    if (!mask.some(Boolean)) return { error: 'insights.error.emptyWindow' };
    f.sessions = maskSessions(mask);
  }
  return { filters: f };
}

/**
 * Value kinds of the journal's own statements (behaviour and exposure), listed on the
 * page unchanged: their keys and wording are the journal's, only the formatting is here.
 */
export const JOURNAL_VALUE_FMT = {
  net: 'signedMoney',
  without: 'signedMoney',
  expectancy: 'signedMoney',
  normal: 'signedMoney',
  first: 'signedMoney',
  later: 'signedMoney',
  afterWin: 'signedMoney',
  afterLoss: 'signedMoney',
  risk: 'money',
  correlated: 'money',
  independent: 'money',
  share: 'pct',
  escalation: 'signedPct',
  trades: 'int',
  winners: 'int',
  count: 'int',
  total: 'int',
  positions: 'int'
};

/** Parameters of a journal statement, as the journal's own panels format them. */
export function journalParams(insight, fmt) {
  const out = {};
  for (const [k, v] of Object.entries(insight.values ?? {})) {
    const f = fmt[JOURNAL_VALUE_FMT[k] ?? 'num'] ?? String;
    out[k] = f(v);
  }
  (insight.labels ?? []).forEach((label, i) => {
    out[i] = label;
  });
  return out;
}

/** Weekday name (1 = Monday), in the reader's language. */
export function weekdayName(d, locale = 'en', style = 'long') {
  return new Date(Date.UTC(2024, 0, Number(d))).toLocaleDateString(locale, { weekday: style, timeZone: 'UTC' });
}

/** Minutes as a short duration through the unit formatters (`min`, `hour`, `day`). */
export function duration(m, units, fmtNum = String) {
  if (m < 90) return units.min(Math.round(m));
  if (m < 60 * 36) return units.hour(fmtNum(m / 60));
  return units.day(fmtNum(m / 1440));
}
