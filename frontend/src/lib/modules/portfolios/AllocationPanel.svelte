<script>
  // What the portfolio holds now, as a part of the whole: by asset or by asset class as a
  // donut with the total in the centre, by sector or by country as one segmented bar (they
  // run long, so the tail folds into "Other"). Legend rows carry name, value and share,
  // so colour is never the only thing naming a slice. Identity colours come from the
  // shared palette in rank order; red is never one of them.
  import { t } from '$lib/i18n';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import Donut from '$lib/ui/charts/Donut.svelte';
  import Segmented from '$lib/ui/charts/Segmented.svelte';
  import SegChips from './SegChips.svelte';
  import { fmtMoney } from './api.js';

  let { assets = [], currency = 'USD', exposure = null } = $props();

  let by = $state('asset'); // 'asset' | 'class' | 'sector' | 'country'
  const MODES = $derived([
    { key: 'asset', label: $t('portfolios.alloc.byAsset') },
    { key: 'class', label: $t('portfolios.alloc.byClass') },
    ...(exposure
      ? [
          { key: 'sector', label: $t('portfolios.alloc.bySector') },
          { key: 'country', label: $t('portfolios.alloc.byCountry') }
        ]
      : [])
  ]);

  // Positive-value positions only; allocation is about what the portfolio holds now.
  const items = $derived.by(() => {
    const held = assets.filter((a) => (a.market_value ?? 0) > 0);
    if ((by === 'sector' || by === 'country') && exposure?.[by]) {
      // Grouped server-side: user-entered first, then Fundamentals for a stock. What nobody
      // classified is its own named slice, never spread over the others.
      return exposure[by].map((g) => ({
        key: g.key,
        label: g.key === 'unclassified' ? $t('portfolios.alloc.unclassified') : g.key,
        value: g.value
      }));
    }
    if (by === 'class') {
      const m = new Map();
      for (const a of held) m.set(a.asset_class, (m.get(a.asset_class) ?? 0) + a.market_value);
      return [...m.entries()].map(([cls, value]) => ({ key: cls, label: $t(`portfolios.class.${cls}`), value }));
    }
    return held.map((a) => ({ key: a.id, label: a.symbol, value: a.market_value }));
  });

  const ring = $derived(by === 'asset' || by === 'class');
  const money = (v) => fmtMoney(v, currency, 0);
</script>

<ChartFrame
  title={$t('portfolios.alloc.title')}
  height={200}
  unavailable={items.length === 0
    ? { title: $t('portfolios.alloc.emptyTitle'), reason: $t('portfolios.alloc.empty'), icon: 'pie-chart' }
    : null}
>
  {#snippet actions()}
    {#if items.length > 0}
      <SegChips options={MODES} bind:value={by} ariaLabel={$t('portfolios.alloc.title')} />
    {/if}
  {/snippet}
  {#if ring}
    <Donut
      {items}
      max={5}
      size={168}
      thickness={20}
      legend="bottom"
      totalLabel={$t('portfolios.alloc.total')}
      otherLabel={$t('portfolios.alloc.other')}
      format={money}
      ariaLabel={$t('portfolios.alloc.title')}
    />
  {:else}
    <div class="bar">
      <Segmented
        {items}
        max={4}
        size="tall"
        otherLabel={$t('portfolios.alloc.other')}
        format={money}
        ariaLabel={$t('portfolios.alloc.title')}
      />
    </div>
  {/if}
</ChartFrame>

<style>
  .bar {
    padding-top: var(--space-2);
  }
</style>
