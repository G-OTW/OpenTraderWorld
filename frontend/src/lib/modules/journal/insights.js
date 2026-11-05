// One statement, rendered.
//
// The server decides *whether* a statement fires and sends the raw numbers
// (`{key, severity, values, labels}`); the wording lives in the language packs and the
// formatting is the reader's (their locale, their display currency). This file is the
// join between the three, shared by the behaviour tab and the exposure tab so the
// `labels` convention cannot drift between them.

/** Icon per severity. `warn` = a leak to fix, `good` = something done right. */
export const SEVERITY_ICON = { warn: 'alert-triangle', good: 'check-circle', info: 'lightbulb' };

/**
 * Resolve one insight to its sentence.
 *
 * `fmt` maps a value name to a formatter; anything it does not name falls back to
 * `fallback`. Positional `labels` (a ticker, a pair of them) interpolate as `{0}`, `{1}`:
 * they are names, not numbers, so no formatter touches them.
 */
export function insightText(i, t, prefix, fmt = {}, fallback = String) {
  const params = {};
  for (const [k, v] of Object.entries(i.values ?? {})) params[k] = (fmt[k] ?? fallback)(v);
  (i.labels ?? []).forEach((label, n) => {
    params[n] = label;
  });
  return t(`${prefix}.${i.key}`, params);
}
