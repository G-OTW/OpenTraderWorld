/** News module API client — feeds, items, secrets, live stream. Single-user. */
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

export const newsApi = {
  // Feeds
  listFeeds: () => req('/feeds').then((r) => r.feeds),
  getFeed: (id) => req(`/feeds/${id}`), // { feed, secret_names }
  createFeed: (feed) => req('/feeds', { method: 'POST', body: JSON.stringify(feed) }).then((r) => r.feed),
  updateFeed: (id, patch) => req(`/feeds/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteFeed: (id) => req(`/feeds/${id}`, { method: 'DELETE' }),
  refreshFeed: (id) => req(`/feeds/${id}/refresh`, { method: 'POST' }), // { new_items }
  refreshAll: () => req('/feeds/refresh-all', { method: 'POST' }), // { new_items }

  // Dashboards
  listDashboards: () => req('/feed-dashboards').then((r) => r.dashboards),
  createDashboard: (name) =>
    req('/feed-dashboards', { method: 'POST', body: JSON.stringify({ name }) }).then((r) => r.dashboard),
  updateDashboard: (id, patch) =>
    req(`/feed-dashboards/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteDashboard: (id) => req(`/feed-dashboards/${id}`, { method: 'DELETE' }),
  setDefaultDashboard: (id) => req(`/feed-dashboards/${id}/default`, { method: 'POST' }),
  refreshDashboard: (id) => req(`/feed-dashboards/${id}/refresh`, { method: 'POST' }), // { new_items }
  dashboardSources: (id) => req(`/feed-dashboards/${id}/sources`).then((r) => r.feeds),
  // Add a source: pass { feed_id } to link an existing one, or { kind, config,
  // name, interval_secs, force } to create. Without force, an identical source
  // returns { duplicate: { feed_id, feed_name, dashboard_names } }.
  addDashboardSource: (id, body) =>
    req(`/feed-dashboards/${id}/sources`, { method: 'POST', body: JSON.stringify(body) }),
  removeDashboardSource: (id, feedId) =>
    req(`/feed-dashboards/${id}/sources/${feedId}`, { method: 'DELETE' }),

  // Secrets (write-only; values never returned)
  listSecrets: (id) => req(`/feeds/${id}/secrets`).then((r) => r.secret_names),
  setSecret: (id, name, value) =>
    req(`/feeds/${id}/secrets`, { method: 'POST', body: JSON.stringify({ name, value }) }),
  deleteSecret: (id, name) =>
    req(`/feeds/${id}/secrets/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  // Items
  listItems: (filter = {}) => {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(filter)) {
      if (v !== undefined && v !== null && v !== '') qs.set(k, v);
    }
    const q = qs.toString();
    return req(`/feed-items${q ? `?${q}` : ''}`).then((r) => r.items);
  },
  sources: () => req('/feed-sources'), // { source_names, source_types }

  /** Subscribe to live feed events. Returns the EventSource (close it on cleanup). */
  stream(onEvent) {
    const es = new EventSource('/api/feeds/stream');
    es.addEventListener('feed', (e) => {
      try {
        onEvent(JSON.parse(e.data));
      } catch {
        /* ignore malformed */
      }
    });
    return es;
  }
};
