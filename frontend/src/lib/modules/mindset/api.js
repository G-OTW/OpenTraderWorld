/** Mindset API client.
 *
 * The module has three pages over one dataset. **Check-in** (`/mindset`) is the day view:
 * the active templates grouped by category, each rendering its prompts as controls.
 * **Templates** (`/mindset/templates`) is the authoring view: categories and full CRUD on a
 * template's name, description, notes, phase and prompt list. **History** charts the past.
 *
 * A template is a named set of prompts bound to a phase of the trading day (`pre` before
 * the session, `post` after). Answers are one map per (date, template), keyed by prompt id.
 * Consistency day marks work exactly as they do on the routines board. */
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
  history: (limit = 60) => req(`/mindset/history?limit=${limit}`),

  saveEntry: (date, templateId, answers) =>
    req('/mindset/entries', {
      method: 'PUT',
      body: JSON.stringify({ date, template_id: templateId, answers })
    }).then((r) => r.entry),
  deleteEntry: (date, templateId) =>
    req('/mindset/entries', {
      method: 'DELETE',
      body: JSON.stringify({ date, template_id: templateId })
    }),

  // Templates. `listTemplates` returns [{ template, prompts }] so the library can show
  // prompt counts without a detail call per row.
  listTemplates: () => req('/mindset/templates'),
  templateDetail: (id) => req(`/mindset/templates/${id}`),
  createTemplate: (body) =>
    req('/mindset/templates', { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.template
    ),
  updateTemplate: (id, patch) =>
    req(`/mindset/templates/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.template
    ),
  deleteTemplate: (id) => req(`/mindset/templates/${id}`, { method: 'DELETE' }),
  duplicateTemplate: (id, name) =>
    req(`/mindset/templates/${id}/duplicate`, {
      method: 'POST',
      body: JSON.stringify({ name })
    }).then((r) => r.template),
  clearTemplates: () => req('/mindset/templates', { method: 'DELETE' }),
  resetTemplates: () => req('/mindset/templates/reset', { method: 'POST' }).then((r) => r.templates),

  // Categories — same shape the shared CategoryManager expects.
  listCategories: () => req('/mindset/categories').then((r) => r.categories),
  createCategory: (name, color) =>
    req('/mindset/categories', { method: 'POST', body: JSON.stringify({ name, color }) }).then(
      (r) => r.category
    ),
  updateCategory: (id, patch) =>
    req(`/mindset/categories/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.category
    ),
  deleteCategory: (id) => req(`/mindset/categories/${id}`, { method: 'DELETE' }),

  // Single-prompt edits, for tweaks that shouldn't resubmit a whole template.
  listPrompts: () => req('/mindset/prompts').then((r) => r.prompts),
  addPrompt: (body) =>
    req('/mindset/prompts', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.prompt),
  updatePrompt: (id, patch) =>
    req(`/mindset/prompts/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.prompt
    ),
  deletePrompt: (id) => req(`/mindset/prompts/${id}`, { method: 'DELETE' }),

  // Consistency day marks.
  listMarks: (from, to) => req(`/mindset/marks?from=${from}&to=${to}`).then((r) => r.marks),
  setMark: (date, mark) =>
    req('/mindset/marks', { method: 'PUT', body: JSON.stringify({ date, mark }) })
};

/** Phases of the trading day, in order. A template belongs to exactly one; the day view
 *  sorts by it, so the morning check-ins always come before the evening ones.
 *
 *  Same three-way split as the routines board — `live` is the session break, the moment a
 *  trader most needs to be asked whether they are still following the plan. */
export const PHASES = [
  { key: 'pre', icon: '☀️' },
  { key: 'live', icon: '📈' },
  { key: 'post', icon: '🌙' }
];

/** The four control types a prompt can render as. Unchanged from the pre-template module —
 *  the builder just presents them better. */
export const KINDS = [
  { key: 'scale', label: 'Scale 1–5', icon: 'bar-chart', needsOptions: false },
  { key: 'choice', label: 'Single choice', icon: 'check-circle', needsOptions: true },
  { key: 'tags', label: 'Multi tags', icon: 'tag', needsOptions: true },
  { key: 'text', label: 'Free text', icon: 'text-quote', needsOptions: false }
];

export function kindOf(key) {
  return KINDS.find((k) => k.key === key) ?? KINDS[0];
}

/** Whether an answer counts as filled — shared by the card counter and the history summary,
 *  so "3/7 answered" and "not filled" never disagree. */
export function isAnswered(v) {
  return v != null && v !== '' && !(Array.isArray(v) && v.length === 0);
}

/** Suggested option sets offered in the builder, so a choice/tags prompt doesn't start from
 *  an empty box. */
export function optionPresets(tr) {
  return [
    { id: 'moods', label: tr('mindset.builder.presetMoods'), options: ['😞', '😕', '😐', '🙂', '😄'] },
    {
      id: 'states',
      label: tr('mindset.builder.presetStates'),
      options: ['calm', 'confident', 'disciplined', 'anxious', 'impatient', 'FOMO', 'tired', 'distracted']
    },
    { id: 'yesno', label: tr('mindset.builder.presetYesNo'), options: ['Yes', 'Partly', 'No'] },
    {
      id: 'process',
      label: tr('mindset.builder.presetProcess'),
      options: ['All process', 'One slip', 'Several', 'Tilted']
    }
  ];
}

// Re-exported so mindset components import their consistency vocabulary from one place,
// the same way the routines module does.
export {
  MARK_CYCLE,
  nextMark,
  MARK_COLORS,
  CATEGORY_COLORS,
  fmtLocal,
  todayStr,
  parseLocal,
  weekdayIndex,
  periodRange,
  periodStats,
  PERIODS
} from '$lib/ui/consistency.js';
