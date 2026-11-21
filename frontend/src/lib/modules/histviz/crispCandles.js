// Pixel-snapped candles. ECharts lays a candle out in CSS pixels and only half-snaps its x:
// the open/close/high/low edges keep their fractional y, so every body ends on an
// antialiased row, and each side of the body rounds on its own, so widths jump by a pixel
// from one candle to the next. This pass runs right after ECharts' own candlestick layout
// and moves every edge onto the screen's physical pixel grid: one body width for the whole
// series, the wick centred in it, crisp top and bottom on any pixel ratio.
import * as echarts from 'echarts';

/** A stroke `w` device pixels wide is crisp when its centre sits on a half pixel for an odd
 *  width and on a whole pixel for an even one. Coordinates in, coordinates out (CSS px). */
export function snapLine(v, dpr, w) {
  const d = v * dpr;
  return (w % 2 ? Math.floor(d) + 0.5 : Math.round(d)) / dpr;
}

/** The stroke width a CSS `width` actually covers, in whole device pixels (never 0). */
export function deviceWidth(width, dpr) {
  return Math.max(1, Math.round((Number(width) || 1) * dpr));
}

/** Snap one normal-mode candle in place. `ends` is ECharts' 8-point layout: body corners
 *  [0..3] (top-left, top-right, bottom-right, bottom-left), then high, body top, low, body
 *  bottom for the two wicks. `half` is the half body width in device pixels. */
export function snapCandle(ends, dpr, w, half) {
  const cx = snapLine(ends[4][0], dpr, w);
  const l = cx - half / dpr;
  const r = cx + half / dpr;
  const top = snapLine(ends[5][1], dpr, w);
  const bot = snapLine(ends[7][1], dpr, w);
  // A wick ends on a butt cap, which is crisp on a whole device pixel.
  const hi = Math.round(ends[4][1] * dpr) / dpr;
  const lo = Math.round(ends[6][1] * dpr) / dpr;
  ends[0][0] = l; ends[0][1] = top;
  ends[1][0] = r; ends[1][1] = top;
  ends[2][0] = r; ends[2][1] = bot;
  ends[3][0] = l; ends[3][1] = bot;
  ends[4][0] = cx; ends[4][1] = hi;
  ends[5][0] = cx; ends[5][1] = top;
  ends[6][0] = cx; ends[6][1] = lo;
  ends[7][0] = cx; ends[7][1] = bot;
}

/** Large mode draws each candle as one high-low line: [sign, x, yLow, yHigh] per bar. */
export function snapLarge(points, dpr, w) {
  for (let i = 0; i < points.length; i += 4) {
    if (Number.isNaN(points[i])) continue;
    points[i + 1] = snapLine(points[i + 1], dpr, w);
    points[i + 2] = Math.round(points[i + 2] * dpr) / dpr;
    points[i + 3] = Math.round(points[i + 3] * dpr) / dpr;
  }
}

let registered = false;

export function registerCrispCandles() {
  if (registered) return;
  registered = true;
  // Same stage as ECharts' candlestick layout, registered after it, so it sits right below
  // it in the series pipeline and re-runs whenever that layout does (zoom, pan, resize).
  echarts.registerLayout(echarts.PRIORITY.VISUAL.LAYOUT, {
    seriesType: 'candlestick',
    reset(seriesModel, ecModel, api) {
      const data = seriesModel.getData();
      const dpr = api.getDevicePixelRatio() || 1;
      const w = deviceWidth(seriesModel.get(['itemStyle', 'borderWidth']), dpr);
      if (seriesModel.pipelineContext.large) {
        const points = data.getLayout('largePoints');
        if (points) snapLarge(points, dpr, w);
        return;
      }
      const cw = data.getLayout('candleWidth') || 0;
      const half = Math.max(0, Math.round((cw * dpr) / 2));
      return {
        progress(params, d) {
          for (let i = params.start; i < params.end; i++) {
            const ends = d.getItemLayout(i)?.ends;
            if (ends?.length === 8 && !Number.isNaN(ends[4][0])) snapCandle(ends, dpr, w, half);
          }
        }
      };
    }
  });
}
