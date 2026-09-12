<script>
  // Net-worth widget: the MyWealth headline — current net worth, its change over the
  // configured window, and a sparkline of that window. Values are the module's display
  // currency (FX-converted server-side), same as the module page.
  // Config: { months } — how far back the window reaches (default 12).
  import { wealthApi, assetTypeLabel } from '$lib/modules/wealth/api.js';
  import { fmtMoney, fmtSignedMoney, fmtSignedPct, fmtMonth, fmtCompactMoney, fmtCompactValue } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import Donut from './parts/Donut.svelte';
  import RankBars from './parts/RankBars.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import TrendChart from './parts/TrendChart.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let { item, editing } = $props();
  const months = $derived(Math.max(1, Math.min(60, item.config?.months ?? 12)));
  // `display` is the original two-way setting; `variant` is the full card set. A saved
  // widget that only carries `display` keeps working.
  const variant = $derived(item.config?.variant ?? (item.config?.display === 'trend' ? 'trend' : 'overview'));
  const display = $derived(variant === 'trend' ? 'trend' : 'overview');
  const limit = $derived(item.config?.limit ?? 5);

  let bd = $state(null);
  let err = $state('');

  async function load(back) {
    err = '';
    try {
      bd = await wealthApi.breakdown({ granularity: 'month', points_back: back });
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    const back = months;
    if (!editing) load(back);
  });

  // Only the "top assets" card needs the asset rows themselves; every other card is
  // served by the one breakdown call.
  let assets = $state(null);
  $effect(() => {
    if (editing || variant !== 'top') return;
    let alive = true;
    wealthApi
      .listAssets()
      .then((r) => { if (alive) assets = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const cur = $derived(bd?.display_currency ?? 'USD');
  const points = $derived(bd?.points ?? []);
  const money = (v) => (v == null ? '—' : fmtMoney(v, cur, 0));

  // Month-over-month steps of the window: the mockup's bar strip beside the change figure.
  const steps = $derived(points.slice(1).map((p, i) => p.net_worth - points[i].net_worth));
  const stepMonths = $derived(points.slice(1).map((p) => fmtMonth(p.at)));

  const categories = $derived(
    Object.entries(bd?.by_category ?? {})
      .map(([key, value]) => ({ label: key || $t('dashboard.widgets.wealth.uncategorized'), value }))
      .sort((a, b) => Math.abs(b.value) - Math.abs(a.value))
  );

  const topAssets = $derived(
    [...(assets ?? [])]
      .filter((a) => a.latest_value != null)
      .sort((a, b) => Math.abs(b.latest_value) - Math.abs(a.latest_value))
      .slice(0, limit)
  );

  // Baseline = the first point that is actually worth something. Months before the first
  // asset existed sit at zero, and measuring growth from zero is a meaningless ∞%.
  const base = $derived(points.find((p) => p.net_worth > 0) ?? null);
  const delta = $derived(base ? (bd?.net_worth ?? 0) - base.net_worth : null);
  const deltaPct = $derived(base && base.net_worth ? (delta / base.net_worth) * 100 : null);
  const down = $derived(delta != null && delta < 0);

  // The plotted series starts at the baseline point: months before the first asset
  // existed sit at zero, and a curve that climbs out of zero states a growth the book
  // never had.
  const series = $derived(
    base ? points.slice(points.indexOf(base)).map((p) => ({ at: p.at, value: p.net_worth })) : []
  );
  const spark = $derived(series.length > 1);
</script>

<WidgetState
  {editing}
  error={err}
  loading={bd === null}
  preview={$t('dashboard.widgets.wealth.preview')}
  rows={3}
>
  {#if variant === 'change'}
    <div class="split">
      <Stat
        value={deltaPct == null ? '—' : fmtSignedPct(deltaPct, 2)}
        tone={down ? 'neg' : 'pos'}
        note={delta == null ? '' : fmtSignedMoney(delta, cur, 0)}
      />
      {#if steps.length > 1}
        <div class="barbox"><MiniBars values={steps} labels={stepMonths} tone="signed" height={46}
          valueFormat={(v) => fmtSignedMoney(v, cur, 0)}
          label={$t('dashboard.widgets.wealth.trend')} /></div>
      {/if}
    </div>
  {:else if variant === 'split'}
    <Donut
      segments={[
        { label: $t('dashboard.widgets.wealth.assets'), value: bd.assets_value, color: 'var(--chart-1)' },
        { label: $t('dashboard.widgets.wealth.liabilities'), value: bd.liabilities_value, color: 'var(--red)' }
      ]}
      extra={[{ label: $t('dashboard.widgets.wealth.netWorth'), value: bd.net_worth }]}
      center={fmtCompactMoney(bd.net_worth) ?? money(bd.net_worth)}
      centerLabel={$t('dashboard.widgets.wealth.netWorth')}
      showPct={false}
      valueFormat={money}
    />
  {:else if variant === 'category'}
    <RankBars rows={categories.map((c) => ({ ...c, display: money(c.value) }))} />
  {:else if variant === 'stale'}
    {#if (bd.stale ?? []).length === 0}
      <p class="w-state">{$t('dashboard.widgets.wealth.noStale')}</p>
    {:else}
      <table class="w-tbl">
        <thead>
          <tr>
            <th colspan="2">{$t('dashboard.widgets.wealth.asset')}</th>
            <th class="num">{$t('dashboard.widgets.wealth.lastUpdate')}</th>
          </tr>
        </thead>
        <tbody>
          {#each bd.stale.slice(0, limit) as a (a.id)}
            <tr>
              <td class="warnmark"><Icon name="alert-triangle" size={14} /></td>
              <td class="w-name">{a.name}</td>
              <td class="num w-neg">
                {a.age_days == null
                  ? $t('dashboard.widgets.wealth.neverValued')
                  : $t('dashboard.widgets.wealth.daysAgo', { days: a.age_days })}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if variant === 'top'}
    {#if assets === null}
      <p class="w-state">…</p>
    {:else}
      <ul class="w-list">
        {#each topAssets as a, i (a.id)}
          <li class="w-row">
            <span class="rank">{i + 1}</span>
            <span class="w-name grow">{a.name}</span>
            <span class="w-sub type">{assetTypeLabel(a.asset_type)}</span>
            <span class="w-num">{fmtMoney(a.latest_value, a.currency, 0)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
  <div class="w-body">
    {#if spark}
      <!-- The chart takes whatever height is left, so the widget reads the same at any
           of the three height presets instead of overflowing the compact one. -->
      <TrendChart
        points={series}
        valueLabel={display === 'overview' ? '' : `${$t('dashboard.widgets.wealth.trend')} · ${months}M`}
        format={(v) => fmtMoney(v, cur, 0)}
        axisFormat={(v) => fmtCompactValue(v, cur)}
        changeFormat={(v) => fmtSignedMoney(v, cur, 0)}
        label={$t('dashboard.widgets.wealth.trend')}
      />
    {:else}
      <div class="head">
        <div class="w-metric">
          <div class="w-metric-value">{fmtMoney(bd.net_worth, cur, 0)}</div>
          {#if delta != null}
            <div class="w-metric-delta {down ? 'w-neg' : 'w-pos'}">
              {fmtSignedMoney(delta, cur, 0)}
              <span class="w-metric-note">
                {$t('dashboard.widgets.wealth.since', { month: fmtMonth(base.at) })}
              </span>
            </div>
          {/if}
        </div>
        {#if deltaPct != null}
          <span class="w-pill" class:pos={!down} class:neg={down}>{fmtSignedPct(deltaPct, 1)}</span>
        {/if}
      </div>
    {/if}

    <div class="w-sub">
      {#if (bd.asset_count ?? 0) === 1}
        {$t('dashboard.widgets.wealth.metaOne')}
      {:else}
        {$t('dashboard.widgets.wealth.metaMany', { count: bd.asset_count ?? 0 })}
      {/if}
      {#if bd.unconverted > 0}
        <span class="w-warn">· {$t('dashboard.widgets.wealth.unconverted', { count: bd.unconverted, cur })}</span>
      {/if}
    </div>
  </div>
  {/if}
</WidgetState>

<style>
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .split {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-width: 0;
  }
  .barbox {
    flex: 1;
    max-width: 55%;
    min-width: 0;
  }
  .warnmark {
    width: 22px;
    padding-right: 0;
    color: var(--amber);
  }
  .rank {
    width: 14px;
    color: var(--faint);
    font-family: var(--mono);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  .grow {
    flex: 1;
  }
  .type {
    flex-shrink: 0;
  }
</style>
