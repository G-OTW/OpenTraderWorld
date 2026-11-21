/** Mindset scoring: turns raw check-in answers into one comparable number per day.
 *
 * A prompt declares how it counts through `config.score`:
 *   asc   a higher scale value, or a later option, is better
 *   desc  a lower scale value, or an earlier option, is better
 *   none  not scored (tags, free text, and any choice left unconfigured)
 * A scale prompt with no `score` field counts `asc`, which is what the starter set means
 * ("Sleep quality", "Discipline"). Every scored answer lands on 0..100 where 100 is the
 * best end, so questions phrased either way can be averaged together.
 *
 * Everything here is pure: the history page feeds it the `/api/mindset/history` payload
 * and the chosen range, and renders what comes back. */

import { docText } from './text.js';

export const SCORE_MODES = ['none', 'asc', 'desc'];

/** Minimum scored days before trends, bands and outliers are shown at all. */
export const MIN_DAYS = 5;

export function scoreMode(prompt) {
  const s = prompt?.config?.score;
  if (SCORE_MODES.includes(s)) return prompt.kind === 'scale' || prompt.kind === 'choice' ? s : 'none';
  return prompt?.kind === 'scale' ? 'asc' : 'none';
}

/** One answer on 0..100 (100 = best), or null when the prompt or answer is not scorable. */
export function normalize(prompt, answer) {
  const mode = scoreMode(prompt);
  if (mode === 'none') return null;
  let x = null;
  if (prompt.kind === 'scale') {
    if (typeof answer !== 'number' || !Number.isFinite(answer)) return null;
    x = ((Math.min(5, Math.max(1, answer)) - 1) / 4) * 100;
  } else if (prompt.kind === 'choice') {
    const opts = prompt.config?.options ?? [];
    const i = opts.indexOf(answer);
    if (i < 0 || opts.length < 2) return null;
    x = (i / (opts.length - 1)) * 100;
  }
  if (x == null) return null;
  return mode === 'desc' ? 100 - x : x;
}

const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);
const std = (xs) => {
  if (xs.length < 2) return 0;
  const m = mean(xs);
  return Math.sqrt(xs.reduce((a, x) => a + (x - m) ** 2, 0) / (xs.length - 1));
};
const round = (x) => (x == null ? null : Math.round(x));

function addDays(iso, n) {
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}
function dayDiff(a, b) {
  return Math.round((new Date(`${b}T00:00:00Z`) - new Date(`${a}T00:00:00Z`)) / 86400000);
}

/** Consecutive logged days, weekend-tolerant: a gap of up to 3 calendar days keeps the
 *  streak (Friday to Monday). Returns { current, best }. */
function streaks(dates) {
  if (dates.length === 0) return { current: 0, best: 0 };
  let best = 1;
  let run = 1;
  for (let i = 1; i < dates.length; i++) {
    run = dayDiff(dates[i - 1], dates[i]) <= 3 ? run + 1 : 1;
    best = Math.max(best, run);
  }
  return { current: run, best };
}

/**
 * @param hist   `/api/mindset/history` payload: { prompts, templates, entries }
 * @param range  { from, to } inclusive ISO dates for the dashboard window; entries before
 *               `from` (back to the same span again) feed the "vs previous" delta.
 */
export function buildDashboard(hist, range) {
  const prompts = hist?.prompts ?? [];
  const promptById = new Map(prompts.map((p) => [p.id, p]));
  const scored = prompts.filter((p) => scoreMode(p) !== 'none');

  // Per day: every scored answer by prompt and phase, plus the day's tags and free text
  // for the word and tag reads. A day with no scored answer has no score and is dropped.
  const byDay = new Map();
  const dayOf = (date) => {
    if (!byDay.has(date)) byDay.set(date, { date, answers: [], tags: [], texts: [] });
    return byDay.get(date);
  };
  for (const e of hist?.entries ?? []) {
    for (const [pid, v] of Object.entries(e.answers ?? {})) {
      const p = promptById.get(pid);
      if (!p) continue;
      if (p.kind === 'tags' && Array.isArray(v) && v.length) dayOf(e.entry_date).tags.push(...v);
      if (p.kind === 'text' && typeof v === 'string' && v.trim()) {
        dayOf(e.entry_date).texts.push({ promptId: pid, label: p.label, text: v.trim() });
      }
      const x = normalize(p, v);
      if (x == null) continue;
      dayOf(e.entry_date).answers.push({ promptId: pid, phase: e.phase, value: x, raw: v });
    }
  }
  for (const n of hist?.notes ?? []) {
    const text = docText(n.content).trim();
    if (text) dayOf(n.day).texts.push({ promptId: 'note', label: '', text });
  }
  const allDays = [...byDay.values()]
    .filter((d) => d.answers.length > 0)
    .map((d) => ({
      ...d,
      score: mean(d.answers.map((a) => a.value)),
      phases: Object.fromEntries(
        ['pre', 'live', 'post'].map((ph) => [
          ph,
          mean(d.answers.filter((a) => a.phase === ph).map((a) => a.value))
        ])
      )
    }))
    .sort((a, b) => a.date.localeCompare(b.date));

  const span = dayDiff(range.from, range.to) + 1;
  const prevFrom = addDays(range.from, -span);
  const days = allDays.filter((d) => d.date >= range.from && d.date <= range.to);
  const prev = allDays.filter((d) => d.date >= prevFrom && d.date < range.from);

  const scores = days.map((d) => d.score);
  const m = mean(scores);
  const sd = std(scores);
  const prevMean = mean(prev.map((d) => d.score));
  const enough = days.length >= MIN_DAYS;

  const best = days.length ? days.reduce((a, d) => (d.score > a.score ? d : a)) : null;
  const worst = days.length ? days.reduce((a, d) => (d.score < a.score ? d : a)) : null;

  // Days outside the mean ± 1 std band, strongest deviation first.
  const extremes = enough
    ? days
        .filter((d) => Math.abs(d.score - m) >= sd && sd > 0)
        .map((d) => ({ ...d, delta: d.score - m, dir: d.score > m ? 'high' : 'low' }))
        .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta))
        .slice(0, 8)
    : [];

  // Split the window's logged days in two halves to read progress.
  const half = Math.floor(days.length / 2);
  const firstHalf = days.slice(0, half);
  const secondHalf = days.slice(half);

  const questions = scored
    .map((p) => {
      const series = days
        .map((d) => {
          const hit = d.answers.filter((a) => a.promptId === p.id);
          return hit.length ? { date: d.date, value: mean(hit.map((a) => a.value)), raw: hit[0].raw } : null;
        })
        .filter(Boolean);
      const vals = series.map((s) => s.value);
      const qm = mean(vals);
      const qsd = std(vals);
      const f = mean(series.filter((s) => s.date < (secondHalf[0]?.date ?? '')).map((s) => s.value));
      const s2 = mean(series.filter((s) => s.date >= (secondHalf[0]?.date ?? '')).map((s) => s.value));
      const delta = f != null && s2 != null ? s2 - f : null;
      const irregular =
        series.length >= MIN_DAYS
          ? series
              .filter((s) => Math.abs(s.value - qm) >= Math.max(1.5 * qsd, 25))
              .map((s) => ({ ...s, promptId: p.id, mean: qm, delta: s.value - qm }))
          : [];
      return {
        prompt: p,
        n: series.length,
        mean: qm,
        std: qsd,
        first: f,
        second: s2,
        delta,
        trend: delta == null ? null : delta >= 5 ? 'up' : delta <= -5 ? 'down' : 'flat',
        series,
        byDate: new Map(series.map((s) => [s.date, s])),
        irregular
      };
    })
    .filter((q) => q.n > 0)
    .sort((a, b) => (a.delta ?? 0) - (b.delta ?? 0));

  const irregular = questions
    .flatMap((q) => q.irregular)
    .sort((a, b) => b.date.localeCompare(a.date))
    .slice(0, 10);

  const overallFirst = mean(firstHalf.map((d) => d.score));
  const overallSecond = mean(secondHalf.map((d) => d.score));

  const dates = days.map((d) => d.date);
  const logged = streaks(dates);
  // Good streak: consecutive logged days at or above the window's mean, ending today-most.
  let good = 0;
  for (let i = days.length - 1; i >= 0 && days[i].score >= m; i--) good++;

  return {
    scoredPrompts: scored.length,
    days,
    enough,
    mean: m,
    std: sd,
    prevMean,
    delta: m != null && prevMean != null ? m - prevMean : null,
    best,
    worst,
    extremes,
    questions,
    irregular,
    overall: {
      first: overallFirst,
      second: overallSecond,
      delta: overallFirst != null && overallSecond != null ? overallSecond - overallFirst : null
    },
    streak: { ...logged, good },
    span,
    prevFrom,
    round
  };
}

/** Pearson correlation of paired samples; null below 3 pairs or with no variance. */
export function pearson(pairs) {
  if (pairs.length < 3) return null;
  const xs = pairs.map((p) => p[0]);
  const ys = pairs.map((p) => p[1]);
  const mx = mean(xs);
  const my = mean(ys);
  let num = 0;
  let dx = 0;
  let dy = 0;
  for (let i = 0; i < pairs.length; i++) {
    num += (xs[i] - mx) * (ys[i] - my);
    dx += (xs[i] - mx) ** 2;
    dy += (ys[i] - my) ** 2;
  }
  if (dx === 0 || dy === 0) return null;
  return num / Math.sqrt(dx * dy);
}

export { mean as meanOf, round as roundOf, addDays };
