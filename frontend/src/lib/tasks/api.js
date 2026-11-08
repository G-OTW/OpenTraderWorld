// Broker imports sent to the background. The import modals ask for an estimate first;
// past a few seconds they offer to run the pull on the server while the user does
// something else, and take the answer back when they are opened again.
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

export const tasksApi = {
  /** Running and finished tasks, optionally under one scope (`journal:<id>`, `taxcalc`…). */
  list: (scope = null) =>
    req(`/tasks${scope ? `?scope=${encodeURIComponent(scope)}` : ''}`).then((r) => r.tasks),
  /** A finished task with its answer. Taken once: the server forgets it after. */
  take: (id) => req(`/tasks/${id}/take`, { method: 'POST' }).then((r) => r.task),
  /** Stop a running task, or dismiss a finished one. */
  cancel: (id) => req(`/tasks/${id}`, { method: 'DELETE' }),
  /** { seconds, long } for a pull: `{ kind: 'executions', from, to, symbols }` or
   * `{ kind: 'holdings' }`. */
  estimate: (accountId, body) =>
    req(`/brokers/${accountId}/estimate`, { method: 'POST', body: JSON.stringify(body) })
};
