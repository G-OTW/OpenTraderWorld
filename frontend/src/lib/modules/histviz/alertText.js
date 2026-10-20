// How a chart alert reads in a list: "SMA(9) crosses above SMA(21)", "close crosses 64,000".
// An alert stores two backtest operands (`lhs`, `rhs`), so this is the backtest's own operand
// text, plus the library name for a custom indicator and the metric's words.
import { operandText, metricHasPeriod } from '$lib/modules/backtest/api.js';

export const OP_KEYS = {
  above: 'histviz.alerts.opAbove',
  below: 'histviz.alerts.opBelow',
  crosses: 'histviz.alerts.opCrosses'
};

/** One side. `library` = `[{ id, name }]` (indicator rows); `t` = the i18n function. */
export function operandLabel(o, library, t) {
  if (!o) return '';
  if (o.kind === 'custom_indicator') {
    const name = library?.find((r) => r.id === o.id)?.name ?? t('histviz.alerts.indicator');
    return o.plot ? `${name}: ${o.plot}` : name;
  }
  if (o.kind === 'metric') {
    const words = t(`backtest.metric.${o.metric}`);
    return metricHasPeriod(o.metric) ? `${words} (${o.period ?? 0})` : words;
  }
  return operandText(o);
}

/** The whole condition. */
export function conditionLabel(a, library, t) {
  return `${operandLabel(a.lhs, library, t)} ${t(OP_KEYS[a.op] ?? OP_KEYS.crosses)} ${operandLabel(a.rhs, library, t)}`;
}

/** Past its expiry: no longer watched, and arming it needs a later date first. */
export const isExpired = (a, now = Date.now()) => !!a.expires_at && new Date(a.expires_at).getTime() <= now;
