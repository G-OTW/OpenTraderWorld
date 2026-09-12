/** Chart display settings — a single fixed/global config persisted server-side
 * (app_settings key `histviz.chart_settings`). The client owns the schema; the
 * backend round-trips the JSON. Empty strings mean "use the theme color".
 *
 * scale:   'linear' | 'log'  price y-axis type
 * grid:    horizontal grid lines on the price pane
 * gridV:   vertical grid lines on the price pane
 * upColor / downColor:  bullish / bearish candle & volume colors ('' = theme)
 * lineColor:  line-chart price series + default overlay accent ('' = theme)
 * crosshair:  show the crosshair (cross lines following the cursor)
 * crosshairTags:  show per-series value vignettes at the crosshair (needs crosshair on)
 * tooltip:  show the ECharts hover tooltip with all series values (off by default)
 * lastPrice:  pin the latest close on the price axis, tinted up/down
 * countdown:  time left before the current bar closes, under the last-price tag (needs
 *   lastPrice on, and a bar that is actually still open)
 * yAxisSide:  'left' | 'right'  which side the price scale is drawn on
 * autosaveData:  store an instrument automatically the first time you chart it, so its
 *   layout (indicators + drawings) has somewhere to live. Off by default: it queues a
 *   download, and downloads spend a metered API key. */
export const DEFAULT_SETTINGS = {
  scale: 'linear',
  grid: true,
  gridV: false,
  upColor: '',
  downColor: '',
  lineColor: '',
  crosshair: true,
  crosshairTags: true,
  tooltip: false,
  lastPrice: true,
  countdown: false,
  // Intraday furniture: where the day changes, and the level it closed at.
  daySeparators: false,
  prevClose: false,
  yAxisSide: 'left',
  autosaveData: false
};

/** Merge a stored (possibly partial/null) blob onto the defaults. */
export function normalizeSettings(raw) {
  return { ...DEFAULT_SETTINGS, ...(raw && typeof raw === 'object' ? raw : {}) };
}
