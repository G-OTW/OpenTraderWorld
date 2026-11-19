/** Mindset on the journal calendar: which check-ins a book reads, and what each day shows.
 *
 * The pick is a user preference per book: `{ [categoryId]: [templateId, …] }`. A day's
 * score is the Mindset module's own reading (`normalize`, mean of the scored answers of
 * the picked templates), so the calendar and the mindset dashboard never disagree.
 *
 * Like the routine dot, a miss is about a traded day: a check-in skipped on a day with no
 * trade is not a miss. */
import { normalize } from '$lib/modules/mindset/scoring.js';

export const MINDSET_PREF = 'otw.journal.calendar.mindset.v1';

/**
 * @param hist  `/api/mindset/history` payload for the window: { prompts, templates, entries }
 * @param ids   picked template ids
 * @returns { templates, byDate } where `templates` are the picked ones still active and
 *          `byDate` maps 'YYYY-MM-DD' to { filled: Set<templateId>, score: number|null }.
 */
export function mindsetByDate(hist, ids) {
  const picked = new Set(ids ?? []);
  const templates = (hist?.templates ?? []).filter((tpl) => picked.has(tpl.id) && tpl.active !== false);
  const live = new Set(templates.map((tpl) => tpl.id));
  const promptById = new Map((hist?.prompts ?? []).map((p) => [p.id, p]));

  const acc = new Map();
  for (const e of hist?.entries ?? []) {
    if (!live.has(e.template_id)) continue;
    const date = String(e.entry_date).slice(0, 10);
    if (!acc.has(date)) acc.set(date, { filled: new Set(), values: [] });
    const day = acc.get(date);
    day.filled.add(e.template_id);
    for (const [pid, v] of Object.entries(e.answers ?? {})) {
      const p = promptById.get(pid);
      const x = p ? normalize(p, v) : null;
      if (x != null) day.values.push(x);
    }
  }

  const byDate = new Map();
  for (const [date, d] of acc) {
    const score = d.values.length ? d.values.reduce((a, b) => a + b, 0) / d.values.length : null;
    byDate.set(date, { filled: d.filled, score });
  }
  return { templates, byDate };
}

/** What one cell shows, or null for nothing. `tone`: done | partial | miss | none. */
export function mindsetCell(read, date, hasTrade) {
  const total = read.templates.length;
  if (total === 0) return null;
  const day = read.byDate.get(date);
  const done = day ? read.templates.filter((tpl) => day.filled.has(tpl.id)).length : 0;
  if (done === 0 && !hasTrade) return null;
  const tone = !hasTrade ? 'none' : done === total ? 'done' : done === 0 ? 'miss' : 'partial';
  return {
    tone,
    done,
    total,
    score: day?.score == null ? null : Math.round(day.score),
    rows: read.templates.map((tpl) => ({ id: tpl.id, name: tpl.name, done: !!day?.filled.has(tpl.id) }))
  };
}
