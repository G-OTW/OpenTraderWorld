/** Time Tracker API client — projects, timers, entries, heartbeat, breakdown, settings. */
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

export const timeApi = {
  listProjects: (includeArchived = false) =>
    req(`/time/projects${includeArchived ? '?include_archived=true' : ''}`).then((r) => r.projects),
  addProject: (p) => req('/time/projects', { method: 'POST', body: JSON.stringify(p) }).then((r) => r.id),
  updateProject: (id, p) =>
    req(`/time/projects/${id}`, { method: 'PATCH', body: JSON.stringify(p) }),
  deleteProject: (id) => req(`/time/projects/${id}`, { method: 'DELETE' }),
  setPosition: (id, position) =>
    req(`/time/projects/${id}/position`, { method: 'POST', body: JSON.stringify({ position }) }),

  start: (id) => req(`/time/projects/${id}/start`, { method: 'POST' }),
  stop: (id) => req(`/time/projects/${id}/stop`, { method: 'POST' }),

  listEntries: (id, limit = 50) => req(`/time/projects/${id}/entries?limit=${limit}`).then((r) => r.entries),
  // body: { started_at, ended_at? , duration_seconds?, note? } (RFC3339 timestamps).
  createEntry: (id, body) =>
    req(`/time/projects/${id}/entries`, { method: 'POST', body: JSON.stringify(body) }).then((r) => r.id),
  deleteEntry: (id) => req(`/time/entries/${id}`, { method: 'DELETE' }),

  getState: () => req('/time/state').then((r) => r.state),
  heartbeat: () => req('/time/heartbeat', { method: 'POST' }),
  revert: () => req('/time/revert', { method: 'POST' }).then((r) => r.reverted),
  updateSettings: (patch) =>
    req('/time/settings', { method: 'PATCH', body: JSON.stringify(patch) }).then((r) => r.state),

  breakdown: (filter = {}) => req(`/time/breakdown${qs(filter)}`).then((r) => r.breakdown)
};

export const CURRENCIES = [
  'USD', 'EUR', 'GBP', 'JPY', 'CNY', 'CHF', 'CAD', 'AUD', 'HKD', 'SEK', 'NOK', 'DKK'
];

// Institutional palette — desaturated tones aligned with the chart family.
// Names/order/count preserved; only the hues are muted (no saturated primaries).
export const COLOR_SWATCHES = [
  { name: 'blue', hex: '#6f8bab' },
  { name: 'green', hex: '#7fb894' },
  { name: 'violet', hex: '#9a8db3' },
  { name: 'amber', hex: '#bfa06f' },
  { name: 'red', hex: '#c9776b' },
  { name: 'pink', hex: '#bd8a9c' },
  { name: 'slate', hex: '#7d8a99' }
];

// Formatting lives in $lib/format.js.
export { fmtDuration, fmtMoney, fmtDateTime, EM_DASH } from '$lib/format.js';

/** Compact hours, e.g. 12.5h. Specific to this module's summaries. */
export function fmtHours(h) {
  if (h === null || h === undefined) return '—';
  return `${h.toFixed(1)}h`;
}

/** Duration in seconds between an entry's start and end (end defaults to now if open). */
export function entrySeconds(e) {
  const start = new Date(e.started_at).getTime();
  const end = e.ended_at ? new Date(e.ended_at).getTime() : Date.now();
  return Math.max(0, (end - start) / 1000);
}

/**
 * Convert a `<input type="datetime-local">` value ("2026-06-20T08:30", local wall time)
 * into an RFC3339 string with the browser's UTC offset, so the server stores the instant
 * the user actually meant. Returns null for an empty/invalid value.
 */
export function localInputToRfc3339(v) {
  if (!v) return null;
  const d = new Date(v); // parsed as local time
  if (Number.isNaN(d.getTime())) return null;
  return d.toISOString(); // RFC3339 (UTC, 'Z') — same instant
}

/**
 * Budget alert level for a used/budget ratio. Returns one of:
 * '' (under 80%), 'warn' (80–94%), 'high' (95–99%), 'over' (>=100%).
 */
export function budgetLevel(usedHours, budgetHours) {
  if (!budgetHours || budgetHours <= 0) return '';
  const pct = (usedHours / budgetHours) * 100;
  if (pct >= 100) return 'over';
  if (pct >= 95) return 'high';
  if (pct >= 80) return 'warn';
  return '';
}
