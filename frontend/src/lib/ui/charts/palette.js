/**
 * Colour roles for the shared chart layer. Pure functions over the object
 * `chartColors()` returns (`$lib/theme/chart.svelte.js`), so every caller reads the
 * palette inside a `$derived` and a theme flip repaints:
 *
 *   const c = $derived(chartColors());
 *   const fill = $derived(identityColor(i, c));
 *
 * Identity (categories, symbols, portfolios) follows one fixed order, validated with
 * the dataviz validator against the light theme (see README.md):
 *
 *   slot 0 --chart-1 blue, slot 1 --chart-5 amber, slot 2 --chart-6 purple,
 *   slot 3 --chart-2 green, past that everything is "Other" in --chart-4 slate.
 *
 * --chart-3 (red) is never an identity slot: red means loss. The palette never
 * cycles: a fifth entity folds into "Other" instead of reusing slot 0.
 *
 * Colour follows the entity, not its rank: pass a stable index (position in a
 * stable list, never the sorted position) so filtering never repaints survivors.
 */

/** Indexes into `colors.series` (0 based: --chart-1 is 0). */
export const IDENTITY_SLOTS = [0, 4, 5, 1];
/** Two extension slots (--chart-7 teal, --chart-8 olive). They FAIL the validator
 *  (chroma floor, normal-vision floor next to green): only for a part-to-whole that
 *  must show 5 or 6 named parts, where legend rows carry the identity. */
export const EXTENDED_SLOTS = [6, 7];
/** "Other" / fold colour: --chart-4 slate. */
export const OTHER_SLOT = 3;
/** Most named identities a chart may show (4 validated + 2 extended). */
export const MAX_IDENTITIES = 6;

/** Text that replaces a figure while the privacy screen is on. */
export const MASK = '••••';

const seriesAt = (colors, i) => colors?.series?.[i] ?? colors?.muted;

/**
 * Identity colour for a stable index. Past `slots` (default 4, the validated set; at
 * most 6 with the extension) it returns the fold colour, so callers that forget to
 * fold still never reuse a hue.
 */
export function identityColor(index, colors, slots = IDENTITY_SLOTS.length) {
  const n = Math.max(0, Math.min(MAX_IDENTITIES, slots));
  if (!Number.isInteger(index) || index < 0 || index >= n) return otherColor(colors);
  const order = [...IDENTITY_SLOTS, ...EXTENDED_SLOTS];
  return seriesAt(colors, order[index]);
}

/** The "Other" colour. */
export function otherColor(colors) {
  return seriesAt(colors, OTHER_SLOT);
}

/** Sign tone: gain --green, loss --red, zero / null / NaN --muted. `invert` flips it
 *  for a figure where down is good (a cost, a drawdown recovery). */
export function toneColor(value, colors, invert = false) {
  if (value == null || !Number.isFinite(value) || value === 0) return colors?.muted;
  const up = invert ? value < 0 : value > 0;
  return up ? colors?.green : colors?.red;
}

/** Same rule as toneColor, as a word: 'up' | 'down' | 'flat'. */
export function toneOf(value, invert = false) {
  if (value == null || !Number.isFinite(value) || value === 0) return 'flat';
  return (invert ? value < 0 : value > 0) ? 'up' : 'down';
}

/**
 * Rank `items` by `valueKey` (descending, stable) and keep the first `max`; the tail
 * is summed into one "Other" item. Nothing is folded when the list already fits.
 *
 *   foldOther(rows, 4, 'label', 'value', 'Other')
 *   -> [...top4, { label: 'Other', value: sum, other: true, items: tail }]
 *
 * Non-finite values count as 0. The input is never mutated; kept items are the
 * caller's own objects, so their `color` / `key` fields travel with them.
 */
export function foldOther(items = [], max = 4, key = 'label', valueKey = 'value', otherLabel = 'Other') {
  const val = (it) => {
    const v = Number(it?.[valueKey]);
    return Number.isFinite(v) ? v : 0;
  };
  const ranked = items
    .map((it, i) => ({ it, i }))
    .sort((a, b) => val(b.it) - val(a.it) || a.i - b.i)
    .map((r) => r.it);
  const keep = Math.max(0, Math.floor(max));
  if (ranked.length <= keep) return ranked;
  const head = ranked.slice(0, keep);
  const tail = ranked.slice(keep);
  const sum = tail.reduce((s, it) => s + val(it), 0);
  return [...head, { [key]: otherLabel, [valueKey]: sum, other: true, items: tail }];
}

/**
 * Parts for a part-to-whole mark (Segmented, Donut): positive values only, folded to
 * `max` named parts plus Other, each with its colour and share (0..1).
 * A part carrying its own `color` keeps it (stable entity colour); otherwise it takes
 * `identityColor(colorIndex ?? rank)`.
 */
export function partsOf(items = [], { max = 4, otherLabel = 'Other', colors } = {}) {
  const cap = Math.max(1, Math.min(MAX_IDENTITIES, max));
  const positive = items.filter((it) => Number.isFinite(Number(it?.value)) && Number(it.value) > 0);
  const folded = foldOther(positive, cap, 'label', 'value', otherLabel);
  const total = folded.reduce((s, p) => s + Number(p.value), 0);
  return {
    total,
    parts: folded.map((p, rank) => ({
      key: p.key ?? (p.other ? '__other' : p.label),
      label: p.label,
      value: Number(p.value),
      share: total > 0 ? Number(p.value) / total : 0,
      other: !!p.other,
      color: p.other ? otherColor(colors) : (p.color ?? identityColor(p.colorIndex ?? rank, colors, cap))
    }))
  };
}

/**
 * A translucent version of any colour the palette hands out: #rgb, #rrggbb, #rrggbbaa
 * or rgb()/rgba(). `withAlpha` from chart.svelte.js only handles #rrggbb; a canvas
 * silently keeps its previous fill when handed an invalid colour, so anything else
 * here comes back unchanged rather than broken.
 */
export function fade(color, alpha) {
  const a = Math.max(0, Math.min(1, Number(alpha)));
  const s = String(color ?? '').trim();
  let m = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(s);
  if (m) return `rgba(${m.slice(1, 4).map((h) => parseInt(h + h, 16)).join(', ')}, ${a})`;
  m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})(?:[0-9a-f]{2})?$/i.exec(s);
  if (m) return `rgba(${m.slice(1, 4).map((h) => parseInt(h, 16)).join(', ')}, ${a})`;
  m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(s);
  if (m) return `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${a})`;
  return s;
}
