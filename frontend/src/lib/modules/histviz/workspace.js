/** The workspace model: a grid of panes.
 *
 * **Rows x columns, not a list of named layouts.** A vertical split is 1xN, a horizontal one
 * is Nx1, and everything in between (2x2, 3x2, 3x4) is the same model, so there is nothing to
 * keep in sync when a shape is added. The server caps it at 3x4 = twelve panes, which is the
 * shape of a screen rather than an arbitrary number.
 *
 * A pane holds coordinates and how they are drawn. What it deliberately does NOT hold is the
 * indicator/drawing board: that belongs to the *instrument* and is stored server-side under
 * its coordinates, so the same symbol opens the same way in any pane, in any workspace, on
 * any browser. */

export const MAX_ROWS = 3;
export const MAX_COLS = 4;
export const MAX_PANES = MAX_ROWS * MAX_COLS;

let seq = 0;
/** Pane ids only have to be unique inside one workspace and stable across a save. */
function paneId() {
  seq += 1;
  return `p${Date.now().toString(36)}${seq.toString(36)}`;
}

export function newPane(coords = null) {
  return {
    id: paneId(),
    coords,
    type: 'candlestick',
    brick: 0,
    instances: [],
    live: false,
    connector_id: null,
    /** Link group colour, or '' for a pane that follows nobody. */
    link: ''
  };
}

/** A fresh single-pane workspace: what the page opens with the first time. */
export function newWorkspace(name = 'Workspace') {
  return {
    id: null,
    name,
    rows: 1,
    cols: 1,
    panes: [newPane()],
    row_sizes: [1],
    col_sizes: [1],
    active: null
  };
}

/** Normalize whatever the server (or an older client) stored into the shape above. Missing
 *  fields are filled rather than refused: a workspace is the user's screen, and losing it
 *  because a field was added would be the worst possible answer. */
export function fromRow(row) {
  const rows = clamp(row?.grid_rows ?? 1, 1, MAX_ROWS);
  const cols = clamp(row?.grid_cols ?? 1, 1, MAX_COLS);
  const panes = (Array.isArray(row?.panes) ? row.panes : [])
    .slice(0, MAX_PANES)
    .map((p) => ({ ...newPane(p?.coords ?? null), ...p, id: p?.id ?? paneId() }));
  const settings = row?.settings ?? {};
  return {
    id: row?.id ?? null,
    name: row?.name ?? 'Workspace',
    rows,
    cols,
    panes: panes.length ? panes : [newPane()],
    row_sizes: sizes(settings.row_sizes, rows),
    col_sizes: sizes(settings.col_sizes, cols),
    active: settings.active ?? null
  };
}

/** The body the workspaces API takes. Panes travel whole (the server round-trips the JSON),
 *  minus the transient bits: a workspace that reopens with every pane already live would
 *  spend the account's connections before the user asked for anything. */
export function toBody(ws) {
  return {
    name: ws.name,
    rows: ws.rows,
    cols: ws.cols,
    panes: ws.panes.map((p) => ({
      id: p.id,
      coords: p.coords,
      type: p.type,
      brick: p.brick,
      instances: p.instances ?? [],
      link: p.link ?? '',
      connector_id: p.connector_id ?? null
    })),
    settings: { row_sizes: ws.row_sizes, col_sizes: ws.col_sizes, active: ws.active }
  };
}

/** Grow or shrink the grid. Panes are never destroyed by a reshape: a smaller grid simply
 *  stops showing the extras, and growing it back brings them out again in the same order. */
export function reshape(ws, rows, cols) {
  ws.rows = clamp(rows, 1, MAX_ROWS);
  ws.cols = clamp(cols, 1, MAX_COLS);
  const cells = ws.rows * ws.cols;
  while (ws.panes.length < cells) ws.panes.push(newPane(inheritCoords(ws)));
  ws.row_sizes = sizes(ws.row_sizes, ws.rows);
  ws.col_sizes = sizes(ws.col_sizes, ws.cols);
  if (!ws.panes.some((p) => p.id === ws.active)) ws.active = ws.panes[0]?.id ?? null;
  return ws;
}

/** The panes the grid actually shows: the first rows x cols, in reading order. */
export const visiblePanes = (ws) => ws.panes.slice(0, ws.rows * ws.cols);

/** A new pane opens on the instrument the active one is showing: a second timeframe of the
 *  same symbol is the commonest split there is, and an empty pane would make the user pick
 *  the instrument they are already looking at. */
function inheritCoords(ws) {
  const from = ws.panes.find((p) => p.id === ws.active) ?? ws.panes[ws.panes.length - 1];
  return from?.coords ? { ...from.coords } : null;
}

/** Fractions for N tracks, kept summing to N so `fr` units read as "share of the grid". */
export function sizes(list, n) {
  const out = Array.isArray(list) ? list.slice(0, n).map((x) => (Number(x) > 0 ? Number(x) : 1)) : [];
  while (out.length < n) out.push(1);
  const total = out.reduce((a, b) => a + b, 0) || n;
  return out.map((x) => (x * n) / total);
}

/** Move a splitter: `delta` is a fraction of the whole axis, taken from one track and given
 *  to the next. Neither track may collapse, which is what keeps a drag from making a pane
 *  unreachable. */
export function resizeTrack(list, index, delta, min = 0.15) {
  const out = list.slice();
  const n = out.length;
  const a = out[index] + delta * n;
  const b = out[index + 1] - delta * n;
  if (a < min || b < min) return list;
  out[index] = a;
  out[index + 1] = b;
  return out;
}

const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, Math.round(Number(n) || lo)));

/** The shapes offered in the picker: every rectangle up to 3x4, plus the shorthand names a
 *  trader actually says ("split vertically" is 1x2). */
export function shapes() {
  const out = [];
  for (let r = 1; r <= MAX_ROWS; r++) {
    for (let c = 1; c <= MAX_COLS; c++) out.push({ rows: r, cols: c, label: `${r}x${c}` });
  }
  return out;
}
