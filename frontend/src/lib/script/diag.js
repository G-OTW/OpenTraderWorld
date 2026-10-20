/** Engine diagnostics (codes + args) to UI text. The engine never words a message: the UI
 *  translates `script.diag.<Code>` with the args as `{a0}`, `{a1}`, …. */

export function diagText(t, d) {
  const params = Object.fromEntries((d.args ?? []).map((a, i) => [`a${i}`, a]));
  // Phase 0 = not planned: its own sentence rather than "planned for phase 0".
  const key = d.code === 'Unsupported' && String(d.args?.[1]) === '0' ? 'UnsupportedNever' : d.code;
  const msg = t(`script.diag.${key}`, params);
  const line = d.span?.line;
  return line ? msg + t('script.diag.atLine', { line }) : msg;
}

/** Warnings grouped by code + feature: `{code, feature, count, lines}`. A `SkippedVisual`
 *  already counts its occurrences (args[1]); anything else counts one per diagnostic. */
export function groupWarnings(warnings) {
  const groups = new Map();
  for (const d of warnings ?? []) {
    const key = `${d.code}\u0000${d.args?.[0] ?? ''}`;
    let g = groups.get(key);
    if (!g) groups.set(key, (g = { code: d.code, feature: d.args?.[0] ?? '', count: 0, lines: [], first: d }));
    g.count += d.code === 'SkippedVisual' ? Number(d.args?.[1] ?? 1) || 1 : 1;
    if (d.span?.line) g.lines.push(d.span.line);
  }
  return [...groups.values()].map(({ first, ...g }) => ({ ...g, diag: first }));
}

/** "line 3" or "lines 3, 9". */
export const linesText = (t, lines) =>
  t(lines.length === 1 ? 'histviz.script.line' : 'histviz.script.lines', { lines: lines.join(', ') });
