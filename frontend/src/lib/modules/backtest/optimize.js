/** Optimizer model: which parameters of a strategy can be varied, and the grid they describe.
 *
 * The server side is deliberately dumb (an axis is a settings path and a list of JSON values),
 * so everything a *parameter* means lives here: where it sits in the settings, what unit the user
 * types it in, and what a sensible sweep around its current value looks like.
 *
 * Two things this file is careful about:
 *
 * - **It reads normalized settings.** The paths it emits index the object that is actually posted
 *   (`normalizeSettings`), not the editor draft, so an axis can never point at a field the engine
 *   never sees.
 * - **A mirrored strategy keeps one parameter in two places.** With "reverse side" on, the short
 *   side is derived from the long one; a spec then carries both paths and writes them from the
 *   same value, because sweeping only the long half would measure a strategy nobody configured.
 */
import { indicatorById } from '$lib/indicators/dag.js';

/** Hard caps, mirroring the engine's (`backtest::optimize`). */
export const MAX_AXES = 8;
export const MAX_AXIS_VALUES = 2000;
export const MAX_TRIALS = 250000;

/** Ranking metrics. `higher` false = the column where less is better. */
export const OPT_METRICS = [
  { id: 'sharpe', digits: 2 },
  { id: 'sortino', digits: 2 },
  { id: 'return_pct', digits: 2, suffix: '%' },
  { id: 'net_pnl', digits: 2 },
  { id: 'profit_factor', digits: 2 },
  { id: 'win_rate', digits: 1, suffix: '%' },
  { id: 'expectancy_pct', digits: 3, suffix: '%' },
  { id: 'avg_trade', digits: 2 },
  { id: 'max_drawdown_pct', digits: 2, suffix: '%', higher: false },
  { id: 'trades', digits: 0 },
  { id: 'oos_return_pct', digits: 2, suffix: '%' }
];

export const metricById = (id) => OPT_METRICS.find((m) => m.id === id);
export const metricHigherBetter = (id) => metricById(id)?.higher !== false;

/** Weekdays as the engine numbers them (1 = Monday … 7 = Sunday). */
export const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7];

// ── Suggested sweep around a value ───────────────────────────────────────────────────────
//
// The default range is the answer to "what would I have typed anyway": half to one-and-a-half
// times the current value, in about nine steps. Nine keeps a three-parameter grid in the
// hundreds instead of the tens of thousands, so the user widens it deliberately, not by accident.

/** Round to a sane number of decimals for a display unit (kills 0.30000000000000004). */
const tidy = (v, decimals) => Number(Number(v).toFixed(decimals));

/** Decimals a step implies. */
const stepDigits = (step) => {
  if (!(step > 0)) return 2;
  const d = Math.ceil(-Math.log10(step));
  return Math.min(6, Math.max(0, d));
};

/** Suggested {from, to, step} for an integer parameter (periods, counts). */
function intRange(value, { min = 1, max = Infinity } = {}) {
  const v = Math.max(min, Math.round(Number(value) || min));
  const from = Math.max(min, Math.round(v * 0.5));
  const to = Math.min(max, Math.max(from + 1, Math.round(v * 1.5)));
  const step = Math.max(1, Math.round((to - from) / 9));
  return { from, to, step };
}

/** Suggested {from, to, step} for a continuous parameter (percentages, multipliers). */
function numRange(value, { min = 0 } = {}) {
  const v = Number(value) || 0;
  const base = v !== 0 ? Math.abs(v) : 1;
  const from = Math.max(min, tidy(base * 0.5, 4));
  const to = tidy(base * 1.5, 4);
  const step = tidy(Math.max((to - from) / 8, base / 100), 4) || 0.1;
  return { from, to, step };
}

/** One candidate parameter, in display units. `scale` converts to what the engine stores. */
function spec({ paths, group, label, sub = '', value, kind = 'int', scale = 1, min = 0, max = Infinity }) {
  const display = Number(value) / scale;
  const range = kind === 'int' ? intRange(display, { min, max }) : numRange(display, { min });
  return {
    id: paths.join('|'),
    paths,
    group,
    label,
    sub,
    kind,
    scale,
    min,
    max,
    value: kind === 'int' ? Math.round(display) : tidy(display, 6),
    ...range
  };
}

// ── Discovery ────────────────────────────────────────────────────────────────────────────

/** Sides the settings actually simulate. `mirrored` says the short side is the long one's
 *  inverse: `normalizeSettings` has already materialized it, so nothing in the posted object
 *  says so any more, and only the caller still knows. */
function activeSides(s, mirrored) {
  const mirror = mirrored && s.mode !== 'long';
  const sides = [];
  if (s.mode !== 'short' && s.long) sides.push('long');
  if (s.mode !== 'long' && s.short && !mirror) sides.push('short');
  return { sides, mirrored: mirror };
}

/** Paths for a parameter of `side`, doubled onto the mirror when the short side is derived. */
const twin = (side, tail, mirrored) =>
  side === 'long' && mirrored ? [`long.${tail}`, `short.${tail}`] : [`${side}.${tail}`];

/** Indicator/const parameters of one signal group (entry or exit). */
function groupSpecs(s, side, group, mirrored, t) {
  const out = [];
  const conds = s[side]?.[group]?.conditions ?? [];
  const groupName = t(group === 'entry' ? 'backtest.opt.entry' : 'backtest.opt.exit');
  const sideName = t(side === 'long' ? 'backtest.opt.long' : 'backtest.opt.short');
  conds.forEach((c, i) => {
    for (const slot of ['left', 'right']) {
      const o = c?.[slot];
      if (!o) continue;
      const where = `${sideName} · ${groupName} ${i + 1}`;
      if (o.kind === 'indicator') {
        const def = indicatorById(o.indicator);
        for (const p of def?.params ?? []) {
          const cur = o[p.key] ?? p.def;
          out.push(
            spec({
              paths: twin(side, `${group}.conditions.${i}.${slot}.${p.key}`, mirrored),
              group: 'signals',
              label: `${def?.label ?? o.indicator} ${p.label}`,
              sub: where,
              value: cur,
              kind: Number(p.step) === 1 ? 'int' : 'num',
              min: Number(p.step) === 1 ? 1 : 0
            })
          );
        }
      } else if (o.kind === 'const') {
        out.push(
          spec({
            paths: twin(side, `${group}.conditions.${i}.${slot}.value`, mirrored),
            group: 'signals',
            label: t('backtest.opt.threshold'),
            sub: where,
            value: o.value ?? 0,
            kind: 'num',
            min: -Infinity
          })
        );
      }
      // A custom indicator carries its definition, not parameters: nothing to vary from here.
    }
  });
  return out;
}

/** Stop-loss / take-profit of one side, in whichever form it is configured. */
function stopSpecs(s, side, mirrored, t) {
  const out = [];
  const sideName = t(side === 'long' ? 'backtest.opt.long' : 'backtest.opt.short');
  for (const [key, label] of [
    ['stop_loss', t('backtest.opt.stopLoss')],
    ['take_profit', t('backtest.opt.takeProfit')]
  ]) {
    const rule = s[side]?.[key];
    const legacy = s[side]?.[`${key}_pct`] ?? 0;
    if (rule && rule.kind === 'atr') {
      out.push(
        spec({
          paths: twin(side, `${key}.value`, mirrored),
          group: 'risk',
          label: `${label} (× ATR)`,
          sub: sideName,
          value: rule.value ?? 0,
          kind: 'num'
        })
      );
      out.push(
        spec({
          paths: twin(side, `${key}.period`, mirrored),
          group: 'risk',
          label: `${label} ATR ${t('backtest.opt.length')}`,
          sub: sideName,
          value: rule.period || 14,
          kind: 'int',
          min: 1
        })
      );
    } else if (rule && (rule.value ?? 0) > 0) {
      // The pct form is mirrored into the legacy field, so both are written: the engine reads
      // the object, an older reader reads the mirror, and they must not disagree.
      const paths = twin(side, `${key}.value`, mirrored).concat(twin(side, `${key}_pct`, mirrored));
      out.push(
        spec({ paths, group: 'risk', label: `${label} (%)`, sub: sideName, value: rule.value, kind: 'num', scale: 0.01 })
      );
    } else if (legacy > 0) {
      out.push(
        spec({
          paths: twin(side, `${key}_pct`, mirrored),
          group: 'risk',
          label: `${label} (%)`,
          sub: sideName,
          value: legacy,
          kind: 'num',
          scale: 0.01
        })
      );
    }
  }
  out.push(...trailSpecs(s, side, mirrored, t, sideName));
  return out;
}

/** Trailing-stop axes of one side, each offered only when the plan actually uses it. */
function trailSpecs(s, side, mirrored, t, sideName) {
  const tr = s[side]?.trailing_stop;
  if (!tr) return [];
  const out = [];
  const label = t('backtest.opt.trailingStop');
  const value = tr.value ?? 0;
  if (value > 0) {
    const unit = tr.kind === 'abs' ? 'pts' : '%';
    out.push(
      spec({
        paths: twin(side, 'trailing_stop.value', mirrored),
        group: 'risk',
        label: `${label} (${unit})`,
        sub: sideName,
        value,
        kind: 'num',
        scale: tr.kind === 'pct' ? 0.01 : 1
      })
    );
  }
  for (const [key, key2] of [
    ['activate_pct', 'backtest.opt.trailActivate'],
    ['breakeven_pct', 'backtest.opt.trailBreakeven']
  ]) {
    if ((tr[key] ?? 0) > 0)
      out.push(
        spec({
          paths: twin(side, `trailing_stop.${key}`, mirrored),
          group: 'risk',
          label: `${t(key2)} (%)`,
          sub: sideName,
          value: tr[key],
          kind: 'num',
          scale: 0.01
        })
      );
  }
  return out;
}

/** The parameters of a savings plan, for a DCA strategy only: what is paid in, what a tranche
 *  deploys and when a rule may fire again. A DCA run reads none of the sizing block, so without
 *  these the picker offered it nothing but knobs the mode ignores and every trial of the grid
 *  ran the same simulation. Mirrors `dca_params` in `backtest/params.rs`, which a remote caller
 *  already gets. Values are stored in the units the form types them in (25 = 25%), so no scale. */
function dcaSpecs(s, t) {
  const d = s.kind === 'dca' ? s.dca : null;
  if (!d) return [];
  const out = [];
  const add = (o) => out.push(spec({ group: 'dca', ...o }));
  const planName = t('backtest.dca.mode');

  // The money the plan starts with is one of its real parameters.
  if ((s.starting_capital ?? 0) > 0)
    add({
      paths: ['starting_capital'],
      label: t('backtest.settings.startingCapital'),
      sub: planName,
      value: s.starting_capital,
      kind: 'num'
    });

  const c = d.contribution;
  const contribName = t('backtest.opt.dcaContribution');
  if (c?.amount > 0)
    add({ paths: ['dca.contribution.amount'], label: t('backtest.opt.dcaAmount'), sub: contribName, value: c.amount, kind: 'num' });
  if (c?.every >= 1)
    add({ paths: ['dca.contribution.every'], label: t('backtest.dca.every'), sub: contribName, value: c.every, kind: 'int', min: 1 });

  const named = (r, i, key) => {
    const n = (r?.name ?? '').trim();
    return n || t(key).replace('{n}', i + 1);
  };
  for (const [list, key] of [['buys', 'backtest.dca.rulePlaceholder'], ['sells', 'backtest.dca.sellPlaceholder']]) {
    (d[list] ?? []).forEach((r, i) => {
      const who = named(r, i, key);
      const base = `dca.${list}.${i}`;
      // "all" takes the whole position: its amount is not a number to sweep.
      if ((r.amount ?? 0) > 0 && r.amount_kind !== 'all')
        add({ paths: [`${base}.amount`], label: t('backtest.opt.dcaAmount'), sub: who, value: r.amount, kind: 'num' });
      if ((r.target_gain_pct ?? 0) > 0)
        add({ paths: [`${base}.target_gain_pct`], label: t('backtest.opt.dcaObjective'), sub: who, value: r.target_gain_pct, kind: 'num' });
      // A cap of 0 means "no cap": sweeping one onto a rule that has none changes what the rule is.
      for (const [f, lbl] of [['max_fires', 'backtest.opt.dcaMaxFires'], ['cooldown_bars', 'backtest.opt.dcaCooldown']])
        if ((r[f] ?? 0) > 0)
          add({ paths: [`${base}.${f}`], label: t(lbl), sub: who, value: r[f], kind: 'int', min: 1 });
      out.push(...conditionSpecs(r.condition, `${base}.condition`, who, t));
    });
  }
  return out;
}

/** Indicator lengths and constant thresholds of a plan rule's condition group. Same operand
 *  shapes as a side's signal group, addressed at the rule's own path. */
function conditionSpecs(group, base, who, t) {
  const out = [];
  (group?.conditions ?? []).forEach((c, i) => {
    for (const slot of ['left', 'right']) {
      const o = c?.[slot];
      if (!o) continue;
      const path = `${base}.conditions.${i}.${slot}`;
      const push = (o2) => out.push(spec({ group: 'dca', sub: who, ...o2 }));
      if (o.kind === 'const') {
        push({ paths: [`${path}.value`], label: t('backtest.opt.threshold'), value: o.value ?? 0, kind: 'num', min: -Infinity });
      } else if (o.kind === 'metric') {
        if ((o.period ?? 0) > 0)
          push({
            paths: [`${path}.period`],
            label: `${t(`backtest.metric.${o.metric}`)} ${t('backtest.opt.length')}`,
            value: o.period,
            kind: 'int',
            min: 1
          });
      } else if (o.kind === 'indicator') {
        const def = indicatorById(o.indicator);
        for (const prm of def?.params ?? []) {
          const cur = o[prm.key] ?? prm.def;
          if (!(cur > 0)) continue;
          push({
            paths: [`${path}.${prm.key}`],
            label: `${def?.label ?? o.indicator} ${prm.label}`,
            value: cur,
            kind: Number(prm.step) === 1 ? 'int' : 'num',
            min: Number(prm.step) === 1 ? 1 : 0
          });
        }
      }
    }
  });
  return out;
}

/** Sizing, cost and limit parameters that are set on this strategy. */
function moneySpecs(s, t) {
  const out = [];
  const add = (o) => out.push(spec(o));
  // A savings plan sizes itself from its weights and its tranches, so the whole sizing block,
  // the portfolio limits and the pyramiding gate are dead config there. Costs still apply.
  const isDca = s.kind === 'dca';
  const sizing = isDca ? {} : (s.sizing ?? {});
  const sizingLabel = t('backtest.opt.groupSizing');
  if (sizing.mode === 'percent_equity')
    add({ paths: ['sizing.percent'], group: 'sizing', label: t('backtest.settings.percentInPct'), sub: sizingLabel, value: sizing.percent ?? 100, kind: 'num' });
  if (sizing.mode === 'fixed_qty')
    add({ paths: ['sizing.qty'], group: 'sizing', label: t('backtest.settings.quantityPerEntry'), sub: sizingLabel, value: sizing.qty ?? 1, kind: 'num' });
  if (sizing.mode === 'risk')
    add({ paths: ['sizing.risk_pct'], group: 'sizing', label: t('backtest.settings.riskPct'), sub: sizingLabel, value: sizing.risk_pct ?? 1, kind: 'num' });
  if (sizing.mode === 'kelly') {
    add({ paths: ['sizing.fraction'], group: 'sizing', label: t('backtest.settings.kellyFraction'), sub: sizingLabel, value: sizing.fraction ?? 0.5, kind: 'num' });
    add({ paths: ['sizing.window'], group: 'sizing', label: t('backtest.settings.kellyWindow'), sub: sizingLabel, value: sizing.window ?? 30, kind: 'int', min: 2 });
    add({ paths: ['sizing.cap_pct'], group: 'sizing', label: t('backtest.settings.kellyCap'), sub: sizingLabel, value: sizing.cap_pct ?? 20, kind: 'num' });
  }
  if (!isDca && (s.pyramiding ?? 1) >= 1)
    add({ paths: ['pyramiding'], group: 'sizing', label: t('backtest.settings.pyramiding'), sub: sizingLabel, value: s.pyramiding ?? 1, kind: 'int', min: 1, max: 20 });
  if (!isDca && (s.leverage ?? 1) > 0)
    add({ paths: ['leverage'], group: 'sizing', label: t('backtest.settings.leverage'), sub: sizingLabel, value: s.leverage ?? 1, kind: 'num', min: 1 });

  const costs = t('backtest.opt.groupCosts');
  if ((s.fees?.amount ?? 0) > 0)
    add({ paths: ['fees.amount'], group: 'costs', label: t('backtest.settings.feeAmount'), sub: costs, value: s.fees.amount, kind: 'num' });
  if ((s.spread_pct ?? 0) > 0)
    add({ paths: ['spread_pct'], group: 'costs', label: t('backtest.settings.spreadPct'), sub: costs, value: s.spread_pct, kind: 'num', scale: 0.01 });
  if ((s.slippage?.value ?? 0) > 0)
    add({
      paths: ['slippage.value'],
      group: 'costs',
      label: t('backtest.adv.slippage'),
      sub: costs,
      value: s.slippage.value,
      kind: 'num',
      scale: s.slippage.kind === 'pct' ? 0.01 : 1
    });

  const limits = t('backtest.opt.groupLimits');
  const risk = isDca ? {} : (s.risk ?? {});
  for (const [key, label] of [
    ['max_exposure_pct', t('backtest.settings.maxExposure')],
    ['max_exposure_per_asset_pct', t('backtest.settings.maxExposurePerAsset')],
    ['max_daily_loss_pct', t('backtest.adv.maxDailyLoss')],
    ['max_drawdown_pct', t('backtest.adv.maxDrawdown')]
  ])
    if ((risk[key] ?? 0) > 0)
      add({ paths: [`risk.${key}`], group: 'limits', label, sub: limits, value: risk[key], kind: 'num' });
  if ((risk.max_open_positions ?? 0) > 0)
    add({ paths: ['risk.max_open_positions'], group: 'limits', label: t('backtest.settings.maxOpen'), sub: limits, value: risk.max_open_positions, kind: 'int', min: 1 });
  if (!isDca && (s.pyramid_steps?.min_distance_pct ?? 0) > 0)
    add({ paths: ['pyramid_steps.min_distance_pct'], group: 'limits', label: t('backtest.adv.minDistance'), sub: limits, value: s.pyramid_steps.min_distance_pct, kind: 'num', scale: 0.01 });
  return out;
}

/** Grid-ladder parameters (only for a grid strategy). */
function gridSpecs(s, t) {
  if (s.kind !== 'grid' || !s.grid) return [];
  const g = s.grid;
  const sub = t('backtest.opt.groupGrid');
  const out = [
    spec({ paths: ['grid.levels'], group: 'grid', label: t('backtest.grid.levels'), sub, value: g.levels ?? 10, kind: 'int', min: 2 })
  ];
  if (g.anchor && g.anchor !== 'none') {
    out.push(spec({ paths: ['grid.anchor_period'], group: 'grid', label: t('backtest.grid.anchorPeriod'), sub, value: g.anchor_period ?? 20, kind: 'int', min: 1 }));
    out.push(spec({ paths: ['grid.width_value'], group: 'grid', label: t('backtest.grid.widthValue'), sub, value: g.width_value ?? 2, kind: 'num' }));
    if (g.width_kind === 'atr')
      out.push(spec({ paths: ['grid.width_period'], group: 'grid', label: t('backtest.grid.atrPeriod'), sub, value: g.width_period ?? 14, kind: 'int', min: 1 }));
  } else {
    if (g.lower > 0) out.push(spec({ paths: ['grid.lower'], group: 'grid', label: t('backtest.grid.lower'), sub, value: g.lower, kind: 'num' }));
    if (g.upper > 0) out.push(spec({ paths: ['grid.upper'], group: 'grid', label: t('backtest.grid.upper'), sub, value: g.upper, kind: 'num' }));
  }
  if (g.qty_per_level > 0)
    out.push(spec({ paths: ['grid.qty_per_level'], group: 'grid', label: t('backtest.grid.qtyPerLevel'), sub, value: g.qty_per_level, kind: 'num' }));
  if (g.total_budget > 0)
    out.push(spec({ paths: ['grid.total_budget'], group: 'grid', label: t('backtest.grid.totalBudget'), sub, value: g.total_budget, kind: 'num' }));
  return out;
}

/** Every parameter of `settings` (already normalized) a grid can vary, in display order.
 *  `t` is the i18n reader, passed in like the step summaries do. */
export function discoverParams(settings, t, { mirrored = !!settings?.reverse_side } = {}) {
  if (!settings) return [];
  const { sides, mirrored: mirror } = activeSides(settings, mirrored);
  const out = [];
  // A grid ladders one instrument and a savings plan buys a basket: neither reads a side's
  // signals or its stops, so offering them is a grid of identical trials.
  if (settings.kind !== 'grid' && settings.kind !== 'dca') {
    for (const side of sides) {
      for (const group of ['entry', 'exit']) out.push(...groupSpecs(settings, side, group, mirror, t));
      out.push(...stopSpecs(settings, side, mirror, t));
    }
  }
  out.push(...gridSpecs(settings, t));
  out.push(...dcaSpecs(settings, t));
  out.push(...moneySpecs(settings, t));
  // Two parameters can land on the same path (a mirrored pair discovered twice); keep the first.
  const seen = new Set();
  return out.filter((p) => !seen.has(p.id) && seen.add(p.id));
}

/** Groups, in the order the picker shows them. */
export const PARAM_GROUPS = ['signals', 'risk', 'grid', 'dca', 'sizing', 'costs', 'limits'];

// ── From picked parameters to axes ───────────────────────────────────────────────────────

/** Display values of a range, inclusive of `to` when the step lands on it. */
export function rangeValues({ from, to, step, kind }) {
  const a = Number(from);
  const b = Number(to);
  let s = Math.abs(Number(step));
  if (!Number.isFinite(a) || !Number.isFinite(b)) return [];
  if (!(s > 0)) s = kind === 'int' ? 1 : Math.abs(b - a) || 1;
  if (kind === 'int') s = Math.max(1, Math.round(s));
  const lo = Math.min(a, b);
  const hi = Math.max(a, b);
  const digits = kind === 'int' ? 0 : stepDigits(s) + 2;
  const out = [];
  // Counted rather than accumulated: adding a float step 2000 times drifts, and the last value
  // would miss `to` by a hair.
  const n = Math.floor((hi - lo) / s + 1e-9);
  for (let k = 0; k <= n && out.length < MAX_AXIS_VALUES; k++) out.push(tidy(lo + k * s, digits));
  return out;
}

/** How many values a picked parameter contributes. */
export const specCount = (p) => rangeValues(p).length;

/** Subsets of `days` to exclude, as the engine's *allowed* weekday lists.
 *  `[]` = nothing excluded (every day allowed), which is why the base case is always tried. */
export function weekdayValues(days) {
  const picked = WEEKDAYS.filter((d) => days.includes(d));
  const out = [];
  for (let mask = 0; mask < 1 << picked.length; mask++) {
    const excluded = picked.filter((_, k) => mask & (1 << k));
    // The engine reads an *allow* list, and an empty one means "every day": a rule that excludes
    // nothing must therefore be sent as [] and not as all seven days, or it would read as a rule.
    out.push(excluded.length ? WEEKDAYS.filter((d) => !excluded.includes(d)) : []);
  }
  return out;
}

/** Number of variants a set of axes describes. */
export const comboCount = (axes) => axes.reduce((n, a) => n * Math.max(1, a.values.length), 1);

/** Picked parameters (+ an optional weekday block) → the axes the server runs. */
export function buildAxes(picked, weekdays = [], t = (k) => k) {
  const axes = picked
    .map((p) => ({
      paths: p.paths,
      label: p.sub ? `${p.label} (${p.sub})` : p.label,
      // Display → engine units happens here, once. `scale` rides along so the ranking table can
      // undo it: the server echoes it back untouched and reads a stop-loss of 0.02 as 2.
      scale: p.scale,
      values: rangeValues(p).map((v) => tidy(v * p.scale, 8))
    }))
    .filter((a) => a.values.length > 0);
  if (weekdays.length) {
    axes.push({
      paths: ['filters.weekdays'],
      label: t('backtest.opt.weekdayAxis'),
      values: weekdayValues(weekdays)
    });
  }
  return axes;
}

/** An axis value as the user reads it: weekday lists as day names, numbers back in display units. */
export function displayValue(axis, value, dayNames) {
  if (Array.isArray(value)) {
    if (!value.length) return dayNames?.all ?? 'all';
    const excluded = WEEKDAYS.filter((d) => !value.includes(d));
    if (!excluded.length) return dayNames?.all ?? 'all';
    return `−${excluded.map((d) => dayNames?.short?.[d - 1] ?? d).join(' ')}`;
  }
  if (typeof value !== 'number') return String(value ?? '');
  const scaled = value / (axis?.scale ?? 1);
  return String(tidy(scaled, 6));
}

/** Write a value at a dot-path, a numeric segment indexing an array. Mirror of the engine's
 *  `set_path`; the two must agree or a replayed variant would not be the variant that ran. */
function setPath(root, path, value) {
  const parts = String(path).split('.').filter(Boolean);
  if (!parts.length) return;
  let cur = root;
  for (const seg of parts.slice(0, -1)) {
    if (cur == null || typeof cur !== 'object') return;
    if (Array.isArray(cur)) {
      const i = Number(seg);
      if (!Number.isInteger(i) || !(i in cur)) return;
      cur = cur[i];
    } else {
      if (cur[seg] == null || typeof cur[seg] !== 'object') cur[seg] = {};
      cur = cur[seg];
    }
  }
  const last = parts.at(-1);
  if (Array.isArray(cur)) {
    const i = Number(last);
    if (Number.isInteger(i) && i in cur) cur[i] = value;
  } else if (cur && typeof cur === 'object') {
    cur[last] = value;
  }
}

/** Base settings with one variant's values written in: the client half of the engine's
 *  `patched`, used to replay a picked row as a full backtest. */
export function patchSettings(base, axes, params) {
  const out = JSON.parse(JSON.stringify(base));
  axes.forEach((a, k) => {
    const v = params?.[k];
    if (v === undefined) return;
    for (const path of a.paths?.length ? a.paths : [a.path]) setPath(out, path, v);
  });
  return out;
}

/** "2 h 14 min", "43 min", "18 s". A duration read at the precision it deserves. */
export function fmtDuration(ms, t) {
  if (!Number.isFinite(ms) || ms < 0) return '–';
  const s = Math.round(ms / 1000);
  if (s < 60) return t('backtest.opt.seconds', { n: s });
  const m = Math.round(s / 60);
  if (m < 60) return t('backtest.opt.minutes', { n: m });
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest ? t('backtest.opt.hoursMinutes', { h, m: rest }) : t('backtest.opt.hours', { n: h });
}
