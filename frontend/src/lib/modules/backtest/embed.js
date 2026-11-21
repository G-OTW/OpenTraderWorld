/** Which custom indicators a strategy reads, and their embedded copies: pure, so the run
 *  payload rules are testable without the API client. Re-exported by `api.js`. */

const clone = (v) => JSON.parse(JSON.stringify(v));

/** Collect the ids of every custom-indicator operand referenced anywhere in a settings object. */
export function usedCustomIndicatorIds(settings) {
  const ids = new Set();
  const scanOperand = (o) => o?.kind === 'custom_indicator' && o.id && ids.add(o.id);
  const scanGroup = (g) =>
    (g?.conditions ?? []).forEach((c) => {
      scanOperand(c.left);
      scanOperand(c.right);
    });
  for (const side of [settings.long, settings.short]) {
    if (!side) continue;
    scanGroup(side.entry);
    scanGroup(side.exit);
  }
  for (const rule of [...(settings.dca?.buys ?? []), ...(settings.dca?.sells ?? [])]) {
    scanGroup(rule.condition);
  }
  return [...ids];
}

/** `inputs` without the keys the library script no longer declares (an input removed or
 *  renamed since the strategy was saved): the run refuses an unknown input, and the form
 *  shows only the declared ones, so a stale key could not be cleared. */
function liveInputs(inputs, row) {
  const declared = row.meta?.script?.inputs;
  const out = clone(inputs ?? {});
  if (!Array.isArray(declared)) return out;
  const keys = new Set(declared.map((i) => i.key));
  for (const k of Object.keys(out)) if (!keys.has(k)) delete out[k];
  return out;
}

/** Embed the custom indicators a strategy uses, keyed by id, so a run/save is self-contained
 *  and reproducible: node graphs into `settings.indicators`, scripts into `settings.scripts`
 *  as `{ name, source, inputs }`. A script's name and source come from the library row, its
 *  input values stay the strategy's own; a script whose row is gone keeps its embedded copy.
 *  `library` = [{ id, kind, name, definition, source }]. */
export function embedIndicators(settings, library) {
  const out = clone(settings);
  const byId = new Map(library.map((i) => [i.id, i]));
  const defs = {};
  const scripts = {};
  for (const id of usedCustomIndicatorIds(out)) {
    const row = byId.get(id);
    const had = settings?.scripts?.[id];
    if (row?.kind === 'script') scripts[id] = { name: row.name, source: row.source, inputs: liveInputs(had?.inputs, row) };
    else if (row) defs[id] = row.definition;
    else if (had) scripts[id] = clone(had);
  }
  out.indicators = defs;
  out.scripts = scripts;
  return out;
}

/** Ids of the scripts embedded in `settings` that an operand reads. */
export function usedScriptIds(settings) {
  return usedCustomIndicatorIds(settings).filter((id) => settings?.scripts?.[id]);
}

/** The plots of a script an operand may read, in plot order: a plot whose title several plots
 *  share is ambiguous (`duplicate`), one that reads the future cannot be backtested
 *  (`noncausal`, with the builtin and line in `args` when the metadata names them). */
export function pickablePlots(meta) {
  const plots = meta?.plots ?? [];
  const count = new Map();
  for (const p of plots) count.set(p.title, (count.get(p.title) ?? 0) + 1);
  return plots.map((p) => {
    if (count.get(p.title) > 1) return { title: p.title, disabled: true, reason: 'duplicate' };
    if (!p.causal)
      return { title: p.title, disabled: true, reason: 'noncausal', ...(p.taint ? { args: { ...p.taint } } : {}) };
    return { title: p.title, disabled: false };
  });
}

/** The plot a new script operand reads: the first one it may, or null. */
export const defaultPlot = (meta) => pickablePlots(meta).find((p) => !p.disabled)?.title ?? null;
