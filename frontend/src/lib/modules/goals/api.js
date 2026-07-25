/** Goals API client — goal CRUD with JSONB KPIs. Single-user. */
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

export const goalsApi = {
  list: () => req('/goals').then((r) => r.goals),
  get: (id) => req(`/goals/${id}`).then((r) => r.goal),
  add: (goal) => req('/goals', { method: 'POST', body: JSON.stringify(goal) }).then((r) => r.goal),
  update: (id, goal) => req(`/goals/${id}`, { method: 'PATCH', body: JSON.stringify(goal) }),
  remove: (id) => req(`/goals/${id}`, { method: 'DELETE' }),
  setPosition: (id, position) =>
    req(`/goals/${id}/position`, { method: 'POST', body: JSON.stringify({ position }) })
};

/** Progress 0..1 from KPIs: reached-points / total-points (0 when no points defined). */
export function progress(kpis = []) {
  let total = 0;
  let reached = 0;
  for (const k of kpis) {
    const pts = Number(k.points) || 0;
    total += pts;
    if (k.reached) reached += pts;
  }
  return total > 0 ? reached / total : 0;
}

// Formatting lives in $lib/format.js. It parses "YYYY-MM-DD" as local midnight, as this
// did, and returns an em-dash rather than null for an absent date — every call site here
// is already behind a "has a deadline" guard.
export { fmtDate } from '$lib/format.js';

/** Days until a deadline (negative if past); null when no deadline. */
export function daysLeft(iso) {
  if (!iso) return null;
  const d = new Date(iso + 'T00:00:00');
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return Math.round((d - today) / 86400000);
}

/** A fresh blank KPI row. */
export function blankKpi() {
  return { name: '', target: null, current: null, reached: false, points: 1 };
}
