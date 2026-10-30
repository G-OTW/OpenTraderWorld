<script>
  // What the grid actually learned, as opposed to which row won.
  //
  // Two reads, both computed over every finished variant:
  //  - per parameter, what each of its values averaged and its best. A parameter whose bars are
  //    all the same height did not matter, and that is worth more than a winning row;
  //  - the spread of the metric over the whole grid, next to the deflated Sharpe, which is the
  //    haircut the trial count earns. The best of many tries is not the quality of a strategy.
  import { t } from '$lib/i18n';
  import { fmtNum } from './api.js';
  import { metricById, metricHigherBetter, displayValue } from './optimize.js';

  let { sensitivity = [], distribution = null, deflated = null, metric = 'sharpe', axes = [] } = $props();

  const digits = $derived(metricById(metric)?.digits ?? 2);
  const suffix = $derived(metricById(metric)?.suffix ?? '');
  const better = $derived(metricHigherBetter(metric));

  const dayNames = $derived({
    all: $t('backtest.opt.everyDay'),
    short: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].map((k) => $t(`common.weekday.${k}`))
  });

  /** Bar width as a fraction, scaled over the observed band of averages so differences show. */
  function bars(values) {
    const avgs = values.map((v) => v.avg).filter((v) => v != null);
    if (!avgs.length) return values.map(() => 0);
    const lo = Math.min(...avgs);
    const hi = Math.max(...avgs);
    const span = hi - lo || Math.abs(hi) || 1;
    return values.map((v) => (v.avg == null ? 0 : 0.08 + 0.92 * ((better ? v.avg - lo : hi - v.avg) / span)));
  }

  const histMax = $derived(distribution?.buckets?.length ? Math.max(...distribution.buckets) : 0);
  const bucketLabel = (k) => {
    if (!distribution) return '';
    const { min, max, buckets } = distribution;
    const w = (max - min) / buckets.length;
    return `${fmtNum(min + k * w, digits)} … ${fmtNum(min + (k + 1) * w, digits)}`;
  };
</script>

<div class="sens">
  {#if distribution}
    <div class="block">
      <h3 class="cap">{$t('backtest.opt.spreadTitle', { metric: $t(`backtest.opt.metric.${metric}`) })}</h3>
      <div class="figs">
        <span>{$t('backtest.opt.variants')} <b>{fmtNum(distribution.n, 0)}</b></span>
        <span>{$t('backtest.opt.worst')} <b>{fmtNum(distribution.min, digits)}{suffix}</b></span>
        <span>{$t('backtest.opt.mean')} <b>{fmtNum(distribution.mean, digits)}{suffix}</b></span>
        <span>{$t('backtest.opt.best')} <b>{fmtNum(distribution.max, digits)}{suffix}</b></span>
        <span>{$t('backtest.opt.positive')} <b>{fmtNum((100 * distribution.positive) / Math.max(1, distribution.n), 0)}%</b></span>
      </div>
      <div class="hist" role="img" aria-label={$t('backtest.opt.spreadTitle', { metric })}>
        {#each distribution.buckets as n, k (k)}
          <div class="bar" style:height="{histMax ? Math.max(2, (100 * n) / histMax) : 0}%" title="{bucketLabel(k)} ({n})"></div>
        {/each}
      </div>
      <div class="axis">
        <span>{fmtNum(distribution.min, digits)}{suffix}</span>
        <span>{fmtNum(distribution.max, digits)}{suffix}</span>
      </div>
    </div>
  {/if}

  {#if deflated}
    <div class="block">
      <h3 class="cap">{$t('backtest.opt.deflatedTitle')}</h3>
      <div class="figs">
        <span>{$t('backtest.opt.bestSharpe')} <b>{fmtNum(deflated.best_sharpe, 2)}</b></span>
        <span>{$t('backtest.opt.selectionBar')} <b>{fmtNum(deflated.expected_max_sharpe, 2)}</b></span>
        <span>{$t('backtest.opt.deflated')} <b>{fmtNum(100 * deflated.deflated_sharpe, 1)}%</b></span>
        <span>{$t('backtest.opt.trialsTried')} <b>{fmtNum(deflated.trials, 0)}</b></span>
      </div>
      <p class="note" class:warn={deflated.selection_explains_it}>
        {deflated.selection_explains_it ? $t('backtest.opt.deflatedFail') : $t('backtest.opt.deflatedNote')}
      </p>
    </div>
  {/if}

  {#each sensitivity as s, k (k)}
    {@const widths = bars(s.values)}
    <div class="block">
      <h3 class="cap">{s.label || s.path}</h3>
      <div class="rows">
        {#each s.values as v, i (i)}
          <div class="row">
            <span class="val">{displayValue(axes[k], v.value, dayNames)}{axes[k]?.unit ?? ''}</span>
            <div class="track"><div class="fill" style:width="{100 * widths[i]}%"></div></div>
            <span class="avg">{v.avg == null ? '–' : `${fmtNum(v.avg, digits)}${suffix}`}</span>
            <span class="best">{v.best == null ? '' : `${$t('backtest.opt.bestShort')} ${fmtNum(v.best, digits)}${suffix}`}</span>
            <span class="n">{fmtNum(v.n, 0)}</span>
          </div>
        {/each}
      </div>
    </div>
  {/each}

  {#if !sensitivity.length && !distribution}
    <p class="note">{$t('backtest.opt.noRowsYet')}</p>
  {/if}
</div>

<style>
  .sens {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    align-items: start;
    gap: var(--space-4);
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
    padding: var(--space-4);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }
  .cap {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .figs {
    display: flex;
    gap: var(--space-3) var(--space-6);
    flex-wrap: wrap;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .figs > span {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .figs b {
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--fs-metric-value);
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
  }
  .note {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: var(--lh-base);
    padding: var(--space-3);
    background: var(--surface-2);
    border-radius: var(--radius-sm);
  }
  .note.warn {
    color: var(--amber-ink);
    background: var(--warning-soft);
  }
  .hist {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 130px;
    border-bottom: var(--hairline) solid var(--border-control);
    background: repeating-linear-gradient(to top, transparent 0, transparent calc(25% - 1px), var(--border) calc(25% - 1px), var(--border) 25%);
  }
  .hist .bar {
    flex: 1;
    background: color-mix(in srgb, var(--accent) 75%, var(--surface));
    min-height: 1px;
    border-radius: 2px 2px 0 0;
  }
  .hist .bar:hover {
    background: var(--accent);
  }
  .axis {
    display: flex;
    justify-content: space-between;
    margin-top: calc(-1 * var(--space-2));
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(56px, 0.8fr) minmax(40px, 1fr) 64px 80px 40px;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    padding: var(--space-2) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:last-child {
    border-bottom: none;
  }
  .val {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .track {
    height: 6px;
    background: var(--surface-3);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
  }
  .avg {
    text-align: right;
    color: var(--text);
  }
  .best, .n {
    text-align: right;
    color: var(--muted);
  }
  @media (max-width: 1100px) {
    .sens {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 480px) {
    .block {
      padding: var(--space-3);
    }
    .row {
      grid-template-columns: minmax(40px, 1fr) minmax(32px, 1fr) 60px;
    }
    .best {
      grid-column: 2;
      text-align: left;
    }
    .n {
      grid-column: 3;
    }
  }
</style>
