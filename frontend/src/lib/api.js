/** Thin fetch wrapper for the core API (proxied at /api by Caddy). */
import { goto } from '$app/navigation';

/** Paths that are part of the auth handshake and must not trigger a redirect on 401. */
const PUBLIC_PATHS = ['/login', '/setup', '/setup/status', '/health'];

/**
 * Send the browser to the login screen when a protected request comes back 401.
 * Centralized here so an expired/cleared session bounces the user out from anywhere.
 */
export function handleUnauthorized() {
  if (typeof window !== 'undefined' && window.location.pathname !== '/login') {
    goto('/login');
  }
}

async function request(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* no JSON body */
  }
  if (res.status === 401 && !PUBLIC_PATHS.includes(path)) {
    handleUnauthorized();
  }
  if (!res.ok) {
    const message = body?.error ?? `request failed (${res.status})`;
    throw new Error(message);
  }
  return body;
}

export const api = {
  setupStatus: () => request('/setup/status'),
  /** Aggregate service health: { status, services: { core, postgres } }. */
  health: () => request('/health'),
  createAdmin: (username, password) =>
    request('/setup', { method: 'POST', body: JSON.stringify({ username, password }) }),
  login: (username, password) =>
    request('/login', { method: 'POST', body: JSON.stringify({ username, password }) }),
  /** True if the current session cookie is valid (does not redirect on failure). */
  isAuthenticated: async () => {
    try {
      const res = await fetch('/api/settings/me', { headers: { 'content-type': 'application/json' } });
      return res.ok;
    } catch {
      return false;
    }
  }
};
