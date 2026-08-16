/** MyWealth API client — templates, assets, revisions, net-worth breakdown, settings. */
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

function qs(filter = {}) {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(filter)) {
    if (v !== undefined && v !== null && v !== '') p.set(k, v);
  }
  const s = p.toString();
  return s ? `?${s}` : '';
}

export const wealthApi = {
  listTemplates: () => req('/wealth/templates').then((r) => r.templates),
  addTemplate: (t) => req('/wealth/templates', { method: 'POST', body: JSON.stringify(t) }).then((r) => r.id),
  updateTemplate: (id, patch) =>
    req(`/wealth/templates/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteTemplate: (id) => req(`/wealth/templates/${id}`, { method: 'DELETE' }),

  listAssets: (filter = {}) => req(`/wealth/assets${qs(filter)}`).then((r) => r.assets),
  addAsset: (a) => req('/wealth/assets', { method: 'POST', body: JSON.stringify(a) }).then((r) => r.id),
  updateAsset: (id, a) => req(`/wealth/assets/${id}`, { method: 'PATCH', body: JSON.stringify(a) }),
  deleteAsset: (id) => req(`/wealth/assets/${id}`, { method: 'DELETE' }),

  listRevisions: (assetId) => req(`/wealth/assets/${assetId}/revisions`).then((r) => r.revisions),
  addRevision: (assetId, rev) =>
    req(`/wealth/assets/${assetId}/revisions`, { method: 'POST', body: JSON.stringify(rev) }),
  updateRevision: (id, rev) =>
    req(`/wealth/revisions/${id}`, { method: 'PATCH', body: JSON.stringify(rev) }),
  deleteRevision: (id) => req(`/wealth/revisions/${id}`, { method: 'DELETE' }),

  breakdown: (filter = {}) => req(`/wealth/breakdown${qs(filter)}`).then((r) => r.breakdown),

  getSettings: () => req('/wealth/settings').then((r) => r.settings),
  updateSettings: (patch) =>
    req('/wealth/settings', { method: 'PATCH', body: JSON.stringify(patch) }).then((r) => r.settings)
};

export const CURRENCIES = [
  'USD', 'EUR', 'GBP', 'JPY', 'CNY', 'CHF', 'CAD', 'AUD', 'HKD', 'SEK', 'NOK', 'DKK'
];

export const ASSET_TYPES = [
  { id: 'money', label: 'Cash / money', icon: '💵' },
  { id: 'stock', label: 'Stock', icon: '📈' },
  { id: 'crypto', label: 'Crypto', icon: '🪙' },
  { id: 'watch', label: 'Watch', icon: '⌚' },
  { id: 'house', label: 'Real estate', icon: '🏠' },
  { id: 'vehicle', label: 'Vehicle', icon: '🚗' },
  { id: 'other', label: 'Other', icon: '📦' }
];

/** Reserved fields a wealth template may bind (feed the typed revision columns). */
export const RESERVED_FIELDS = [
  { reserved: 'price', label: 'Price', type: 'number' },
  { reserved: 'quantity', label: 'Quantity', type: 'number' }
];

export const CUSTOM_FIELD_TYPES = [
  { id: 'text', label: 'Text' },
  { id: 'number', label: 'Number' },
  { id: 'textarea', label: 'Long text' },
  { id: 'date', label: 'Date' },
  { id: 'url', label: 'URL' }
];

export function shortId() {
  return Math.random().toString(36).slice(2, 8);
}

export function assetTypeLabel(id) {
  return ASSET_TYPES.find((a) => a.id === id)?.label ?? id;
}
export function assetTypeIcon(id) {
  return ASSET_TYPES.find((a) => a.id === id)?.icon ?? '📦';
}

// Formatting lives in $lib/format.js.
export { fmtMoney } from '$lib/format.js';

export function monthLabel(iso) {
  const d = new Date(iso + 'T00:00:00');
  return d.toLocaleDateString(undefined, { month: 'short', year: '2-digit' });
}
