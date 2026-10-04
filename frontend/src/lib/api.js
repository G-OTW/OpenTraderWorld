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
    const err = new Error(message);
    // The server tags refusals the client has to *act* on rather than merely display:
    // `totp_required`, `setup_token_required`, `reauth_required`, `cross_origin`. Carrying
    // the tag on the Error is what lets a caller branch without matching on prose.
    err.code = body?.code ?? null;
    err.status = res.status;
    throw err;
  }
  return body;
}

export const api = {
  setupStatus: () => request('/setup/status'),
  /** Aggregate service health: { status, services: { core, postgres } }. */
  health: () => request('/health'),
  /**
   * Create the first admin. `setupToken` is required only on a network-facing instance,
   * where core prints it to the log at startup so the wizard cannot be claimed by whoever
   * loads the page first.
   */
  createAdmin: (username, password, setupToken = '') =>
    request('/setup', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
      headers: {
        'content-type': 'application/json',
        ...(setupToken ? { 'x-otw-setup-token': setupToken } : {})
      }
    }),
  /**
   * Sign in. `code` is the TOTP digits; leave it empty on the first attempt — an account
   * with a second factor answers `totp_required` and the caller asks for it then.
   */
  login: (username, password, code = '') =>
    request('/login', {
      method: 'POST',
      body: JSON.stringify({ username, password, code: code || null })
    }),
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
