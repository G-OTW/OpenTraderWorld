/** Chart drawings — the model behind the drawing rail.
 *
 * A drawing is stored in **data coordinates**, never in pixels: `x` is an epoch-ms instant
 * and `y` a price. Panning, zooming, switching timeframe or loading older history all move
 * the pixels around; anchoring to time+price is what keeps a trend line on the same two
 * candles through every one of them. Chart.svelte re-projects them to pixels on each frame.
 *
 *   { id, tool, a: {x, y}, b: {x, y}|null, text?, style: { color, width } }
 *
 * `a`/`b` are the two anchors a tool needs (one for the single-point tools). Everything else
 * — handles, hit areas, fib rungs — is derived at paint time.
 */

/** Tools, in rail order. `points` is how many clicks/anchors the tool takes. */
export const DRAW_TOOLS = [
  { id: 'cursor', icon: 'cursor', points: 0, labelKey: 'histviz.draw.cursor' },
  { id: 'trend', icon: 'line-diagonal', points: 2, labelKey: 'histviz.draw.trend' },
  { id: 'hline', icon: 'line-horizontal', points: 1, labelKey: 'histviz.draw.hline' },
  { id: 'vline', icon: 'line-vertical', points: 1, labelKey: 'histviz.draw.vline' },
  { id: 'rect', icon: 'square', points: 2, labelKey: 'histviz.draw.rect' },
  { id: 'fib', icon: 'fib', points: 2, labelKey: 'histviz.draw.fib' },
  { id: 'text', icon: 'type', points: 1, labelKey: 'histviz.draw.text' },
  { id: 'long', icon: 'arrow-up', points: 2, labelKey: 'histviz.draw.long' },
  { id: 'short', icon: 'arrow-down', points: 2, labelKey: 'histviz.draw.short' },
  { id: 'ruler', icon: 'ruler', points: 2, labelKey: 'histviz.draw.ruler' }
];

/** Position tools carry a third anchor — the stop — which is a price, not a point: it runs
 *  the width of the box like the entry and the target do. */
export const isPosition = (tool) => tool === 'long' || tool === 'short';

export const drawTool = (id) => DRAW_TOOLS.find((t) => t.id === id) ?? DRAW_TOOLS[0];

/** Retracement rungs, the set every charting tool ships with. */
export const FIB_LEVELS = [0, 0.236, 0.382, 0.5, 0.618, 0.786, 1];

/** '' means "use the chart's accent" — same convention as the indicator style overrides. */
export const DEFAULT_DRAW_STYLE = { color: '', width: 0 };

let seq = 0;
export const newDrawingId = () => `d${Date.now().toString(36)}${(seq++).toString(36)}`;

/** A drawing from its anchors, ready to push into the list.
 *
 *  A position tool is dragged out entry → target; its stop starts symmetric (1:1), which is
 *  the only defensible default — anything else would put words in the trader's mouth. */
export function makeDrawing(tool, a, b = null, extra = {}) {
  const d = { id: newDrawingId(), tool, a, b, style: { ...DEFAULT_DRAW_STYLE }, ...extra };
  if (isPosition(tool) && b && d.stop == null) {
    const dist = Math.abs(b.y - a.y);
    d.b = { ...b, y: tool === 'long' ? a.y + dist : a.y - dist };
    d.stop = tool === 'long' ? a.y - dist : a.y + dist;
  }
  return d;
}

/** Move a whole drawing by a data-space delta. */
export function translate(d, dx, dy) {
  const move = (p) => (p ? { x: p.x + dx, y: p.y + dy } : null);
  const out = { ...d, a: move(d.a), b: move(d.b) };
  if (d.stop != null) out.stop = d.stop + dy;
  return out;
}

/** Move one anchor ('a' | 'b' | 'stop') of a drawing. The stop is a price only. */
export function moveAnchor(d, which, p) {
  if (which === 'stop') return { ...d, stop: p.y };
  // Entry and target keep the box's span: dragging the entry of a position carries the whole
  // setup, dragging the target only re-prices that edge.
  if (which === 'a' && isPosition(d.tool)) {
    const dy = p.y - d.a.y;
    return { ...d, a: p, b: { ...d.b, y: d.b.y + dy }, stop: d.stop + dy };
  }
  return { ...d, [which]: p };
}

/** Does this tool span two anchors? Single-anchor tools ignore `b` entirely. */
export const isTwoPoint = (tool) => drawTool(tool).points === 2;

/** The anchor an in-progress one lines up with, decided per axis in **pixels**.
 *
 *  Candidates carry both their data anchor (`x` epoch ms, `y` price) and where they are on
 *  screen (`px`, `py`); the tolerance is a distance on screen, because that is what "looks
 *  aligned" means, and a price tolerance would be meaningless across instruments.
 *
 *  Time and price are answered separately on purpose: lining a level up with a previous high
 *  is a price question and lining a vertical up with an event is a time one, so requiring
 *  both at once would make the common case unreachable. */
export function nearestAnchor(px, py, candidates, tol = 6) {
  let x = null;
  let y = null;
  let bestX = tol;
  let bestY = tol;
  for (const c of candidates) {
    if (c.px != null && Math.abs(c.px - px) < bestX) {
      bestX = Math.abs(c.px - px);
      x = c.x;
    }
    if (c.py != null && Math.abs(c.py - py) < bestY) {
      bestY = Math.abs(c.py - py);
      y = c.y;
    }
  }
  return { x, y };
}

// ── Clipboard ──
// One buffer for the whole page, so an object copied on one pane pastes onto another. It is
// deliberately in memory and not in localStorage: a copied shape is a gesture in progress,
// not something a reload should hand back.

let clipboard = null;

/** Copy an object, remembering which instrument it came from. */
export function copyDrawing(d, instrument) {
  if (!d) return false;
  clipboard = { instrument, drawing: JSON.parse(JSON.stringify(d)) };
  return true;
}

export const canPaste = () => !!clipboard;

/** The buffer as a new object, or `null` when nothing was copied.
 *
 *  Pasted onto the instrument it came from it is nudged along by `stepMs` so it does not sit
 *  invisibly on top of the original; pasted onto a different one it keeps its anchors
 *  exactly, because a level copied from one chart to another is meant to land on that price
 *  and at that instant. */
export function pasteDrawing(instrument, stepMs = 0) {
  if (!clipboard) return null;
  const src = JSON.parse(JSON.stringify(clipboard.drawing));
  const dx = clipboard.instrument === instrument ? stepMs : 0;
  return { ...translate(src, dx, 0), id: newDrawingId() };
}
