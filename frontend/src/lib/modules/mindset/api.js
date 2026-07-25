/** Mindset API client.
 *
 * Two check-ins per trading day: a pre-mortem (before the session) and a post-mortem (after).
 * Each is a set of prompts — scale 1–5, single choice, multi tags, free text — answered into
 * one JSONB map per (date, phase). Prompts are fully customizable; `/day` seeds a trader
 * starter set on first use. */
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

export const mindsetApi = {
  day: (date) => req(`/mindset/day${date ? `?date=${date}` : ''}`),
  saveEntry: (date, phase, answers) =>
    req('/mindset/entries', {
      method: 'PUT',
      body: JSON.stringify({ date, phase, answers })
    }).then((r) => r.entry),
  deleteEntry: (date, phase) =>
    req('/mindset/entries', { method: 'DELETE', body: JSON.stringify({ date, phase }) }),
  history: (limit = 60) => req(`/mindset/history?limit=${limit}`),

  listPrompts: () => req('/mindset/prompts').then((r) => r.prompts),
  addPrompt: (body) =>
    req('/mindset/prompts', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.prompt),
  updatePrompt: (id, patch) =>
    req(`/mindset/prompts/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.prompt
    ),
  deletePrompt: (id) => req(`/mindset/prompts/${id}`, { method: 'DELETE' }),
  clearPrompts: () => req('/mindset/prompts', { method: 'DELETE' }),
  resetPrompts: () => req('/mindset/prompts/reset', { method: 'POST' }).then((r) => r.prompts)
};

export const PHASES = [
  {
    key: 'pre',
    label: 'Pre-mortem',
    icon: '☀️',
    hint: 'Before the session — set the state, spot the risks.'
  },
  {
    key: 'post',
    label: 'Post-mortem',
    icon: '🌙',
    hint: 'After the session — grade the process, not the PnL.'
  }
];

export const KINDS = [
  { key: 'scale', label: 'Scale 1–5' },
  { key: 'choice', label: 'Single choice' },
  { key: 'tags', label: 'Multi tags' },
  { key: 'text', label: 'Free text' }
];
