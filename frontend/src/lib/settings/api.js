/** Settings API client — account, defaults, data management, logs. Single-user. */
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

export const settingsApi = {
  // Account
  me: () => req('/settings/me'),
  updateAccount: (input) =>
    req('/settings/account', { method: 'POST', body: JSON.stringify(input) }),
  logout: () => req('/settings/logout', { method: 'POST' }),

  // Defaults
  getDefaults: () => req('/settings/defaults'),
  setDefaults: (input) =>
    req('/settings/defaults', { method: 'POST', body: JSON.stringify(input) }),

  // Network exposure (bind/port/domain). Applying a change needs a host restart.
  getNetwork: () => req('/settings/network'),
  setNetwork: (input) =>
    req('/settings/network', { method: 'POST', body: JSON.stringify(input) }),

  // About
  version: () => req('/settings/version').then((r) => r.version),
  // { current, latest, update_available } — latest is null when GitHub is unreachable.
  updateCheck: () => req('/settings/update-check'),

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
  deleteMcpToken: (id) => req(`/mcp/tokens/${id}`, { method: 'DELETE' })
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
