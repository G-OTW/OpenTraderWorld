/** Chart display settings — a single fixed/global config persisted server-side
 * (app_settings key `histviz.chart_settings`). The client owns the schema; the
 * backend round-trips the JSON. Empty strings mean "use the theme color".
 *
 * Symbol
 *   upColor / downColor:  bullish / bearish candle body & volume colors ('' = theme)
 *   bodyOpacity:  candle body opacity, in percent (borders and wicks stay opaque)
 *   borderUpColor / borderDownColor:  candle border & wick colors ('' = same as the body)
 *   borderWidth:  candle border & wick width in px (the OHLC bar uses it as its stroke)
 *   lineColor / lineWidth:  line-chart price series ('' = theme accent), also the default
 *     overlay accent
 *   volumeOpacity:  volume bars opacity, in percent
 *
 * Scales
 *   scale:  'linear' | 'log'  price y-axis type
 *   yAxisSide:  'left' | 'right'  which side the price scale is drawn on
 *   axisTextColor / axisFontSize:  axis labels ('' = theme)
 *   lastPrice:  pin the latest close on the price axis, tinted up/down
 *   lastPriceLine:  a horizontal line across the pane at the latest close (needs lastPrice on)
 *   lastLineStyle / lastLineColor / lastLineWidth:  that line's stroke: 'solid' | 'dashed' |
 *     'dotted', a colour ('' = tinted like the last candle) and a width in px
 *   countdown:  time left before the current bar closes, under the last-price tag (needs
 *     lastPrice on, and a bar that is actually still open)
 *   prevClose:  intraday, the level the previous day closed at
 *   bidAsk:  on a live chart, the venue's best bid and ask as two lines with their price on
 *     the axis. Only where the broker streams them; nothing is ever derived from the candles
 *
 * Canvas
 *   bgColor:  chart background ('' = the page surface)
 *   grid / gridV:  horizontal / vertical grid lines
 *   gridColor / gridWidth / gridStyle:  their stroke ('' = theme)
 *   crosshair:  show the crosshair (cross lines following the cursor)
 *   crosshairColor / crosshairWidth / crosshairStyle:  its stroke ('' = theme)
 *   crosshairTags:  per-series value vignettes at the crosshair (needs crosshair on)
 *   tooltip:  the ECharts hover tooltip with all series values (off by default)
 *   daySeparators:  intraday, a line where the calendar day changes
 *
 * Data
 *   customTimeframes:  the user's own timeframes (`7m`, `3h`…), each consolidated on read from
 *     a served timeframe that divides it
 *   formulas:  favourite indicator source formulas, [{ name, expr }] (see formula.js)
 *   autosaveData:  store an instrument automatically the first time you chart it, so its
 *     layout (indicators + drawings) has somewhere to live. Off by default: it queues a
 *     download, and downloads spend a metered API key. */
export const DEFAULT_SETTINGS = {
  upColor: '',
  downColor: '',
  bodyOpacity: 100,
  borderUpColor: '',
  borderDownColor: '',
  borderWidth: 1,
  lineColor: '',
  lineWidth: 1.5,
  volumeOpacity: 45,

  scale: 'linear',
  yAxisSide: 'left',
  axisTextColor: '',
  axisFontSize: 10,
  lastPrice: true,
  lastPriceLine: false,
  lastLineStyle: 'dashed',
  lastLineColor: '',
  lastLineWidth: 1,
  countdown: false,
  prevClose: false,
  bidAsk: false,

  bgColor: '',
  grid: true,
  gridV: false,
  gridColor: '',
  gridWidth: 0.5,
  gridStyle: 'solid',
  crosshair: true,
  crosshairColor: '',
  crosshairWidth: 1,
  crosshairStyle: 'dashed',
  crosshairTags: true,
  tooltip: false,
  daySeparators: false,

  customTimeframes: [],
  formulas: [],
  autosaveData: false
};

/** The keys the chart draws from: a change to any of them rebuilds the option. */
export const STYLE_KEYS = Object.keys(DEFAULT_SETTINGS).filter(
  (k) => !['autosaveData', 'customTimeframes', 'formulas'].includes(k)
);

export const LINE_STYLES = ['solid', 'dashed', 'dotted'];

/** Merge a stored (possibly partial/null) blob onto the defaults. */
export function normalizeSettings(raw) {
  return { ...DEFAULT_SETTINGS, ...(raw && typeof raw === 'object' ? raw : {}) };
}

/** `#rrggbb` (or `#rgb`) at `pct` percent opacity; anything else is returned as is. */
export function withAlpha(color, pct) {
  const a = Math.max(0, Math.min(100, Number(pct ?? 100))) / 100;
  if (a >= 1 || typeof color !== 'string') return color;
  let h = color.trim();
  if (!/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(h)) return color;
  if (h.length === 4) h = '#' + [...h.slice(1)].map((x) => x + x).join('');
  const n = parseInt(h.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}
