/** Voice control API: settings, speech engines, spoken commands, transcription. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api/voice${path}`, {
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

const json = (method, payload) => ({ method, body: JSON.stringify(payload) });

export const voiceApi = {
  settings: () => req('/settings').then((r) => r.settings),
  saveSettings: (s) => req('/settings', json('PUT', s)).then((r) => r.settings),

  engines: () => req('/engines'),
  addEngine: (e) => req('/engines', json('POST', e)).then((r) => r.engine),
  updateEngine: (id, e) => req(`/engines/${id}`, json('PATCH', e)).then((r) => r.engine),
  deleteEngine: (id) => req(`/engines/${id}`, { method: 'DELETE' }),
  testEngine: (id) => req(`/engines/${id}/test`, { method: 'POST' }),

  commands: () => req('/commands').then((r) => r.commands),
  addCommand: (c) => req('/commands', json('POST', c)).then((r) => r.command),
  updateCommand: (id, c) => req(`/commands/${id}`, json('PATCH', c)).then((r) => r.command),
  deleteCommand: (id) => req(`/commands/${id}`, { method: 'DELETE' }),
  reorderCommands: (ids) => req('/commands/reorder', json('POST', { ids })),

  /** Send one WAV recording; resolves to `{ text, ms }`. */
  transcribe: (wav, lang = '') =>
    req(`/transcribe${lang ? `?lang=${encodeURIComponent(lang)}` : ''}`, {
      method: 'POST',
      headers: { 'content-type': 'audio/wav' },
      body: wav
    })
};
