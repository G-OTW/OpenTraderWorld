/** Settings API client — account, defaults, data management, logs. Single-user. */
import { redirectIfUnauthorized } from '$lib/auth.js';
import { shouldRetryAfterReauth } from '$lib/reauth.svelte.js';

/**
 * `retry` is false on the second pass: a step-up refusal raises the password prompt once,
 * and if the retry is refused too, the refusal is real and belongs to the caller.
 */
async function req(path, options = {}, retry = true) {
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
  if (!res.ok) {
    // A sensitive route refused for want of a recent password: prompt, then run it again.
    // Handled here rather than at each call site so every guarded action behaves the same.
    if (retry && (await shouldRetryAfterReauth(res.status, body))) {
      return req(path, options, false);
    }
    const err = new Error(body?.error ?? `request failed (${res.status})`);
    // `reauth_required` and `totp_required` are answered by prompting, not by showing.
    err.code = body?.code ?? null;
    err.status = res.status;
    throw err;
  }
  return body;
}

export const settingsApi = {
  // Account
  me: () => req('/settings/me'),
  updateAccount: (input) =>
    req('/settings/account', { method: 'POST', body: JSON.stringify(input) }),
  logout: () => req('/settings/logout', { method: 'POST' }),

  // Step-up re-authentication. Unlocks the sensitive actions (minting an MCP token,
  // changing the network mode, exporting credentials, writing a vault item, turning the
  // second factor off) for a few minutes, tied to this session.
  // Whether this session already holds a grant, without spending one.
  reauthStatus: () => req('/settings/reauth'),
  reauth: (password, code = '') =>
    req('/settings/reauth', {
      method: 'POST',
      body: JSON.stringify({ password, code: code || null })
    }),

  // Active sessions
  listSessions: () => req('/settings/sessions'),
  revokeSession: (id) =>
    req('/settings/sessions/revoke', { method: 'POST', body: JSON.stringify({ id }) }),
  revokeOtherSessions: () => req('/settings/sessions/revoke-others', { method: 'POST' }),

  // Second factor (TOTP)
  totpStatus: () => req('/settings/totp'),
  totpEnroll: () => req('/settings/totp/enroll', { method: 'POST' }),
  totpConfirm: (code) =>
    req('/settings/totp/confirm', { method: 'POST', body: JSON.stringify({ code }) }),
  totpDisable: () => req('/settings/totp', { method: 'DELETE' }),

  // How long one request may take before it is stopped. Applies immediately.
  getRequestTimeout: () => req('/settings/request-timeout'),
  setRequestTimeout: (seconds) =>
    req('/settings/request-timeout', { method: 'POST', body: JSON.stringify({ seconds }) }),

  // Defaults
  getDefaults: () => req('/settings/defaults'),
  setDefaults: (input) =>
    req('/settings/defaults', { method: 'POST', body: JSON.stringify(input) }),

  // Network exposure (bind/port/domain). Applying a change needs a host restart.
  getNetwork: () => req('/settings/network'),
  setNetwork: (input) =>
    req('/settings/network', { method: 'POST', body: JSON.stringify(input) }),

  // Scheduled backup, as reported by the host-side backup script. `{ configured: false }`
  // on an install that backs up by hand, which is the normal case and not an error.
  backupStatus: () => req('/settings/backup/status'),

  // About
  version: () => req('/settings/version').then((r) => r.version),
  // { current, latest, update_available } — latest is null when GitHub is unreachable.
  updateCheck: () => req('/settings/update-check'),

  // Move your data: a per-module zip out, and the same zip back in. Separate from the
  // whole-database backup, which stays a host-level pg_dump.
  exportUrl: ({ modules = [], credentials = false } = {}) => {
    const q = new URLSearchParams();
    if (modules.length) q.set('modules', modules.join(','));
    if (credentials) q.set('credentials', 'true');
    const qs = q.toString();
    return `/api/settings/data/export${qs ? `?${qs}` : ''}`;
  },
  // What loading this file would meet, before loading it: exact counts from the file's
  // manifest against what is here now. Reads only; nothing is written.
  previewData: ({ file }) => {
    const form = new FormData();
    form.append('file', file);
    return req('/settings/data/preview', { method: 'POST', body: form, headers: {} });
  },
  importData: ({ file, modules = [], mode = 'merge' }) => {
    const form = new FormData();
    form.append('file', file);
    if (modules.length) form.append('modules', modules.join(','));
    form.append('mode', mode);
    // No content-type header: the browser sets the multipart boundary itself.
    return req('/settings/data/import', { method: 'POST', body: form, headers: {} });
  },

  // Data management
  dataUsage: () => req('/settings/data'),
  wipeModule: (module) =>
    req('/settings/data/wipe', { method: 'POST', body: JSON.stringify({ module }) }),

  // Logs
  logs: ({ level, search, limit } = {}) => {
    const q = new URLSearchParams();
    if (level) q.set('level', level);
    if (search) q.set('search', search);
    if (limit) q.set('limit', String(limit));
    const qs = q.toString();
    return req(`/settings/logs${qs ? `?${qs}` : ''}`).then((r) => r.logs);
  },
  clearLogs: () => req('/settings/logs', { method: 'DELETE' }),
  getLogLevel: () => req('/settings/logs/level'),
  setLogLevel: (level) =>
    req('/settings/logs/level', { method: 'POST', body: JSON.stringify({ level }) }),

  // API rate tracking — per-provider outbound-call volume + over-limit events.
  rateUsage: (days = 1) => req(`/rate/usage?days=${days}`),

  // MCP — global toggle + bearer tokens for AI agents (plaintext returned once on create).
  mcpSettings: () => req('/mcp/settings'),
  setMcpEnabled: (enabled) =>
    req('/mcp/settings', { method: 'POST', body: JSON.stringify({ enabled }) }),
  mcpTokens: () => req('/mcp/tokens').then((r) => r.tokens),
  createMcpToken: (input) =>
    req('/mcp/tokens', { method: 'POST', body: JSON.stringify(input) }),
  updateMcpToken: (id, input) =>
    req(`/mcp/tokens/${id}`, { method: 'PATCH', body: JSON.stringify(input) }),
  deleteMcpToken: (id) => req(`/mcp/tokens/${id}`, { method: 'DELETE' }),

  // External control: chat channels that drive OTW back.
  controlSettings: () => req('/control/settings'),
  /** `pairTtl` is optional: omitted, the stored pairing lifetime is left alone. */
  setControlEnabled: (enabled, pairTtl) =>
    req('/control/settings', {
      method: 'POST',
      body: JSON.stringify({ enabled, ...(pairTtl == null ? {} : { pair_ttl_minutes: pairTtl }) })
    }),
  controlBindings: () => req('/control/bindings').then((r) => r.bindings),
  createControlBinding: (input) =>
    req('/control/bindings', { method: 'POST', body: JSON.stringify(input) }),
  updateControlBinding: (id, input) =>
    req(`/control/bindings/${id}`, { method: 'PATCH', body: JSON.stringify(input) }),
  deleteControlBinding: (id) => req(`/control/bindings/${id}`, { method: 'DELETE' }),
  pairControlBinding: (id) => req(`/control/bindings/${id}/pair`, { method: 'POST' }),
  unpairControlSender: (id, senderId) =>
    req(`/control/bindings/${id}/senders/${encodeURIComponent(senderId)}`, { method: 'DELETE' })
};

/** Human-readable byte size. */
export function fmtBytes(n) {
  if (n == null) return '—';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}
