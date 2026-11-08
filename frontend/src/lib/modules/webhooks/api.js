/** Webhooks API client — inbound endpoints redirecting payloads to a module. */
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

export const webhooksApi = {
  /** → { webhooks, targets } */
  list: () => req('/webhooks'),
  /** → { webhook, token, path } — plaintext token is returned exactly once. */
  create: (input) => req('/webhooks', { method: 'POST', body: JSON.stringify(input) }),
  update: (id, input) =>
    req(`/webhooks/${id}`, { method: 'PATCH', body: JSON.stringify(input) }).then((r) => r.webhook),
  remove: (id) => req(`/webhooks/${id}`, { method: 'DELETE' }),
  events: (id) => req(`/webhooks/${id}/events`).then((r) => r.events)
};
