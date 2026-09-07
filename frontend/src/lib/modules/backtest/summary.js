/** One-line summaries of a settings object, shown under each step tab.
 *
 * Pure functions: the caller passes the already-resolved translator (`$t`), so these stay
 * usable from any component and from a test without a store. Keep them short — the tab strip
 * truncates, and a summary that needs two lines is a step body, not a summary. */
import { conditionText } from './api.js';

/** Selected datasets, e.g. "BTCUSDT · 1h" or "3 assets · 1h". */
export function dataSummary(datasets, datasetIds, t) {
  const sel = datasetIds.map((id) => datasets.find((d) => d.id === id)).filter(Boolean);
  if (!sel.length) return t('backtest.settings.pickDataset');
  if (sel.length === 1) return `${sel[0].ticker} · ${sel[0].timeframe}`;
  return t('backtest.settings.dataMulti', {
    tickers: sel.map((d) => d.ticker).join(', '),
    tf: sel[0].timeframe
  });
}

/** Direction + first entry rule, e.g. "long · EMA(20) crosses above close +1". */
export function strategySummary(settings, t) {
  if (settings.kind === 'dca') {
    const d = settings.dca ?? {};
    const bits = [];
    if (d.contribution?.amount > 0) {
      const c = d.contribution;
      bits.push(
        t('backtest.dca.summaryContribution', {
          amount: c.amount,
          every: c.every > 1 ? `${c.every} ` : '',
          period: t(`backtest.dca.period_${c.period}`)
        })
      );
    }
    const rules = (d.buys?.length ?? 0) + (d.sells?.length ?? 0);
    if (rules) bits.push(t('backtest.dca.summaryRules', { buys: d.buys?.length ?? 0, sells: d.sells?.length ?? 0 }));
    return bits.length ? bits.join(' · ') : t('backtest.dca.summaryEmpty');
  }
  if (settings.kind === 'grid') {
    const g = settings.grid ?? {};
    const reset = g.reset_on_close ? ` · ${t('backtest.grid.summaryReset')}` : '';
    if (g.anchor && g.anchor !== 'none') {
      const width = g.width_kind === 'atr' ? `${g.width_value ?? 0}×ATR` : `${g.width_value ?? 0}%`;
      return (
        t('backtest.grid.summaryAnchored', {
          levels: g.levels ?? 0,
          anchor: `${g.anchor.toUpperCase()}(${g.anchor_period ?? 0})`,
          width
        }) + reset
      );
    }
    return t('backtest.grid.summary', { levels: g.levels ?? 0, lower: g.lower ?? 0, upper: g.upper ?? 0 }) + reset;
  }
  const side = settings.mode === 'short' ? settings.short : settings.long;
  const conds = side?.entry?.conditions ?? [];
  const first = conds.length ? conditionText(conds[0]) : t('backtest.settings.noEntryRule');
  const more = conds.length > 1 ? ` +${conds.length - 1}` : '';
  return `${settings.mode} · ${first}${more}`;
}

/** Size, leverage, starting capital, pyramiding — plus the fee line (same step). */
export function sizingSummary(settings, t) {
  // A savings plan is sized by its weights and tranches, so this step only carries its costs.
  if (settings.kind === 'dca') {
    const f = settings.fees ?? {};
    const fee = f.amount
      ? `${f.amount}${f.amount_kind === 'pct' ? '%' : ''}/${f.per}`
      : t('backtest.settings.noFees');
    return `${t('backtest.dca.sizedByPlan')} · ${fee}`;
  }
  const s = settings.sizing;
  let size;
  switch (s.mode) {
    case 'percent_equity':
      size = t('backtest.settings.summaryPercentEquity', { percent: s.percent });
      break;
    case 'fixed_qty':
      size = t('backtest.settings.summaryQty', { qty: s.qty });
      break;
    case 'risk':
      size = t('backtest.settings.summaryRisk', { pct: s.risk_pct });
      break;
    case 'equity_tiers':
      size = t('backtest.settings.summaryTiers', { n: s.tiers?.length ?? 0 });
      break;
    case 'kelly':
      size = t('backtest.settings.summaryKelly', { fraction: s.fraction });
      break;
    default:
      size = s.mode;
  }
  const pyr =
    (settings.pyramiding ?? 1) > 1 ? ` · ${t('backtest.settings.summaryPyr', { count: settings.pyramiding })}` : '';
  const f = settings.fees ?? {};
  const fee = f.amount
    ? ` · ${f.amount}${f.amount_kind === 'pct' ? '%' : ''}/${f.per}`
    : ` · ${t('backtest.settings.noFees')}`;
  return `${size} · ${settings.leverage}×${pyr}${fee}`;
}

/** Day keys in engine order (1 = Monday), for the weekday chips and the summary. */
export const DAY_KEYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];

/** "UTC+02:00" for an offset in minutes. */
export function tzLabel(min) {
  const m = Math.abs(min ?? 0);
  return `UTC${(min ?? 0) < 0 ? '-' : '+'}${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

/** One date rule as text: a single day, or "from → to". */
export function dateRuleText(r) {
  const from = r?.from ?? '';
  return r?.to && r.to !== from ? `${from} → ${r.to}` : from;
}

/** Collapse consecutive selected weekdays into "Mon-Fri", keeping gaps as separate runs. */
function dayRuns(days, t) {
  const sorted = [...new Set(days)].filter((d) => d >= 1 && d <= 7).sort((a, b) => a - b);
  const label = (d) => t(`common.weekday.${DAY_KEYS[d - 1]}`);
  const out = [];
  let i = 0;
  while (i < sorted.length) {
    let j = i;
    while (j + 1 < sorted.length && sorted[j + 1] === sorted[j] + 1) j++;
    out.push(j - i >= 2 ? `${label(sorted[i])}-${label(sorted[j])}` : sorted.slice(i, j + 1).map(label).join(', '));
    i = j + 1;
  }
  return out.join(', ');
}

/** When the strategy may open, or "always" when no rule is set. */
export function filtersSummary(settings, t) {
  const f = settings.filters ?? {};
  const bits = [];
  if (f.weekdays?.length) bits.push(dayRuns(f.weekdays, t));
  if (f.sessions?.length) {
    bits.push(f.sessions.map((s) => `${s.from}-${s.to}`).join(', '));
    bits.push(tzLabel(f.tz_offset_min));
  }
  if (f.include_dates?.length) bits.push(t('backtest.filters.summaryOnly', { n: f.include_dates.length }));
  if (f.exclude_dates?.length) bits.push(t('backtest.filters.summaryNever', { n: f.exclude_dates.length }));
  if (!bits.length) return t('backtest.filters.summaryAlways');
  if (f.on_window_end === 'flat') bits.push(t('backtest.filters.summaryFlat'));
  return bits.join(' · ');
}

/** What the Advanced step actually changes, or "defaults" when it changes nothing. */
export function advancedSummary(settings, t) {
  const bits = [];
  const ps = settings.pyramid_steps ?? {};
  if (ps.scale?.length) bits.push(t('backtest.adv.summaryScale', { seq: ps.scale.join('/') }));
  const inst = settings.instrument ?? {};
  if (inst.multiplier && inst.multiplier !== 1) bits.push(`×${inst.multiplier}`);
  if (inst.lot_step) bits.push(t('backtest.adv.summaryLot', { step: inst.lot_step }));
  if (settings.slippage?.value) bits.push(t('backtest.adv.summarySlip'));
  const rk = settings.risk ?? {};
  if (rk.max_drawdown_pct || rk.max_daily_loss_pct) bits.push(t('backtest.adv.summaryBreaker'));
  if (settings.oos_split_pct) bits.push(t('backtest.adv.summaryOos', { pct: Math.round(settings.oos_split_pct * 100) }));
  if (settings.funding?.annual_rate_pct) bits.push(t('backtest.adv.summaryFunding'));
  return bits.length ? bits.join(' · ') : t('backtest.adv.summaryNone');
}
