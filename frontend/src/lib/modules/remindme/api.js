/** RemindMe API client — reminder CRUD plus the notifications surface. Single-user. */
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

export const remindApi = {
  // Reminders
  list: () => req('/reminders').then((r) => r.reminders),
  get: (id) => req(`/reminders/${id}`).then((r) => r.reminder),
  add: (r) => req('/reminders', { method: 'POST', body: JSON.stringify(r) }).then((x) => x.reminder),
  update: (id, r) => req(`/reminders/${id}`, { method: 'PATCH', body: JSON.stringify(r) }),
  remove: (id) => req(`/reminders/${id}`, { method: 'DELETE' }),

  // Notifications
  notifications: (limit = 200) => req(`/notifications?limit=${limit}`), // { notifications, unread }
  unreadSince: (since) =>
    req(`/notifications/unread${since ? `?since=${encodeURIComponent(since)}` : ''}`), // { notifications, unread }
  ackAll: () => req('/notifications/ack-all', { method: 'POST' }),
  markRead: (id) => req(`/notifications/${id}/read`, { method: 'POST' }),
  removeNotif: (id) => req(`/notifications/${id}`, { method: 'DELETE' }),

  // External notification channels (email / telegram / slack / discord)
  channels: () => req('/notif-channels').then((r) => r.channels),
  addChannel: (c) =>
    req('/notif-channels', { method: 'POST', body: JSON.stringify(c) }).then((x) => x.channel),
  updateChannel: (id, c) =>
    req(`/notif-channels/${id}`, { method: 'PATCH', body: JSON.stringify(c) }),
  removeChannel: (id) => req(`/notif-channels/${id}`, { method: 'DELETE' }),
  testChannel: (id) => req(`/notif-channels/${id}/test`, { method: 'POST' })
};

/**
 * Channel kinds and the shape of each one's setup. `fields` are the non-secret config
 * inputs; `secret` describes the single credential (write-only, never returned). All are
 * free to integrate — the user brings their own account and pays any provider fees.
 */
export const CHANNEL_KINDS = [
  {
    id: 'email',
    label: 'Email',
    secret: { key: 'password', label: 'SMTP password', help: 'App password recommended.' },
    fields: [
      { key: 'host', label: 'SMTP host', placeholder: 'smtp.gmail.com', required: true },
      { key: 'port', label: 'Port', placeholder: '587', type: 'number' },
      { key: 'from', label: 'From address', placeholder: 'you@example.com', required: true },
      { key: 'to', label: 'Send to', placeholder: 'you@example.com', required: true },
      { key: 'username', label: 'Username', placeholder: '(defaults to From)' }
    ]
  },
  {
    id: 'telegram',
    label: 'Telegram',
    secret: { key: 'token', label: 'Bot token', help: 'From @BotFather.' },
    fields: [{ key: 'chat_id', label: 'Chat ID', placeholder: '123456789', required: true }]
  },
  {
    id: 'slack',
    label: 'Slack',
    secret: {
      key: 'webhook',
      label: 'Incoming Webhook URL',
      help: 'Slack → Apps → Incoming Webhooks.'
    },
    fields: []
  },
  {
    id: 'discord',
    label: 'Discord',
    secret: {
      key: 'webhook',
      label: 'Webhook URL',
      help: 'Channel → Edit → Integrations → Webhooks.'
    },
    fields: []
  }
];

export const channelMeta = (id) => CHANNEL_KINDS.find((k) => k.id === id);

export const KINDS = [
  { id: 'custom', label: 'Custom' },
  { id: 'goal', label: 'Goal' },
  { id: 'todo', label: 'ToDo' }
];

export const FREQUENCIES = [
  { id: 'once', label: 'Once' },
  { id: 'daily', label: 'Daily' },
  { id: 'weekly', label: 'Weekly' },
  { id: 'monthly', label: 'Monthly' },
  { id: 'yearly', label: 'Yearly' }
];

export const freqLabel = (id) => FREQUENCIES.find((f) => f.id === id)?.label ?? id;
export const kindLabel = (id) => KINDS.find((k) => k.id === id)?.label ?? id;

/** The in-app route a notification's linked item points at (or the reminders list). */
export function linkFor(n) {
  if (n.kind === 'goal' && n.linked_id) return '/goals';
  if (n.kind === 'todo' && n.linked_id) return '/todos';
  return '/remindme';
}

// fmtDate lives in $lib/format.js. The one call site guards with `?? '—'`, which the
// shared em-dash return makes redundant but not wrong.
export { fmtDate } from '$lib/format.js';

/**
 * "Mar 14, 14:32" — day and time, no year. Deliberately not the shared fmtDateTime
 * (which includes the year) nor fmtTime (which omits the day): a reminder list is
 * scanned within the current year, and both parts have to fit one narrow column.
 */
export function fmtTime(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  });
}
