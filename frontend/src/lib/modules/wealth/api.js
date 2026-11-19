/** MyWealth API client: templates, assets, revisions, net-worth breakdown, settings, and the
 *  goals, movers, snapshots, targets, attachments and export built on them. */
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
    req('/wealth/settings', { method: 'PATCH', body: JSON.stringify(patch) }).then((r) => r.settings),
  /** Goals, stale notice and CPI series, written as a whole. */
  savePreferences: (prefs) =>
    req('/wealth/preferences', { method: 'PUT', body: JSON.stringify(prefs) }).then((r) => r.settings),

  schedule: (assetId) => req(`/wealth/assets/${assetId}/schedule`),
  goals: () => req('/wealth/goals'),
  movers: (from, to) => req(`/wealth/movers${qs({ from, to })}`),
  snapshot: (at) => req(`/wealth/snapshot${qs({ at })}`),
  targets: () => req('/wealth/targets'),
  saveTargets: (targets) => req('/wealth/targets', { method: 'PUT', body: JSON.stringify({ targets }) }),
  categories: () => req('/wealth/categories').then((r) => r.categories),
  setLiquid: (name, liquid) =>
    req('/wealth/categories', { method: 'PUT', body: JSON.stringify({ name, liquid }) }).then((r) => r.categories),
  real: (seriesId) => req(`/wealth/real${qs({ series_id: seriesId })}`),
  channels: () => req('/wealth/channels').then((r) => r.channels),
  /** The Subscriptions module's 12-month forecast, read as a recurring cost. */
  subscriptionsForecast: () => req('/subscriptions/forecast'),

  attachments: (assetId) => req(`/wealth/assets/${assetId}/attachments`).then((r) => r.attachments),
  /** Raw upload: the body is the file. */
  async upload(assetId, file, revisionId = null) {
    const p = new URLSearchParams({ name: file.name });
    if (revisionId) p.set('revision_id', revisionId);
    const res = await fetch(`/api/wealth/assets/${assetId}/attachments?${p}`, {
      method: 'POST',
      headers: { 'content-type': file.type || 'application/octet-stream' },
      body: file
    });
    let body = null;
    try {
      body = await res.json();
    } catch {
      /* empty */
    }
    redirectIfUnauthorized(res);
    if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
    return body.id;
  },
  deleteAttachment: (id) => req(`/wealth/attachments/${id}`, { method: 'DELETE' }),
  /** Fetch a file and hand it to the browser as a download; a refusal comes back as an error. */
  download: (path, name) => downloadFile(`/api${path}`, name),
  attachmentPath: (id) => `/wealth/attachments/${id}`
};

async function downloadFile(url, name) {
  const res = await fetch(url);
  redirectIfUnauthorized(res);
  if (!res.ok) {
    let msg = `request failed (${res.status})`;
    try {
      msg = (await res.json())?.error ?? msg;
    } catch {
      /* empty */
    }
    throw new Error(msg);
  }
  const blob = await res.blob();
  const href = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = href;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(href), 1000);
}

export const CURRENCIES = [
  'USD', 'EUR', 'GBP', 'JPY', 'CNY', 'CHF', 'CAD', 'AUD', 'HKD', 'SEK', 'NOK', 'DKK'
];

/** What a wealth asset can be.
 *
 * `stock` and `crypto` are `legacy`: older rows still open and still render, but they are no
 * longer offered when creating one. Anything with a price series belongs to the Portfolio
 * Tracker, and a wealth asset points at a portfolio (`portfolio`) instead of holding a copy
 * of its value that is wrong the next morning.
 *
 * `loan` is the liability side. A net worth without debt is not a net worth.
 *
 * `broker_account` and `wallet` are read live: a broker account's equity, a public address's
 * balance on chain. A broker account is created from the link dialog; a wallet from the form. */
export const ASSET_TYPES = [
  { id: 'money', label: 'Cash / money', icon: '💵' },
  { id: 'portfolio', label: 'Portfolio (linked)', icon: '🔗', linked: true },
  { id: 'loan', label: 'Loan / debt', icon: '🏦', liability: true },
  { id: 'watch', label: 'Watch', icon: '⌚' },
  { id: 'house', label: 'Real estate', icon: '🏠' },
  { id: 'vehicle', label: 'Vehicle', icon: '🚗' },
  { id: 'other', label: 'Other', icon: '📦' },
  { id: 'stock', label: 'Stock', icon: '📈', legacy: true },
  { id: 'crypto', label: 'Crypto', icon: '🪙', legacy: true },
  { id: 'wallet', label: 'Wallet address', icon: '👛' },
  { id: 'broker_account', label: 'Broker account (linked)', icon: '📊', linked: true }
];

/** Chains a wallet address can be read on. */
export const CHAINS = [
  { id: 'btc', label: 'Bitcoin (BTC)' },
  { id: 'eth', label: 'Ethereum (ETH)' }
];

/** The types offered when creating an asset by hand. A legacy type still renders, and a
 *  linked one is created by the link flow rather than typed. */
export const CREATABLE_TYPES = ASSET_TYPES.filter((a) => !a.legacy && !a.linked);

/** Default sign for a type: a loan is owed, everything else is owned. */
export const signOf = (id) => (ASSET_TYPES.find((a) => a.id === id)?.liability ? -1 : 1);

/** Suggested review interval in days: something priced by a market does not need one, a
 *  house or a watch does. 0 means never nag. */
export const REVIEW_DEFAULTS = { house: 365, watch: 365, vehicle: 365, other: 180, loan: 90 };

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
