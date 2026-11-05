/** Chart templates: a chart's studies and how it is laid out (type, sub-pane heights, style),
 *  saved under a name and applied to any chart. They live in the global chart settings
 *  (`chartTemplates`), so every workspace offers the same list.
 *
 *  The brick size is not carried: it is a price step of one instrument, so a template opens
 *  a Renko of another symbol on its automatic size. */

const clone = (v) => (v == null ? v : JSON.parse(JSON.stringify(v)));

/** A template of `pane` (a workspace pane, its studies in `pane.instances`). */
export function templateOf(pane, name) {
  return {
    id: `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
    name: name.trim(),
    type: pane.type ?? 'candlestick',
    instances: clone(pane.instances ?? []),
    paneHeights: clone(pane.paneHeights ?? {}),
    settings: clone(pane.settings ?? null)
  };
}

/** Give `pane` the template's studies and layout. The studies keep the template's ids, so the
 *  sub-pane heights (keyed by them) still match. Coordinates and links are the pane's own. */
export function applyTemplate(pane, tpl) {
  pane.type = tpl.type ?? 'candlestick';
  pane.brick = 0;
  pane.instances = clone(tpl.instances ?? []);
  pane.paneHeights = clone(tpl.paneHeights ?? {});
  pane.settings = clone(tpl.settings ?? null);
  return pane;
}

/** The stored list, whatever the settings blob held. */
export const templatesIn = (settings) =>
  Array.isArray(settings?.chartTemplates) ? settings.chartTemplates.filter((t) => t?.id && t?.name) : [];
