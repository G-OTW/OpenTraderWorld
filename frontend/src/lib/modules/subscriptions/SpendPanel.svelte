<script>
  // The chart card: billed spend per month (or per calendar year) stacked by category with
  // the cost-creep line over it, the category ranking, or the 12-month forecast.
  //
  // Category colour follows the category, never the bar's rank in one period: slots come
  // from the breakdown's monthly-equivalent ranking (the four largest get the identity
  // order, the rest fold into Other), and the same mapping paints the stack and the ranking.
  import { fmtMoney, fmtCompactValue, fmtMonth } from '$lib/format.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { identityColor, otherColor } from '$lib/ui/charts/palette.js';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import ForecastChart from './ForecastChart.svelte';
  import Seg from './Seg.svelte';
  import { stackByCategory, UNCATEGORIZED } from './calc.js';
  import { t } from '$lib/i18n';

  let { bd = null, subs = [], cur = 'USD', version = 0 } = $props();

  // 'spend' | 'category' | 'forecast', and 'month' | 'year' for the first two.
  let view = $state('spend');
  let mode = $state('month');

  const HEIGHT = 260;
  const KEEP = 4;
  const c = $derived(chartColors());
  const fmt = (v) => fmtMoney(v, cur);
  const axis = (v) => fmtCompactValue(v, cur);
  const catLabel = (n) => (n === UNCATEGORIZED ? $t('subscriptions.page.uncategorized') : n);

  const now = new Date();
  const thisMonth = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-01`;
  const thisYear = String(now.getFullYear());

  const ranking = $derived((bd?.categories ?? []).map((x) => x.category));
  const colorOf = (name) => {
    const i = ranking.indexOf(name);
    return i >= 0 && i < KEEP ? identityColor(i, c) : otherColor(c);
  };

  // Month view: a year back and a year ahead of the current month (the breakdown is
  // fetched wider so the year view has whole calendar years).
  const periods = $derived.by(() => {
    if (mode === 'year') return bd?.years ?? [];
    const all = bd?.months ?? [];
    const i = all.findIndex((m) => m.month === thisMonth);
    return i < 0 ? all : all.slice(Math.max(0, i - 12), i + 13);
  });
  const nowIdx = $derived(
    periods.findIndex((p) => (mode === 'year' ? p.month.slice(0, 4) === thisYear : p.month === thisMonth))
  );
  // Periods after the current one are scheduled at today's prices: drawn as estimates.
  const ahead = $derived(periods.map((_, i) => nowIdx >= 0 && i > nowIdx));
  const labels = $derived(periods.map((p) => (mode === 'year' ? p.month.slice(0, 4) : fmtMonth(p.month))));

  // Measured periods stack by category; periods ahead are one hollow dashed total each
  // (a projection has no category story to tell, and dashed segments in a stack blur).
  const measured = (vals) => vals.map((v, i) => (ahead[i] ? null : v));
  const spendSeries = $derived.by(() => {
    const st = stackByCategory(periods, subs, ranking, KEEP);
    const out = st.named.length
      ? st.named.map((n, i) => ({ key: n, label: catLabel(n), values: measured(st.rows[i]), color: identityColor(i, c) }))
      : [{ key: 'total', label: $t('subscriptions.page.billed'), values: measured(periods.map((p) => p.amount)) }];
    if (st.named.length && st.other) {
      out.push({ key: '__other', label: $t('subscriptions.page.other'), values: measured(st.other), color: otherColor(c) });
    }
    if (ahead.some(Boolean)) {
      out.push({
        key: '__ahead',
        label: $t('subscriptions.page.projected'),
        values: periods.map((p, i) => (ahead[i] ? p.amount : null)),
        color: c.dim,
        dashed: true
      });
    }
    return out;
  });

  // Cost creep: measured up to this month (solid), projected after it (dashed).
  const creepLines = $derived.by(() => {
    if (mode !== 'month' || !periods.some((p) => p.monthly_equiv != null)) return [];
    const v = (p) => p.monthly_equiv ?? null;
    const past = periods.map((p, i) => (nowIdx < 0 || i <= nowIdx ? v(p) : null));
    const out = [{ key: 'creep', label: $t('subscriptions.monthlyChart.creep'), values: past, color: c.text }];
    if (nowIdx >= 0 && nowIdx < periods.length - 1) {
      out.push({
        key: 'creep-ahead',
        label: $t('subscriptions.page.creepProjected'),
        values: periods.map((p, i) => (i >= nowIdx ? v(p) : null)),
        color: c.text,
        dashed: true
      });
    }
    return out;
  });
  const spendEmpty = $derived(!periods.some((p) => p.amount > 0));

  // Category ranking for the current month (monthly equivalent) or year (full cost).
  const catTotals = $derived.by(() => {
    const p =
      mode === 'year'
        ? (bd?.years ?? []).find((y) => y.month.slice(0, 4) === thisYear)
        : (bd?.months ?? []).find((m) => m.month === thisMonth);
    return Object.entries(p?.by_category ?? {})
      .filter(([, v]) => v > 0)
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  });

  const viewOptions = $derived([
    { value: 'spend', label: $t('subscriptions.page.spend') },
    { value: 'category', label: $t('subscriptions.page.categories') },
    { value: 'forecast', label: $t('subscriptions.forecast.tab') }
  ]);
  const modeOptions = $derived([
    { value: 'month', label: $t('subscriptions.page.month') },
    { value: 'year', label: $t('subscriptions.page.year') }
  ]);
</script>

{#snippet toggles()}
  <Seg options={viewOptions} bind:value={view} ariaLabel={$t('subscriptions.page.visualization')} />
  {#if view !== 'forecast'}
    <Seg options={modeOptions} bind:value={mode} ariaLabel={$t('subscriptions.page.chartGranularity')} />
  {/if}
{/snippet}

<section class="panel">
  {#if view === 'forecast'}
    <ForecastChart {version} actions={toggles} height={HEIGHT} />
  {:else if view === 'category'}
    <ChartFrame
      title={mode === 'year' ? $t('subscriptions.page.thisYearByCategory') : $t('subscriptions.page.thisMonthByCategory')}
      subtitle={mode === 'year'
        ? $t('subscriptions.page.categoryYearSubtitle', { cur })
        : $t('subscriptions.page.categoryMonthSubtitle', { cur })}
      actions={toggles}
      height={HEIGHT}
      unavailable={catTotals.length ? null : { reason: $t('subscriptions.categoryChart.empty'), icon: 'pie-chart' }}
    >
      <Bars
        horizontal
        labels="ends"
        categories={catTotals.map(([n]) => ({ key: n, label: catLabel(n) }))}
        series={[
          {
            key: 'amount',
            label: $t('subscriptions.page.categories'),
            values: catTotals.map(([, v]) => v),
            colors: catTotals.map(([n]) => colorOf(n))
          }
        ]}
        format={fmt}
        ariaLabel={$t('subscriptions.page.thisMonthByCategory')}
      />
    </ChartFrame>
  {:else}
    <ChartFrame
      title={mode === 'year' ? $t('subscriptions.page.yearlySpend') : $t('subscriptions.page.monthlySpend')}
      subtitle={mode === 'year'
        ? $t('subscriptions.page.yearSubtitle', { cur })
        : $t('subscriptions.page.spendSubtitle', { cur })}
      actions={toggles}
      height={HEIGHT}
      unavailable={spendEmpty ? { reason: $t('subscriptions.monthlyChart.empty') } : null}
    >
      <Bars
        categories={labels}
        series={spendSeries}
        lines={creepLines}
        height={HEIGHT}
        format={fmt}
        axisFormat={axis}
        totalLabel={$t('subscriptions.page.total')}
        estimateLabel={$t('subscriptions.page.projected')}
        ariaLabel={mode === 'year' ? $t('subscriptions.page.yearlySpend') : $t('subscriptions.page.monthlySpend')}
      />
    </ChartFrame>
  {/if}
</section>

<style>
  .panel {
    min-width: 0;
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  .panel :global(.cf-actions) {
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  @media (max-width: 767px) {
    .panel :global(.cf-head) {
      flex-direction: column;
    }
    .panel :global(.cf-actions) {
      justify-content: flex-start;
    }
  }
</style>
