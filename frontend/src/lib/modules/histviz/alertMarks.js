// Which armed alerts a chart shows on its scales, and where. The rule follows what the value
// depends on:
//   - an instant is the same on every chart: a time alert shows on all of them, whatever the
//     instrument or the timeframe it was created on;
//   - a price level belongs to the instrument, at every timeframe;
//   - an indicator level also depends on the bars the indicator reads, so it only shows at the
//     alert's own timeframe. One in price units sits on the price scale; any other needs its
//     study on the chart, since that study's pane is the only scale it can be read on.
// A moving level (a series against a series), a metric (a percentage of nothing on screen) and
// volume (its scale is hidden) have no value to pin on a scale, so they draw nothing.
import { defaultParams, isCustom, isScript } from './indicators.js';

/** Built-ins expressed in price, whatever the study list holds. */
const PRICE_UNITS = /^(sma|ema|dema|tema|wma|hma|vwap|supertrend|psar|bb_|keltner_|donchian_)/;

/** The other built-ins: the chart study drawing each one, and which operand field feeds which
 *  of its params. */
const period = { period: 'period' };
const macd = { fast: 'fast', slow: 'slow' };
const stochastic = { k: 'period', d: 'signal_period' };
const PANE_STUDIES = {
  rsi: ['rsi', period],
  macd: ['macd', macd],
  macd_signal: ['macd', { ...macd, signal: 'signal_period' }],
  macd_hist: ['macd', { ...macd, signal: 'signal_period' }],
  stoch_k: ['stochastic', stochastic],
  stoch_d: ['stochastic', stochastic],
  atr: ['atr', period],
  adx: ['adx', period],
  stddev: ['stddev', period],
  roc: ['roc', period],
  momentum: ['momentum', period],
  willr: ['williamsr', period],
  cci: ['cci', period],
  mfi: ['mfi', period],
  obv: ['obv', {}]
};

const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();

/** The visible study that draws operand `o`: same series, same settings, fed by the close. */
function studyOf(o, instances) {
  const shown = instances.filter((i) => i.visible);
  if (o?.kind === 'custom_indicator') {
    return shown.find((i) => (isCustom(i) && i.custom?.id === o.id) || (isScript(i) && i.script?.id === o.id)) ?? null;
  }
  const spec = o?.kind === 'indicator' ? PANE_STUDIES[o.indicator] : null;
  if (!spec) return null;
  const [type, fields] = spec;
  return (
    shown.find((i) => {
      if (i.type !== type) return false;
      const p = { ...defaultParams(type), ...i.params };
      if (p.source && p.source !== 'close') return false;
      return Object.entries(fields).every(([param, field]) => Number(p[param]) === Number(o[field]));
    }) ?? null
  );
}

/** What chart `coords` draws of `alerts`: `levels` on a Y scale (`study` = the instance whose
 *  scale it reads, null for the price scale) and `times` on the X scale (`at` in ms). */
export function alertMarks(alerts, coords, instances, now = Date.now()) {
  const levels = [];
  const times = [];
  for (const alert of alerts) {
    if (!alert.enabled || (alert.expires_at && Date.parse(alert.expires_at) <= now)) continue;
    if (alert.op === 'at_time') {
      const at = Date.parse(alert.fire_at);
      if (Number.isFinite(at)) times.push({ alert, at });
      continue;
    }
    if (!coords || alert.rhs?.kind !== 'const') continue;
    if (!same(alert.provider, coords.provider) || !same(alert.asset_type, coords.asset_type) || !same(alert.ticker, coords.ticker)) continue;
    const value = Number(alert.rhs.value);
    if (!Number.isFinite(value)) continue;
    const o = alert.lhs;
    if (o?.kind === 'price') {
      if (o.field !== 'volume') levels.push({ alert, value, study: null });
      continue;
    }
    if (alert.timeframe !== coords.timeframe) continue;
    if (o?.kind === 'indicator' && PRICE_UNITS.test(o.indicator)) {
      levels.push({ alert, value, study: null });
      continue;
    }
    const study = studyOf(o, instances);
    if (study) levels.push({ alert, value, study });
  }
  return { levels, times };
}
