/**
 * Mailbox module API client — connected IMAP accounts, senders, stored mail, and the
 * curated newsletter store. Single-user, session-authed like the rest.
 */
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

const qs = (params) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== null && v !== undefined && v !== '' && v !== 'all') p.set(k, v);
  }
  const s = p.toString();
  return s ? `?${s}` : '';
};

export const mailboxApi = {
  // Accounts — { accounts, presets, counts }
  overview: () => req('/mailbox/accounts'),
  createAccount: (body) =>
    req('/mailbox/accounts', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.account),
  updateAccount: (id, patch) =>
    req(`/mailbox/accounts/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.account
    ),
  deleteAccount: (id) => req(`/mailbox/accounts/${id}`, { method: 'DELETE' }),
  /** { ok, messages } or { ok: false, error } — a failed test is an answer, not an exception. */
  testAccount: (id) => req(`/mailbox/accounts/${id}/test`, { method: 'POST' }),
  pollAccount: (id) => req(`/mailbox/accounts/${id}/poll`, { method: 'POST' }),
  pollAll: () => req('/mailbox/poll', { method: 'POST' }),
  /**
   * Start a sign-in. The backend picks the flow from this instance's own address:
   * `{ flow, mode: 'code', sign_in: { authorize_url, redirect_uri } }`, or
   * `{ flow, mode: 'device', device: { user_code, verification_uri, … } }`.
   * `mode: 'device'` forces the fallback.
   */
  oauthStart: (id, mode) =>
    req(`/mailbox/accounts/${id}/oauth/start`, {
      method: 'POST',
      body: JSON.stringify({ mode: mode ?? null })
    }),
  /** Hand back what the redirect carried → { ok, error? }. */
  oauthCallback: (body) =>
    req('/mailbox/oauth/callback', { method: 'POST', body: JSON.stringify(body) }),
  /** Poll it → { status: 'pending' | 'done' | 'failed', interval?, error? }. */
  oauthPoll: (flow) =>
    req('/mailbox/oauth/poll', { method: 'POST', body: JSON.stringify({ flow }) }),

  // Senders
  listSenders: () => req('/mailbox/senders').then((r) => r.senders ?? []),
  updateSender: (id, patch) =>
    req(`/mailbox/senders/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.sender
    ),
  deleteSender: (id) => req(`/mailbox/senders/${id}`, { method: 'DELETE' }),

  // Messages
  listMessages: (filter = {}) => req(`/mailbox/messages${qs(filter)}`).then((r) => r.messages ?? []),
  /** { message, attachments, unsubscribe_url }. `images` un-parks the remote images. */
  getMessage: (id, images = false) =>
    req(`/mailbox/messages/${id}${images ? '?images=true' : ''}`),
  updateMessage: (id, patch) =>
    req(`/mailbox/messages/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteMessage: (id) => req(`/mailbox/messages/${id}`, { method: 'DELETE' }),
  readAll: (senderId = null) =>
    req('/mailbox/messages/read-all', {
      method: 'POST',
      body: JSON.stringify({ sender_id: senderId })
    }),
  /** { ok } when unsubscribed for you, { ok: false, open_url } when it needs a browser. */
  unsubscribe: (id) => req(`/mailbox/messages/${id}/unsubscribe`, { method: 'POST' }),
  remind: (id, body) =>
    req(`/mailbox/messages/${id}/remind`, { method: 'POST', body: JSON.stringify(body) }),
  attachmentUrl: (id) => `/api/mailbox/attachments/${id}`,

  // Store links
  listLinks: () => req('/mailbox/links').then((r) => r.links ?? []),
  createLink: (body) =>
    req('/mailbox/links', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.link),
  updateLink: (id, patch) =>
    req(`/mailbox/links/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.link
    ),
  deleteLink: (id) => req(`/mailbox/links/${id}`, { method: 'DELETE' }),

  // Settings
  getSettings: () => req('/mailbox/settings').then((r) => r.settings),
  putSettings: (settings) =>
    req('/mailbox/settings', { method: 'PUT', body: JSON.stringify(settings) }).then(
      (r) => r.settings
    )
};

/** Sender categories, in display order. Labels are i18n keys resolved by the caller. */
export const CATEGORIES = ['news', 'newsletter', 'broker', 'other'];

/** Store topics, in display order. */
export const TOPICS = ['mindset', 'finance', 'trading', 'geopolitics', 'economics', 'other'];
