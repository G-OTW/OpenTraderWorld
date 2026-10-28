/**
 * Fundamentals data access, all from `/api/fundamentals`.
 *
 * Aggregator datasets (estimates, dividends, holders...) are snapshots: `tab()` makes sure
 * the ones a tab needs are stored and fresh (refreshing through the first connected
 * provider) and returns the tab's data plus one note per dataset (who answered, when, or
 * the error naming the fix).
 */
async function req(path, options = {}) {
  const res = await fetch(`/api/fundamentals${path}`, { headers: { 'content-type': 'application/json' }, ...options });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* no JSON body */
  }
  if (!res.ok) {
    const err = new Error(body?.error ?? `request failed (${res.status})`);
    err.status = res.status;
    throw err;
  }
  return body;
}

const post = (path, body) => req(path, { method: 'POST', body: JSON.stringify(body ?? {}) });
const patch = (path, body) => req(path, { method: 'PATCH', body: JSON.stringify(body) });
const del = (path) => req(path, { method: 'DELETE' });
const enc = encodeURIComponent;

/** Datasets each company tab needs, and the tabs the backend joins with statements. */
const TAB_DATASETS = {
  overview: ['price'],
  estimates: ['estimates'],
  earnings: ['earnings', 'price'],
  segments: ['segments'],
  capital: ['dividends'],
  ownership: ['holders', 'short_interest'],
  peers: ['peers'],
  ratings: ['esg']
};
const VIEWS = new Set(['earnings', 'capital', 'ownership', 'peers', 'ratings']);

/** Nothing was asked: every provider refused lately or spent its quota. `deferred` says who. */
function deferredError(deferred) {
  const err = new Error('deferred');
  err.deferred = deferred;
  return err;
}

/**
 * Stored snapshot, refreshed first when missing or stale. An automatic refresh skips the
 * providers that refused lately or spent their quota (the snapshot or the error then
 * carries `deferred`); `force` (the Refresh button) asks them anyway.
 */
async function ensure(dataset, subject = '_', force = false) {
  const q = `subject=${enc(subject)}`;
  const { snapshot } = await req(`/data/${dataset}?${q}`);
  if (snapshot && !snapshot.stale && !force) return snapshot;
  let r;
  try {
    r = await post(`/data/${dataset}/refresh?${q}${force ? '&force=true' : ''}`);
  } catch (e) {
    // A stale answer beats none; the note still says why it is stale.
    if (snapshot) return { ...snapshot, error: e.message };
    throw e;
  }
  if (!r.deferred) return r.snapshot;
  const kept = r.snapshot ?? snapshot;
  if (kept) return { ...kept, deferred: r.deferred };
  throw deferredError(r.deferred);
}

function note(dataset, snap, err) {
  return {
    dataset,
    provider: snap?.provider_label ?? '',
    fetched_at: snap?.fetched_at ?? null,
    error: err ? (err.deferred ? '' : err.message) : (snap?.error ?? ''),
    deferred: err?.deferred ?? snap?.deferred ?? []
  };
}

/** A series is picked by `provider:code`, stable across reinstalls; the API wants its id. */
const seriesIds = new Map();
const seriesKey = (s) => `${s.provider_id}:${s.code}`;

export const fundamentalsApi = {
  series: async () => {
    const { series } = await req('/series');
    return series.map((s) => {
      seriesIds.set(seriesKey(s), s.id);
      return { ...s, uuid: s.id, id: seriesKey(s) };
    });
  },
  observations: async (key) => {
    const id = seriesIds.get(key);
    if (!id) return [];
    return (await req(`/series/${id}/observations`)).observations;
  },
  addSeries: (provider, code, category) => post('/series', { provider, code, category }).then((r) => r.series),
  removeSeries: (uuid) => del(`/series/${uuid}`),
  refreshSeries: (uuid) => post(`/series/${uuid}/refresh`),
  setSeriesCategory: (uuid, category) => patch(`/series/${uuid}`, { category }),
  starter: () => req('/starter').then((r) => r.series),
  searchSeries: (provider, q) => req(`/lookup/series?provider=${enc(provider)}&q=${enc(q)}`).then((r) => r.series),
  yieldCurve: () => req('/lookup/yield-curve').then((r) => r.curve),
  centralBanks: async () => (await ensure('central_banks')).data,

  searchCompanies: async (q) => {
    if (!q.trim()) return [];
    return (await req(`/lookup/companies?q=${enc(q)}`)).companies;
  },
  companies: () => req('/companies').then((r) => r.companies),
  /** Strict stored reads for dashboards: never resolve or fetch a missing company. */
  storedCompany: (ticker) => req(`/companies/${enc(ticker)}`).then((r) => r.company),
  storedView: (ticker, view) => req(`/companies/${enc(ticker)}/view/${enc(view)}`),
  /** Stored profile, or open it (resolve on EDGAR, store, fetch) when it is not yet. */
  company: async (ticker) => {
    try {
      return (await req(`/companies/${enc(ticker)}`)).company;
    } catch (e) {
      if (e.status !== 404) throw e;
      return (await post('/companies', { ticker })).company;
    }
  },
  refreshCompany: (ticker) => post(`/companies/${enc(ticker)}/refresh`),
  followCompany: (ticker, followed) => patch(`/companies/${enc(ticker)}`, { followed }),
  removeCompany: (ticker) => del(`/companies/${enc(ticker)}`),
  /** Move a stored company to the front of the recent ones. */
  openedCompany: (ticker) => patch(`/companies/${enc(ticker)}`, { opened: true }),
  /** ETFs the user opened (favourites and recent ones). */
  etfs: () => req('/etfs').then((r) => r.etfs),
  /** Keep an ETF once its data is stored, or move it to the front of the recent ones. */
  openedEtf: (ticker) => patch(`/etfs/${enc(ticker)}`, { opened: true }),
  followEtf: (ticker, followed) => patch(`/etfs/${enc(ticker)}`, { followed }),
  statements: (t, kind, freq) => req(`/companies/${enc(t)}/statements?kind=${kind}&freq=${freq}`).then((r) => r.statements),
  filings: (t) => req(`/companies/${enc(t)}/documents`).then((r) => r.documents),
  insiders: (t) => req(`/companies/${enc(t)}/insiders`).then((r) => r.insiders),
  documents: (q = '') => req(`/documents${q.trim() ? `?q=${enc(q.trim())}` : ''}`).then((r) => r.documents),
  sources: () => req('/sources').then((r) => r.sources),

  ensure,
  /** The stored snapshot only, never a provider call (dashboard widgets). */
  stored: (dataset, subject = '_') => req(`/data/${dataset}?subject=${enc(subject)}`).then((r) => r.snapshot),
  /** A company tab's data: `{ data, notes }`. `data` is null when nothing could be fetched. */
  tab: async (ticker, tab, force = false) => {
    const names = TAB_DATASETS[tab] ?? [];
    const snaps = {};
    const notes = await Promise.all(
      names.map(async (d) => {
        try {
          snaps[d] = await ensure(d, ticker, force);
          return note(d, snaps[d]);
        } catch (e) {
          return note(d, null, e);
        }
      })
    );
    let data = null;
    if (VIEWS.has(tab)) data = await req(`/companies/${enc(ticker)}/view/${tab}`);
    else data = snaps[names[0]]?.data ?? null;
    return { data, notes };
  },
  transcripts: (t) => req(`/companies/${enc(t)}/documents?form=transcript`).then((r) => r.documents),
  /** List a company's calls; `force` (Refresh) asks providers that refused lately too. */
  loadTranscripts: async (t, force = false) => {
    const r = await post(`/companies/${enc(t)}/transcripts/refresh${force ? '?force=true' : ''}`);
    if (r.deferred) throw deferredError(r.deferred);
    return r;
  },
  transcript: async (id) => {
    let { document } = await req(`/documents/${id}`);
    if (!document.segments.length) document = (await post(`/documents/${id}/fetch`)).document;
    return document;
  },
  datasets: () => req('/datasets'),
  /** Provider priority of a dataset (or `transcripts`): every provider id, best first. */
  setOrder: (id, providers) => req(`/datasets/${enc(id)}/order`, { method: 'PUT', body: JSON.stringify({ providers }) }),
  resetOrder: (id) => del(`/datasets/${enc(id)}/order`),

  etf: async (ticker, force = false) => {
    const snap = await ensure('etf', ticker.toUpperCase(), force);
    return { ...snap.data, note: note('etf', snap) };
  },
  calendar: async (force = false) => {
    const notes = [];
    for (const d of ['calendar', 'central_banks']) {
      try {
        notes.push(note(d, await ensure(d, '_', force)));
      } catch (e) {
        notes.push(note(d, null, e));
      }
    }
    return { ...(await req('/calendar')), notes };
  },
  /**
   * One alternative dataset (`congress`, `lobbying`, `contracts`, `patents`) for a ticker,
   * or `_` for the latest congress trades: `{ data, notes }`.
   */
  alt: async (dataset, subject = '_', force = false) => {
    try {
      const snap = await ensure(dataset, subject, force);
      return { data: snap.data, notes: [note(dataset, snap)] };
    } catch (e) {
      return { data: null, notes: [note(dataset, null, e)] };
    }
  }
};

export { MACRO_CATEGORIES, RECESSIONS, STATEMENT_LINES, SUGGESTED_ETFS, RECENT_SYMBOLS, DATA_ELEMENTS } from './constants.js';

/** Series still fetching, or that failed, show their state instead of a value. */
export const isPending = (row) => row?.status === 'pending';

const DAY_MS = 86400000;

/**
 * The observation a year before each one, matched on the date: the latest period starting
 * no later than one year back, within half a period of it. A gap in the series gives no
 * YoY value rather than a comparison against the wrong period.
 */
function yearAgo(obs, freq) {
  const slack = (freq === 'D' ? 4 : freq === 'W' ? 4 : freq === 'M' ? 15 : freq === 'Q' ? 45 : 183) * DAY_MS;
  let j = 0;
  return obs.map(([t]) => {
    const d = new Date(t);
    const target = Date.UTC(d.getUTCFullYear() - 1, d.getUTCMonth(), d.getUTCDate());
    while (j + 1 < obs.length && obs[j + 1][0] <= target) j++;
    const prev = obs[j];
    return prev && prev[0] <= target && target - prev[0] <= slack ? prev[1] : null;
  });
}

/** Transforms are computed on read, never stored. `obs` is `[[ts, value], ...]`. */
export function transform(obs, kind, freq) {
  switch (kind) {
    case 'yoy': {
      const base = yearAgo(obs, freq);
      return obs
        .map(([t, v], i) => [t, base[i] ? ((v - base[i]) / Math.abs(base[i])) * 100 : null])
        .filter(([t]) => t - obs[0][0] >= 365 * DAY_MS);
    }
    case 'pop':
      return obs.slice(1).map(([t, v], i) => [t, obs[i][1] ? ((v - obs[i][1]) / Math.abs(obs[i][1])) * 100 : null]);
    case 'diff':
      return obs.slice(1).map(([t, v], i) => [t, v - obs[i][1]]);
    case 'index': {
      const base = obs[0]?.[1];
      return base ? obs.map(([t, v]) => [t, (v / base) * 100]) : obs;
    }
    default:
      return obs;
  }
}

export function sliceRange(obs, range) {
  if (range === 'max' || !obs.length) return obs;
  const years = { '1y': 1, '5y': 5, '10y': 10 }[range] ?? 100;
  const from = Date.now() - years * 365.25 * 86400000;
  return obs.filter(([t]) => t >= from);
}

// ── Formatting ──────────────────────────────────────────────────────────────

export function fmtBig(v, currency = '') {
  if (v == null || Number.isNaN(v)) return '·';
  const a = Math.abs(v);
  const sign = v < 0 ? '−' : '';
  const unit = a >= 1e12 ? [1e12, 'T'] : a >= 1e9 ? [1e9, 'B'] : a >= 1e6 ? [1e6, 'M'] : a >= 1e3 ? [1e3, 'K'] : [1, ''];
  return `${sign}${(a / unit[0]).toFixed(a >= 1e3 ? 2 : 0)}${unit[1]}${currency ? ' ' + currency : ''}`;
}

export function fmtNum(v, d = 2) {
  if (v == null || Number.isNaN(v)) return '·';
  return v.toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d }).replace('-', '−');
}

export function fmtPct(v, d = 1, signed = false) {
  if (v == null || Number.isNaN(v)) return '·';
  const s = signed && v > 0 ? '+' : '';
  return `${s}${fmtNum(v, d)}%`;
}

export function fmtDate(ts) {
  return new Date(ts).toISOString().slice(0, 10);
}
