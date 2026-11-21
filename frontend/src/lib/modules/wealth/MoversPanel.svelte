<script>
  // Net worth attribution: which categories moved the total over the headline period, split
  // into revaluation (new revisions, market prices, drift, FX) and assets added or removed.
  // One diverging bar per category around zero, the three parts stacked, the net change at
  // the bar end; the tooltip carries each part.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { wealthApi } from './api.js';
  import { money, signedMoney } from './view.js';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { currency = 'USD', reloadKey = 0, from = null } = $props();

  let data = $state(null);
  let error = $state('');

  $effect(() => {
    void reloadKey;
    const start = from;
    if (!start) return;
    error = '';
    wealthApi
      .movers(start, dateKey())
      .then((r) => (data = r))
      .catch((e) => {
        data = null;
        error = e.message;
      });
  });

  const m = $derived(data?.movers ?? null);
  const label = (c) => c || $t('wealth.page.uncategorized');
  const tiny = (v) => Math.abs(v ?? 0) < 0.005;
  const moved = $derived((m?.categories ?? []).filter((c) => !(tiny(c.valuation) && tiny(c.added) && tiny(c.removed))));
  const still = $derived((m?.categories?.length ?? 0) - moved.length);
  const fmt = (v) => signedMoney(v, currency);
</script>

<div class="panel">
  <ErrorText {error} />
  {#if m}
    <div class="bar">
      <span class="total">
        <span class="lbl">{$t('wealth.movers.total')}</span>
        <Delta value={m.total_delta} format={(v) => money(v, currency)} sensitive />
      </span>
      <span class="effects">
        {$t('wealth.movers.market', { amount: signedMoney(m.market_effect, currency) })}
        {!tiny(m.drift_effect) ? ` · ${$t('wealth.movers.drift', { amount: signedMoney(m.drift_effect, currency) })}` : ''}
        {m.unconverted ? ` · ${$t('wealth.movers.unconverted', { count: m.unconverted })}` : ''}
      </span>
    </div>
    {#if moved.length === 0}
      <EmptyState icon="bar-chart" description={$t('wealth.movers.none')} compact />
    {:else}
      <Bars
        horizontal
        labels="ends"
        categories={moved.map((c) => ({ key: c.category || '__none', label: label(c.category) }))}
        series={[
          { key: 'valuation', label: $t('wealth.movers.valuation'), values: moved.map((c) => c.valuation) },
          { key: 'added', label: $t('wealth.movers.added'), values: moved.map((c) => c.added) },
          { key: 'removed', label: $t('wealth.movers.removed'), values: moved.map((c) => c.removed) }
        ]}
        format={fmt}
        totalLabel={$t('wealth.movers.change')}
        ariaLabel={$t('wealth.movers.tab')}
      />
      {#if still > 0}<p class="note">{$t('wealth.movers.unchanged', { n: still })}</p>{/if}
    {/if}
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
  }
  .total {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .lbl {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .effects,
  .note {
    margin: 0;
    color: var(--dim);
    font-size: 12px;
  }
</style>
