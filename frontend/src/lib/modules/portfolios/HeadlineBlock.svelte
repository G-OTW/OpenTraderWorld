<script>
  // The portfolio's headline (the Delta / Robinhood shape): the value at 28px, the change
  // over the picked period with its glyph and arrow, the period chips on the same row, and
  // the value-over-time chart under it. Snapshots are daily valuations; the granularity
  // switch keeps the last snapshot of each day, week, month or year.
  //
  // The period change is the market's move, not the money put in: the value moved minus the
  // net purchases at cost (the change in cost basis), over the value at the start plus what
  // was added. A snapshot's `flow` is not used for this: rebuilt and live snapshots do not
  // record it the same way, and a period spanning both would count a purchase as a gain.
  import { t } from '$lib/i18n';
  import { dateKey, fmtDate } from '$lib/format.js';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import SegChips from './SegChips.svelte';
  import { fmtMoney, fmtCompact } from './api.js';

  let { snapshots = [], value = null, currency = 'USD' } = $props();

  const PERIODS = ['1d', '1w', '1m', '3m', 'ytd', '1y', 'all'];
  const GRANS = [
    { key: 'day', labelKey: 'portfolios.valueChart.day' },
    { key: 'week', labelKey: 'portfolios.valueChart.week' },
    { key: 'month', labelKey: 'portfolios.valueChart.month' },
    { key: 'year', labelKey: 'portfolios.valueChart.year' }
  ];
  let period = $state('1y');
  let gran = $state('day');

  const sorted = $derived([...(snapshots ?? [])].sort((a, b) => a.snap_date.localeCompare(b.snap_date)));

  /** First day of the period, as YYYY-MM-DD (null = everything). */
  function cutoff(p) {
    const d = new Date();
    if (p === 'all' || p === '1d') return null;
    if (p === 'ytd') return `${d.getFullYear()}-01-01`;
    const days = { '1w': 7, '1m': 30, '3m': 91, '1y': 365 }[p];
    d.setDate(d.getDate() - days);
    return dateKey(d);
  }

  // The window's snapshots, anchored on the last one before the cutoff so the change is
  // measured from where the period began, not from its first new valuation. The anchor is
  // drawn at the cutoff (the last known value carried to the period's first day), so the
  // axis spans the period the chip names.
  const windowed = $derived.by(() => {
    const all = sorted;
    if (period === '1d') return all.slice(-2);
    const from = cutoff(period);
    if (!from) return all;
    const i = all.findIndex((s) => s.snap_date >= from);
    if (i < 0) return all.slice(-1);
    if (i === 0 || all[i].snap_date === from) return all.slice(i);
    return [{ ...all[i - 1], snap_date: from }, ...all.slice(i)];
  });

  // Period key for a YYYY-MM-DD date string, per granularity. Bucketing keeps one point per period.
  function periodKey(dateStr, g) {
    const d = new Date(dateStr + 'T00:00:00');
    if (g === 'day') return dateStr;
    if (g === 'year') return String(d.getFullYear());
    if (g === 'month') return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
    // Week: Monday-anchored. `d` is local midnight, so it is serialized from its local
    // parts; toISOString() would shift it back a day east of Greenwich.
    const day = (d.getDay() + 6) % 7;
    const monday = new Date(d);
    monday.setDate(d.getDate() - day);
    return dateKey(monday);
  }

  const bucketed = $derived.by(() => {
    const byKey = new Map();
    for (const s of windowed) byKey.set(periodKey(s.snap_date, gran), s); // later wins
    return [...byKey.values()];
  });

  const change = $derived.by(() => {
    const w = windowed;
    if (w.length < 2) return null;
    const start = w[0];
    const end = w[w.length - 1];
    const added = (Number(end.cost_basis) || 0) - (Number(start.cost_basis) || 0);
    const abs = end.market_value - start.market_value - added;
    const base = start.market_value + Math.max(0, added);
    return { abs, pct: base > 0 ? (abs / base) * 100 : null, from: start.snap_date };
  });

  const points = $derived(bucketed.map((s) => [s.snap_date, s.market_value]));
  const money = (v) => fmtMoney(v, currency);
</script>

<section class="hero">
  <div class="top">
    <div class="figure">
      <span class="eyebrow">{$t('portfolios.detail.value')}</span>
      <div class="line">
        <span class="worth">{money(value)}</span>
        {#if change}
          <Delta value={change.abs} format={money} size="md" sensitive empty={$t('portfolios.na')} />
          <Delta value={change.pct} size="md" arrow={false} empty="" />
          <span class="since" title={$t('portfolios.headline.netOfFlows')}>
            {period === 'all' ? $t('portfolios.headline.allTime') : $t('portfolios.headline.since', { date: fmtDate(change.from) })}
          </span>
        {/if}
      </div>
    </div>
    <div class="controls">
      <SegChips
        options={PERIODS.map((p) => ({ key: p, label: $t(`portfolios.headline.period.${p}`) }))}
        bind:value={period}
        mono
        ariaLabel={$t('portfolios.headline.periodAria')}
      />
      <SegChips
        options={GRANS.map((g) => ({ key: g.key, label: $t(g.labelKey) }))}
        bind:value={gran}
        ariaLabel={$t('portfolios.headline.granAria')}
      />
    </div>
  </div>

  <ChartFrame
    height={280}
    unavailable={points.length < 2
      ? { title: $t('portfolios.valueChart.emptyTitle'), reason: $t('portfolios.valueChart.notEnoughHistory'), icon: 'trending-up' }
      : null}
  >
    <TimeSeries
      series={[{ key: 'value', label: $t('portfolios.detail.value'), points }]}
      area
      height={280}
      change="none"
      format={money}
      axisFormat={(v) => fmtCompact(v, currency)}
      ariaLabel={$t('portfolios.valueChart.ariaLabel')}
    />
  </ChartFrame>
</section>

<style>
  .hero {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .top {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .figure {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .line {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
  }
  .worth {
    font-size: 28px;
    font-weight: 600;
    line-height: var(--lh-tight);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .since {
    font-size: 12px;
    color: var(--dim);
  }
  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
</style>
