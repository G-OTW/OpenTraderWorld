/** FinanceDatabase API client — catalog install, search, folders + favorites. Single-user. */
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

export const findbApi = {
  status: () => req('/findb/status'),
  install: () => req('/findb/install', { method: 'POST' }),

  /**
   * Search/browse the catalog. `opts` may include: q, type, exchange, currency, country,
   * sector, industry, category, family, sort ('relevance'|'symbol'|'name'), limit, offset.
   * Returns { results, has_more }.
   */
  search: (opts = {}) => {
    const p = new URLSearchParams();
    for (const [k, v] of Object.entries(opts)) {
      if (v !== '' && v != null) p.set(k, String(v));
    }
    return req(`/findb/search?${p}`);
  },

  /** Distinct values for a filter column, optionally scoped to an asset type. */
  facet: (column, type = '') => {
    const p = new URLSearchParams({ column });
    if (type) p.set('type', type);
    return req(`/findb/facets?${p}`).then((r) => r.values);
  },

  listFolders: () => req('/findb/folders').then((r) => r.folders),
  addFolder: (folder) =>
    req('/findb/folders', { method: 'POST', body: JSON.stringify(folder) }).then((r) => r.folder),
  updateFolder: (id, folder) =>
    req(`/findb/folders/${id}`, { method: 'PATCH', body: JSON.stringify(folder) }),
  removeFolder: (id) => req(`/findb/folders/${id}`, { method: 'DELETE' }),

  listFavorites: () => req('/findb/favorites').then((r) => r.favorites),
  addFavorite: (fav) =>
    req('/findb/favorites', { method: 'POST', body: JSON.stringify(fav) }).then((r) => r.id),
  updateFavorite: (id, fav) =>
    req(`/findb/favorites/${id}`, { method: 'PATCH', body: JSON.stringify(fav) }),
  removeFavorite: (id) => req(`/findb/favorites/${id}`, { method: 'DELETE' })
};

/** Asset-type facets for the search filter (value matches backend asset_type). */
export const ASSET_TYPES = [
  { value: '', label: 'All' },
  { value: 'equity', label: 'Equities' },
  { value: 'etf', label: 'ETFs' },
  { value: 'fund', label: 'Funds' },
  { value: 'index', label: 'Indices' },
  { value: 'currency', label: 'Currencies' },
  { value: 'crypto', label: 'Crypto' },
  { value: 'moneymarket', label: 'Money markets' }
];

const TYPE_LABELS = Object.fromEntries(ASSET_TYPES.map((t) => [t.value, t.label]));

/**
 * Filter facets shown in the search bar. `key` is both the query param and facet column.
 * `types` restricts which asset types the filter is relevant for (null = all). The
 * universal facets (exchange/currency/country) show always; the rest are contextual.
 */
export const FILTERS = [
  { key: 'exchange', label: 'Exchange', types: null },
  { key: 'currency', label: 'Currency', types: null },
  { key: 'country', label: 'Country', types: ['equity'] },
  { key: 'sector', label: 'Sector', types: ['equity'] },
  { key: 'industry', label: 'Industry', types: ['equity'] },
  { key: 'category', label: 'Category', types: ['etf', 'fund', 'index'] },
  { key: 'family', label: 'Family', types: ['etf', 'fund', 'moneymarket'] }
];

/** Which filters apply for a given asset type (empty type = universal only). */
export function filtersFor(type) {
  return FILTERS.filter((f) => !f.types || (type && f.types.includes(type)));
}

export const SORTS = [
  { value: 'relevance', label: 'Best match' },
  { value: 'symbol', label: 'Symbol A–Z' },
  { value: 'name', label: 'Name A–Z' }
];

/** Singular display label for an asset_type, e.g. 'equity' -> 'Equity'. */
export function typeLabel(t) {
  return TYPE_LABELS[t] ?? t;
}

/** Debounce a function by `ms` (used for the live search box). */
export function debounce(fn, ms = 250) {
  let h;
  return (...args) => {
    clearTimeout(h);
    h = setTimeout(() => fn(...args), ms);
  };
}
