/** Chart display settings — a single fixed/global config persisted server-side
 * (app_settings key `histviz.chart_settings`). The client owns the schema; the
 * backend round-trips the JSON. Empty strings mean "use the theme color".
 *
 * Symbol
 *   upColor / downColor:  bullish / bearish candle body colors ('' = theme)
 *   bodyOpacity:  candle body opacity, in percent (borders and wicks stay opaque)
 *   borderUpColor / borderDownColor:  candle border & wick colors ('' = same as the body)
 *   borderWidth:  candle border & wick width in px (the OHLC bar uses it as its stroke)
 *   lineColor / lineWidth:  line-chart price series ('' = theme accent), also the default
 *     overlay accent
 *   volumeUpColor / volumeDownColor:  volume bars under a rising / falling candle ('' = theme,
 *     independent of the candle colors)
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
 *   indicatorTags:  each visible indicator line's latest value on its pane's scale, in the
 *     line's colour
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
 * Trading (positions and working orders of a trading account, drawn on its chart)
 *   posPnl:  what a position's label adds: 'off', its result in percent ('pct'), or in money
 *     ('value'), against the chart's last price
 *   posLine:  'full' (a line across the pane), 'tight' (a short line by the label) or 'scale'
 *     (no line: a small tag on the price scale at the entry price)
 *   posSide / posQty:  write the side / the quantity on the label (off: the colour says it)
 *   ordLine:  'full' | 'tight' | 'scale', as for positions: on the scale, an order is a tag
 *     there (`1@Buy LMT`) that drags, double-clicks and cancels like its line
 *   ordSide / ordQty:  write the side / the quantity on an order's label
 *   fills:  the account's fills on the chart's instrument as markers at their price and bar
 *     (a buy points up, a sell down), the last 30 days. Off by default: each read is a call
 *     to the broker's fill history
 *   markLongColor / markShortColor:  the markers of a buy / a sell ('' = theme accent / red),
 *     fills and quick backtest alike
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
  volumeUpColor: '',
  volumeDownColor: '',
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
  indicatorTags: true,
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

  posPnl: 'off',
  posLine: 'full',
  posSide: true,
  posQty: true,
  ordLine: 'full',
  ordSide: true,
  ordQty: true,
  fills: false,
  markLongColor: '',
  markShortColor: '',

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

/** White, or black where white would not read (contrast under 3:1), on `color` (`#rgb`, `#rrggbb` or `rgb()`) once its
 * channels are scaled by `shade` (0.55 = mixed 55% with black). Unparsable: white. */
export function inkOn(color, shade = 1) {
  let rgb = null;
  const c = typeof color === 'string' ? color.trim() : '';
  const m = c.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i);
  if (m) rgb = [m[1], m[2], m[3]].map(Number);
  else if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(c)) {
    const h = c.length === 4 ? [...c.slice(1)].map((x) => x + x).join('') : c.slice(1);
    const n = parseInt(h, 16);
    rgb = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  if (!rgb) return '#fff';
  const lin = (v) => {
    const x = (v * shade) / 255;
    return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  const L = 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
  // White keeps the theme's look on the usual greens and reds; 1.05 / (L + 0.05) < 3 past L 0.3.
  return L > 0.3 ? '#000' : '#fff';
}
