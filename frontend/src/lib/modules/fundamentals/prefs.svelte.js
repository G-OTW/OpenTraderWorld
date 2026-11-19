/**
 * Fundamentals view preferences: section order and visibility per page, collapsed
 * sections, density, and the per-page options (macro views, overview tiles, statement
 * units...). A user preference (`$lib/prefs.svelte.js`, mirrored to localStorage as
 * JSON), so it follows the user across devices.
 */
import { getPref, onPref, setPref } from '$lib/prefs.svelte.js';

const KEY = 'fundamentals.prefs.v2'; // gitleaks:allow

export const METRIC_GROUPS = {
  valuation: ['marketCap', 'ev', 'pe', 'forwardPe', 'evEbitda', 'ps', 'pb', 'fcfYield'],
  profitability: ['grossMargin', 'opMargin', 'netMargin', 'roe', 'roic'],
  balance: ['debtEquity', 'currentRatio', 'beta'],
  shareholders: ['divYield', 'sharesOut']
};

const DEFAULTS = {
  density: 'comfortable',
  // page -> { order: [section ids], hidden: [section ids] }; missing ids fall back to
  // the page's declared order, so a new section shows up without a migration.
  layout: {},
  collapsed: [],
  macro: {
    picked: ['fred:CPIAUCSL', 'fred:FEDFUNDS'],
    tf: 'level',
    range: '10y',
    chart: 'line',
    shade: true,
    views: [
      { name: 'Inflation vs rates', picked: ['fred:CPIAUCSL', 'fred:FEDFUNDS', 'fred:DGS10'], tf: 'level', range: '10y' },
      { name: 'Labour market', picked: ['fred:UNRATE', 'fred:ICSA'], tf: 'level', range: '5y' }
    ]
  },
  company: {
    ticker: 'AAPL',
    tabs: ['overview', 'financials', 'estimates', 'earnings', 'segments', 'capital', 'ownership', 'peers', 'ratings', 'filings', 'transcripts'],
    hiddenTabs: [],
    tiles: ['marketCap', 'pe', 'forwardPe', 'evEbitda', 'fcfYield', 'grossMargin', 'opMargin', 'netMargin', 'roe', 'roic', 'debtEquity', 'divYield']
  },
  financials: { units: 'auto', growth: true, hiddenLines: [] },
  etf: { ticker: 'SPY' },
  library: { tab: 'series' }
};

function merge(base, over) {
  if (!over || typeof over !== 'object' || Array.isArray(base)) return over ?? base;
  const out = { ...base };
  for (const k of Object.keys(over)) {
    out[k] = base[k] && typeof base[k] === 'object' && !Array.isArray(base[k]) ? merge(base[k], over[k]) : over[k];
  }
  return out;
}

function load(value = getPref(KEY, null)) {
  try {
    // A copy: the proxy below must not mutate the value the preference store holds.
    return merge(structuredClone(DEFAULTS), JSON.parse(JSON.stringify(value ?? null)));
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export const prefs = $state(load());

// The JSON last read from or written to the preference: the save effect skips it, so
// restoring the preferences is never echoed back as a write.
let stored = JSON.stringify(prefs);

// The server's copy arrives after first paint (another browser may have changed it).
onPref(KEY, (v) => {
  const next = load(v ?? null);
  const json = JSON.stringify(next);
  if (json === JSON.stringify(prefs)) {
    stored = json;
    return;
  }
  for (const k of Object.keys(prefs)) if (!(k in next)) delete prefs[k];
  Object.assign(prefs, next);
  stored = JSON.stringify(prefs);
});

$effect.root(() => {
  $effect(() => {
    const snap = JSON.stringify(prefs);
    if (snap === stored) return;
    stored = snap;
    setPref(KEY, JSON.parse(snap));
  });
});

export function resetPage(page) {
  delete prefs.layout[page];
  prefs.collapsed = prefs.collapsed.filter((k) => !k.startsWith(`${page}.`));
  if (page === 'company') {
    prefs.company.tabs = [...DEFAULTS.company.tabs];
    prefs.company.hiddenTabs = [];
    prefs.company.tiles = [...DEFAULTS.company.tiles];
    prefs.financials = structuredClone(DEFAULTS.financials);
  }
  if (page === 'macro') {
    const views = prefs.macro.views;
    prefs.macro = { ...structuredClone(DEFAULTS.macro), views };
  }
}

/** Section ids of `page` in the user's order, declared order for anything new. */
export function ordered(page, ids) {
  const saved = prefs.layout[page]?.order ?? [];
  return [...saved.filter((id) => ids.includes(id)), ...ids.filter((id) => !saved.includes(id))];
}

export function visibleSections(page, ids) {
  const hidden = prefs.layout[page]?.hidden ?? [];
  return ordered(page, ids).filter((id) => !hidden.includes(id));
}

export function setLayout(page, order, hidden) {
  prefs.layout[page] = { order, hidden };
}

export function isCollapsed(key) {
  return prefs.collapsed.includes(key);
}

export function toggleCollapsed(key) {
  prefs.collapsed = isCollapsed(key) ? prefs.collapsed.filter((k) => k !== key) : [...prefs.collapsed, key];
}

/** Move `id` one step within `list` (dir -1 up, +1 down); returns a new array. */
export function move(list, id, dir) {
  const i = list.indexOf(id);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= list.length) return list;
  const out = [...list];
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}
