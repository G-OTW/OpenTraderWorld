/** ToDo API client — flat task-list CRUD plus a done toggle. Single-user. */
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

export const todosApi = {
  list: () => req('/todos').then((r) => r.todos),
  get: (id) => req(`/todos/${id}`).then((r) => r.todo),
  add: (todo) => req('/todos', { method: 'POST', body: JSON.stringify(todo) }).then((r) => r.todo),
  update: (id, todo) => req(`/todos/${id}`, { method: 'PATCH', body: JSON.stringify(todo) }),
  setDone: (id, done) =>
    req(`/todos/${id}/done`, { method: 'PATCH', body: JSON.stringify({ done }) }),
  remove: (id) => req(`/todos/${id}`, { method: 'DELETE' })
};

// Formatting lives in $lib/format.js. It parses "YYYY-MM-DD" as local midnight, as this
// did; the display site is guarded by {#if td.due_date}, so the em-dash never shows.
export { fmtDate } from '$lib/format.js';

/** Relative urgency of a due date: 'overdue' | 'today' | 'soon' | 'later' | null. */
export function dueState(iso) {
  if (!iso) return null;
  const d = new Date(iso + 'T00:00:00');
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const days = Math.round((d - today) / 86400000);
  if (days < 0) return 'overdue';
  if (days === 0) return 'today';
  if (days <= 3) return 'soon';
  return 'later';
}
