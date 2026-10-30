// Background jobs, one list for the Automator's Jobs tab. Each job is configured where its
// module configures it (a journal's broker sync, a portfolio's, a mailbox); this is the
// shared view: pause, interval, run now, delete a schedule, and the run log.
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

// Job ids carry a colon (`journal-sync:<uuid>`): encoded, they stay one path segment.
const path = (id) => `/jobs/${encodeURIComponent(id)}`;

export const jobsApi = {
  list: () => req('/jobs').then((r) => r.jobs),
  update: (id, patch) => req(path(id), { method: 'PATCH', body: JSON.stringify(patch) }),
  remove: (id) => req(path(id), { method: 'DELETE' }),
  run: (id) => req(`${path(id)}/run`, { method: 'POST' }),
  runs: (job, limit = 100) => {
    const q = new URLSearchParams({ limit: String(limit) });
    if (job) q.set('job', job);
    return req(`/jobs/runs?${q}`).then((r) => r.runs);
  }
};

/** Broker sync intervals offered by the modals and the Jobs tab, in minutes. */
export const INTERVALS = [15, 30, 60, 240, 720, 1440, 10080];

/** "Every 4 h", in the viewer's language. `t` is the resolved `$t`. */
export function fmtInterval(minutes, t) {
  const m = Number(minutes) || 0;
  if (m >= 1440 && m % 1440 === 0) return t('jobs.every.days', { n: m / 1440 });
  if (m >= 60 && m % 60 === 0) return t('jobs.every.hours', { n: m / 60 });
  return t('jobs.every.minutes', { n: m });
}

/** Select options; an interval set elsewhere (a mailbox's) is kept in the list. */
export function intervalOptions(t, current) {
  const c = Number(current);
  const list = !c || INTERVALS.includes(c) ? INTERVALS : [...INTERVALS, c].sort((a, b) => a - b);
  return list.map((m) => ({ value: m, label: fmtInterval(m, t) }));
}
