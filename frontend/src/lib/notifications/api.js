/** Notification broker API client — external channels shared by every notifying module.
 *
 * A channel is a destination the user owns (their SMTP account, their Telegram bot, a
 * Slack/Discord webhook): a config, one secret plugged from the vault, and the list of
 * modules allowed to push to it (`modules`, where `['*']` means all). The secret is
 * write-only — the server only ever reports whether one is set. */
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

export const channelsApi = {
  /** Module ids a channel can be granted to, in display order. */
  modules: () => req('/notif-channels/modules').then((r) => r.modules),

  /** Channels. `module` restricts to what that module may push to; omit for all. */
  list: (module = null) =>
    req(`/notif-channels${module ? `?module=${encodeURIComponent(module)}` : ''}`).then(
      (r) => r.channels
    ),

  /** payload: { kind, name, config, enabled, modules?, secret_vault_item? } */
  create: (payload) =>
    req('/notif-channels', { method: 'POST', body: JSON.stringify(payload) }).then(
      (r) => r.channel
    ),
  update: (id, payload) =>
    req(`/notif-channels/${id}`, { method: 'PATCH', body: JSON.stringify(payload) }),
  remove: (id) => req(`/notif-channels/${id}`, { method: 'DELETE' }),
  test: (id) => req(`/notif-channels/${id}/test`, { method: 'POST' })
};

/** Grant value meaning "every notifying module, present and future". */
export const ALL_MODULES = '*';

/** Whether a channel may be pushed to on behalf of `module`. */
export function allows(channel, module) {
  const m = channel?.modules ?? [];
  return m.includes(ALL_MODULES) || m.includes(module);
}

/** A channel is ready to send once its credential is plugged in. */
export const isReady = (c) => !!c?.has_secret;

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
    // `vaultable`: the field may hold a vault item instead of a literal. The id is stored
    // under `<key>_vault_item` and resolved server-side at send time.
    fields: [
      { key: 'chat_id', label: 'Chat ID', placeholder: '123456789', required: true, vaultable: true }
    ]
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
export const kindLabel = (id) => channelMeta(id)?.label ?? id;
