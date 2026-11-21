# Shared chart layer

One set of charts for Portfolios, Watchlists, Subscriptions and MyWealth. Colours come
from `chartColors()` inside a `$derived` (a theme flip repaints), figures go through the
caller's `format` and are masked (`••••`) under the privacy screen (`sensitive`, default
true; Delta defaults to false). Text props have English defaults: pass `$t(...)` strings.

## Palette (`palette.js`)

Identity order: `--chart-1` blue, `--chart-5` amber, `--chart-6` purple, `--chart-2` green,
then everything folds into "Other" in `--chart-4` slate. `--chart-3` red is never an
identity. Never cycle. Colour follows the entity: pass a stable index or `color`, never the
sorted rank. Single series: `--accent`, or `tone: 'sign'` (green / red) for a P/L.

- `identityColor(index, colors, slots = 4)`: fold colour past `slots` (max 6: slots 5 and 6
  are `--chart-7`, `--chart-8`, which fail the validator; legend rows must carry identity).
- `otherColor(c)`, `toneColor(v, c, invert)`, `toneOf(v, invert)` ('up' | 'down' | 'flat').
- `foldOther(items, max = 4, key = 'label', valueKey = 'value', otherLabel = 'Other')`.
- `partsOf(items, { max, otherLabel, colors })`, `fade(color, alpha)`, `MASK`.

`scale.js`: `niceTicks`, `indexTo100`, `toSeconds`, `alignSeries`, `splitEstimated`,
`extent`, `linear`, `barPath`, `changePct`, `textWidth` (tested in `tests/ui-charts.test.js`).

Validator (`dataviz/scripts/validate_palette.js`, 2026-11-06, light surface `#fdfdfe`):
- `#356fd6,#b87516,#8a6cb1,#15866a` (4 identities, adjacent): ALL CHECKS PASS (worst
  CVD 10.2, normal 19.7, contrast >= 3:1).
- Same 4, `--pairs all`: FAIL (blue vs purple normal 12.0, CVD 6.7): keep a legend on.
- 4 + Other `#74808b`: FAIL by design (slate is the gray fold, chroma 0.022; Other vs green
  CVD 5.0). Other is always last, labelled, separated by the 2px gap.
- Dark (`#081118`), 4 identities: CVD, normal-vision and contrast PASS; lightness band
  FAILS (amber, purple, green above L 0.67). Theme tokens were out of scope.

## Components

- `ChartFrame`: `title`, `subtitle`, `legend` [{ key?, label, color, kind?: 'line'|'rect',
  dashed?, value? }], `height` (body min height), `loading`, `unavailable` { reason,
  title?, icon?, action?: { label, onclick? | href?, icon? } }, `emphasized` (bindable),
  `toggle`, `sensitive`, snippets `actions`, `children`. Frameless (sits in your card).
- `TimeSeries` (uPlot): `series` [{ key?, label, points: [[ts, value, note?]], color?,
  dashed?, tone?: 'sign', area?, width?, step? }], `bands` [{ label, lower, upper, color? }],
  `baseline`, `area`, `indexed`, `format`, `axisFormat`, `xFormat`, `xAxisFormat`,
  `change` ('pct'|'abs'|'none'), `height` (240), `legend` ('auto'|'none'),
  `estimateLabel`, `emphasized` (bindable), `sensitive`, `ariaLabel`, `onHover(point|null)`.
  `step` holds each value until the next point (a price that changes on dates).
  A truthy `note` marks an estimate (dashed from the last measured point; a string note is
  shown in the tooltip). `null` value = gap. Snapping crosshair, tooltip with change vs
  start, 2px surface-ring marker, legend from 2 entries, legend click emphasises.
- `Bars` (SVG): `categories`, `series` [{ key?, label, values, color?, colors? (per bar),
  dashed? (bool | bool[]: estimate), tone?: 'sign' }], `lines` (overlay, vertical; `width` px per line, 1 for a reference hairline),
  `stacked` (true), `horizontal`, `format`, `axisFormat`, `labels` ('ends'|'none'),
  `height` (240), `rowHeight` (32), `legend`, `totalLabel`, `estimateLabel`,
  `emphasized`, `sensitive`, `ariaLabel`, `onSelect(index)`, `tipExtra(index)` (extra
  tooltip rows `[{ label, value }]`, formatted and masked by the caller). Negative values grow from
  one zero line. 'ends': every cap that fits, else the extreme and the last.
- `Segmented`: `items` [{ key?, label, value, color?, colorIndex? }], `max` (4, <= 6),
  `otherLabel`, `format`, `size` ('thin' 8px | 'tall' 28px), `legend`, `emphasized`,
  `sensitive`, `ariaLabel`. Legend rows: name, value, share; click emphasises.
- `Donut`: same items contract, plus `totalLabel`, `total` (centre override), `size`
  (168), `thickness` (20), `legend` ('side'|'bottom'|'none').
- `Sparkline`: `values`, `width` (120 or a CSS width), `height` (30), `tone`
  ('sign'|'accent'|'muted'), `invert`, `area`, `endDot`, `strokeWidth` (1.5), `emptyText`,
  `label`. Fewer than two points: dashed hairline, or `emptyText`.
- `Delta`: `value`, `suffix` ('%'), `digits` (2), `size` ('sm'|'md'), `format(abs)`,
  `invert`, `arrow`, `empty` ('n/a'), `sensitive` (false).
- `Tooltip`, `Legend`: internal building blocks, usable directly.
