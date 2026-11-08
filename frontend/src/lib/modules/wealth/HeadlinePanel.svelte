<script>
  // The headline block: net worth at 28px, its change over the picked period, the period
  // chips, the Nominal / Real switch and the estimated share on one row; the curve under it
  // (estimates dashed); then the tiles: assets, liabilities, liquid, runway, stale.
  //
  // The page owns `bd` (it also feeds the composition and the table) and the period, so
  // one filter row scopes everything. This panel owns the Real curve and the two reads the
  // tiles need (liquid flags, the Subscriptions forecast).
  import Icon from '$lib/ui/Icon.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { wealthApi, monthLabel } from './api.js';
  import { deflate, curveChange, runway, liquidTotal, PERIODS } from './engine.js';
  import { money, signedMoney, compactMoney, pct } from './view.js';
  import { fundamentalsApi } from '$lib/modules/fundamentals/api.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    /** The breakdown for the page filter and period. */
    bd = null,
    /** The curve for the period (ALL already trimmed). */
    points = [],
    currency = 'USD',
    /** Date of the latest recorded valuation in view: what comes after it is the estimated
     *  tail, drawn dashed. */
    lastValued = null,
    period = $bindable('1Y'),
    settings = {},
    /** (amount, from) => amount in `currency`, or null without a rate. */
    toDisplay = (v) => v,
    reloadKey = 0
  } = $props();

  // 'nominal' | 'real': the curve deflated by the CPI series picked in Settings.
  let mode = $state('nominal');
  let shape = $state('line'); // line | bars
  let real = $state(null);
  let realError = $state('');
  let realLoading = $state(false);

  async function loadReal() {
    realLoading = true;
    realError = '';
    try {
      real = await wealthApi.real(settings?.cpi_series_id ?? null);
    } catch (e) {
      realError = e.message;
      real = null;
    }
    realLoading = false;
  }
  $effect(() => {
    void settings?.cpi_series_id;
    if (mode === 'real') loadReal();
  });
  // The US CPI from FRED, added to Fundamentals the way its Macro page adds a series.
  async function loadCpi() {
    realLoading = true;
    realError = '';
    try {
      await fundamentalsApi.addSeries('fred', 'CPIAUCSL', 'inflation');
      realError = $t('wealth.real.loading');
      await loadReal();
    } catch (e) {
      realError = e.message;
    }
    realLoading = false;
  }

  // While the series loads the real chart is empty, never the nominal curve under a Real label.
  const shown = $derived(
    mode === 'real' ? (real?.observations?.length ? deflate(points, real.observations) : []) : points
  );
  const realOff = $derived(mode === 'real' && !realLoading && (realError || (real && !real.observations?.length)));
  const change = $derived(curveChange(shown));
  const headline = $derived(mode === 'real' && shown.length ? shown[shown.length - 1].net_worth : bd?.net_worth);
  const estShare = $derived(bd?.net_worth ? Math.abs(bd.estimated_value ?? 0) / Math.abs(bd.net_worth) : 0);

  const isEst = (p) => Math.abs(p?.estimated ?? 0) > 1e-9;
  // Dashed is the tail past the last real valuation: a curve that is 99% recorded with a
  // drifting house in it is measured history, not a projection. Earlier points still name
  // their estimated part in the tooltip.
  const isTail = (p) => isEst(p) && (!lastValued || p.at > lastValued);
  const yearly = $derived(bd?.points?.length > 1 && shown.length > 1 && daysBetween(shown[0].at, shown[1].at) > 200);
  function daysBetween(a, b) {
    return (new Date(b + 'T00:00:00') - new Date(a + 'T00:00:00')) / 86400000;
  }
  const series = $derived([
    {
      key: 'nw',
      label: $t('wealth.page.netWorth'),
      points: shown.map((p) => [
        p.at,
        p.net_worth,
        isTail(p) ? $t('wealth.drift.tip', { amount: money(p.estimated, currency) }) : null
      ])
    }
  ]);
  const barCats = $derived(shown.map((p) => (yearly ? p.at.slice(0, 4) : monthLabel(p.at))));
  const fmt = (v) => money(v, currency);
  const axis = (v) => compactMoney(v, currency);

  // Tiles. Liquid reads the categories flagged in Liquidity; runway adds the Subscriptions
  // forecast to the monthly expenses set in Goals.
  let cats = $state([]);
  let subs = $state(null);
  $effect(() => {
    void reloadKey;
    wealthApi.categories().then((c) => (cats = c)).catch(() => (cats = []));
    wealthApi
      .subscriptionsForecast()
      .then((f) => (subs = { monthly: f.monthly_avg ?? 0, currency: f.currency }))
      .catch(() => (subs = null));
  });
  const liquid = $derived(liquidTotal(bd?.by_category ?? {}, cats.filter((c) => c.liquid).map((c) => c.name)));
  const subsMonthly = $derived(subs ? toDisplay(subs.monthly, subs.currency) : null);
  const run = $derived(runway(liquid, settings?.monthly_expenses, subsMonthly));
  const staleList = $derived(bd?.stale ?? []);
</script>

<section class="box headline">
  <div class="head-row">
    <div class="figure">
      <span class="eyebrow">{$t('wealth.page.netWorth')} · {currency}{mode === 'real' ? ` · ${$t('wealth.real.real')}` : ''}</span>
      <div class="value-line">
        <strong class="nw">{money(headline, currency)}</strong>
        {#if change.abs != null}
          <span class="change">
            <Delta value={change.abs} format={(v) => money(v, currency)} size="md" sensitive />
            {#if change.pct != null}<Delta value={change.pct} digits={1} arrow={false} />{/if}
            <span class="since">{$t('wealth.headline.since', { date: change.from })}</span>
          </span>
        {/if}
      </div>
    </div>
    <div class="controls">
      <div class="seg" role="group" aria-label={$t('wealth.period.group')}>
        {#each PERIODS as p (p)}
          <button class:active={period === p} aria-pressed={period === p} onclick={() => (period = p)}>{$t(`wealth.period.${p}`)}</button>
        {/each}
      </div>
      <div class="seg" role="group" aria-label={$t('wealth.real.group')}>
        <button class:active={mode === 'nominal'} aria-pressed={mode === 'nominal'} onclick={() => (mode = 'nominal')}>{$t('wealth.real.nominal')}</button>
        <button class:active={mode === 'real'} aria-pressed={mode === 'real'} onclick={() => (mode = 'real')}>{$t('wealth.real.real')}</button>
      </div>
      <div class="seg icons" role="group" aria-label={$t('wealth.page.chartShapeGroup')}>
        <button class:active={shape === 'line'} aria-pressed={shape === 'line'} title={$t('wealth.page.line')} aria-label={$t('wealth.page.line')} onclick={() => (shape = 'line')}><Icon name="trending-up" size={13} /></button>
        <button class:active={shape === 'bars'} aria-pressed={shape === 'bars'} title={$t('wealth.page.bars')} aria-label={$t('wealth.page.bars')} onclick={() => (shape = 'bars')}><Icon name="bar-chart" size={13} /></button>
      </div>
    </div>
  </div>
  {#if estShare > 0}
    <!-- The part of the headline that is an assumed drift since the last valuation. -->
    <p class="est-line"><span class="est-mark" aria-hidden="true"></span>{$t('wealth.drift.share', { pct: (estShare * 100).toFixed(1) })} ({money(bd.estimated_value, currency)})</p>
  {/if}

  <ChartFrame
    height={240}
    loading={mode === 'real' && realLoading && !real}
    unavailable={realOff
      ? { reason: $t('wealth.real.none'), icon: 'trending-up', action: { label: $t('wealth.real.load'), onclick: loadCpi } }
      : shown.length === 0
        ? { reason: $t('wealth.netWorthChart.empty'), icon: 'trending-up' }
        : null}
  >
    {#if shape === 'bars'}
      <Bars
        categories={barCats}
        series={[{ key: 'nw', label: $t('wealth.page.netWorth'), values: shown.map((p) => p.net_worth), dashed: shown.map(isTail) }]}
        format={fmt}
        axisFormat={axis}
        height={240}
        estimateLabel={$t('wealth.drift.series')}
        ariaLabel={$t('wealth.page.netWorth')}
      />
    {:else}
      <TimeSeries
        {series}
        area
        format={fmt}
        axisFormat={axis}
        height={240}
        change="abs"
        estimateLabel={$t('wealth.drift.series')}
        ariaLabel={$t('wealth.page.netWorth')}
      />
    {/if}
  </ChartFrame>
  {#if mode === 'real' && real?.selected && !realOff}
    <p class="note">{$t('wealth.real.note', { series: real.series.find((x) => x.id === real.selected)?.title ?? '' })}</p>
  {/if}
  {#if realOff}<ErrorText error={realError} />{/if}
  {#if bd?.unconverted > 0}
    <p class="warn"><Icon name="alert-triangle" size={13} /> {$t('wealth.page.unconvertedWarning', { count: bd.unconverted, currency })}</p>
  {/if}
</section>

<div class="tiles">
  <StatCard label={$t('wealth.page.assets')} value={money(bd?.assets_value, currency)} hint={$t('wealth.page.assetCount', { count: bd?.asset_count ?? 0 })} />
  <!-- Owed, before it is netted: a headline that shows only the difference is a number the
       reader cannot check. Negative, with the minus glyph. -->
  <StatCard label={$t('wealth.page.liabilities')} value={bd?.liabilities_value ? signedMoney(-bd.liabilities_value, currency) : money(0, currency)} hint={bd?.assets_value > 0 && bd?.liabilities_value ? $t('wealth.page.ofAssets', { pct: pct((bd.liabilities_value / bd.assets_value) * 100) }) : ''} />
  <StatCard
    label={$t('wealth.liquidity.liquid')}
    value={liquid == null ? $t('wealth.page.na') : money(liquid, currency)}
    hint={liquid == null ? $t('wealth.liquidity.flagOne') : bd?.net_worth > 0 ? $t('wealth.page.ofNetWorth', { pct: pct((liquid / bd.net_worth) * 100) }) : ''}
  />
  <StatCard
    label={$t('wealth.liquidity.runway')}
    value={run.months != null ? $t('wealth.liquidity.months', { n: fmtNum(run.months, 1) }) : $t('wealth.page.na')}
    hint={run.months != null
      ? $t('wealth.page.perMonth', { amount: money(run.cost, currency) })
      : run.reason === 'no_cost'
        ? $t('wealth.liquidity.setExpenses')
        : $t('wealth.liquidity.flagOne')}
  />
  <!-- A valuation that has aged past what its owner asked for. The failure mode this names
       is silence: a house valued three years ago is wrong, and no tracker says so. -->
  <StatCard
    label={$t('wealth.page.stale')}
    value={String(staleList.length)}
    hint={staleList.length
      ? staleList[0].age_days == null
        ? $t('wealth.page.staleNever', { name: staleList[0].name })
        : $t('wealth.page.staleOldest', { name: staleList[0].name, days: staleList[0].age_days })
      : $t('wealth.page.staleNone')}
  />
</div>

<style>
  .box {
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    padding: var(--space-4);
    min-width: 0;
  }
  .headline {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head-row {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .figure {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .eyebrow {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .value-line {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-wrap: wrap;
    min-width: 0;
  }
  /* The one number the page leads with: 28px, the UI sans, proportional figures. */
  .nw {
    color: var(--text);
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.1;
    overflow-wrap: anywhere;
  }
  .change {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .since {
    color: var(--dim);
    font-size: 12px;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  /* Segmented chips: one frame, the active part a quiet fill. */
  .seg {
    display: inline-flex;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .seg button {
    background: transparent;
    border: none;
    border-left: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--muted);
    padding: 4px 10px;
    font-size: 12px;
    line-height: 1.4;
    cursor: pointer;
    transition: background-color 160ms ease, color 160ms ease;
  }
  .seg.icons button {
    display: inline-flex;
    align-items: center;
    padding: 4px 8px;
  }
  .seg button:first-child {
    border-left: none;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.active {
    background: var(--surface-3);
    color: var(--text);
  }
  .est-line {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .est-mark {
    width: 16px;
    border-top: 2px dashed var(--accent);
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  .warn {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--amber-ink);
    font-size: 12px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--space-3);
  }
  @media (max-width: 1100px) {
    .tiles {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  @media (max-width: 767px) {
    .tiles {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .seg button {
      min-height: 44px;
    }
    .nw {
      font-size: 24px;
    }
  }
</style>
