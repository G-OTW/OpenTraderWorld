/** Guards on manual trading from the charts (`/api/brokers/chart-guards`): the form's draft,
 *  the rule it writes, and a rule in words. The server reads and fires the rules
 *  (`brokers::chart_guards`); the chart only shows what fired. */

export const RULE_KINDS = ['losing_trades', 'trades', 'drawdown', 'trade_loss', 'fees'];
export const ACTIONS = ['alert', 'acknowledge', 'stop', 'kill_switch'];

/** A new guard as the form starts it: three losing trades in a row, an alert, every account. */
export function blankDraft() {
  return {
    account_id: '',
    kind: 'losing_trades',
    n: 3,
    consecutive: true,
    windowValue: 1,
    windowUnit: 'h',
    rolling: true,
    value: '',
    pct: false,
    of: 'notional',
    action: 'alert',
    close_positions: false,
    enabled: true
  };
}

/** The draft a stored guard opens as. */
export function draftOf(g) {
  const r = g.rule ?? {};
  const d = { ...blankDraft(), account_id: g.account_id ?? '', kind: r.kind, action: g.action, close_positions: !!g.close_positions, enabled: g.enabled !== false };
  if (r.kind === 'losing_trades') Object.assign(d, { n: r.n, consecutive: !!r.consecutive });
  if (r.kind === 'trades') {
    const hours = r.minutes % 60 === 0;
    Object.assign(d, { n: r.n, rolling: !!r.rolling, windowValue: hours ? r.minutes / 60 : r.minutes, windowUnit: hours ? 'h' : 'min' });
  }
  if (r.kind === 'drawdown') Object.assign(d, { value: String(r.value), pct: !!r.pct });
  if (r.kind === 'trade_loss') Object.assign(d, { value: String(r.value), pct: !!r.pct, of: r.of ?? 'notional' });
  if (r.kind === 'fees') Object.assign(d, { value: String(r.value) });
  return d;
}

/** The body the API takes for a draft. */
export function guardOf(d) {
  let rule;
  switch (d.kind) {
    case 'losing_trades':
      rule = { kind: d.kind, n: Math.round(Number(d.n)), consecutive: !!d.consecutive };
      break;
    case 'trades':
      rule = {
        kind: d.kind,
        n: Math.round(Number(d.n)),
        minutes: Math.round(Number(d.windowValue) * (d.windowUnit === 'h' ? 60 : 1)),
        rolling: !!d.rolling
      };
      break;
    case 'drawdown':
      rule = { kind: d.kind, value: String(d.value).trim(), pct: !!d.pct };
      break;
    case 'trade_loss':
      rule = { kind: d.kind, value: String(d.value).trim(), pct: !!d.pct, of: d.of };
      break;
    default:
      rule = { kind: 'fees', value: String(d.value).trim() };
  }
  return {
    account_id: d.account_id || null,
    rule,
    action: d.action,
    close_positions: d.action === 'stop' && !!d.close_positions,
    enabled: d.enabled !== false
  };
}

/** A window of minutes in words: whole hours as hours. */
export function windowText(minutes, t) {
  return minutes % 60 === 0 ? t('histviz.guards.hours', { n: minutes / 60 }) : t('histviz.guards.minutes', { n: minutes });
}

/** A rule in words. */
export function ruleText(rule, t) {
  const r = rule ?? {};
  const amount = (v, pct) => (pct ? `${v}%` : String(v));
  switch (r.kind) {
    case 'losing_trades':
      return t(r.consecutive ? 'histviz.guards.text.lossesInARow' : 'histviz.guards.text.losses', { n: r.n });
    case 'trades':
      return t(r.rolling ? 'histviz.guards.text.tradesSliding' : 'histviz.guards.text.tradesEvery', { n: r.n, window: windowText(r.minutes, t) });
    case 'drawdown':
      return t('histviz.guards.text.drawdown', { value: amount(r.value, r.pct) });
    case 'trade_loss':
      return t(r.pct ? `histviz.guards.text.tradeLossPct.${r.of ?? 'notional'}` : 'histviz.guards.text.tradeLoss', { value: amount(r.value, r.pct) });
    case 'fees':
      return t('histviz.guards.text.fees', { value: r.value });
    default:
      return '';
  }
}
