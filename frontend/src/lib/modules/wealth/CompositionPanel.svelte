<script>
  // What the net worth is made of: one tall segmented bar by category (six named parts,
  // the rest folded into Other) with its legend rows (value, share), and, when more than one
  // household member holds something, the owner split as a thin bar.
  import Segmented from '$lib/ui/charts/Segmented.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { money } from './view.js';
  import { t } from '$lib/i18n';

  let {
    byCategory = {},
    byOwner = {},
    currency = 'USD',
    /** Category name -> identity colour, stable across filters (the page keeps it). */
    colorFor = () => undefined
  } = $props();

  const label = (c) => c || $t('wealth.page.uncategorized');
  const cats = $derived(
    Object.entries(byCategory ?? {}).map(([k, v]) => ({ key: k || '__none', label: label(k), value: v, color: colorFor(k) }))
  );
  const owners = $derived(
    Object.entries(byOwner ?? {}).map(([k, v]) => ({ key: k || '__none', label: k || $t('wealth.owner.none'), value: v }))
  );
  const anyPositive = $derived(cats.some((c) => c.value > 0));
  const fmt = (v) => money(v, currency);
</script>

<section class="box comp">
  <header class="head">
    <h3>{$t('wealth.composition.title')}</h3>
    <span class="sub">{$t('wealth.composition.byCategory')}</span>
  </header>
  {#if anyPositive}
    <Segmented items={cats} max={6} size="tall" otherLabel={$t('wealth.composition.other')} format={fmt} ariaLabel={$t('wealth.composition.byCategory')} />
  {:else}
    <EmptyState icon="pie-chart" description={$t('wealth.categoryBars.empty')} compact />
  {/if}
  {#if owners.length > 1}
    <div class="owners">
      <span class="eyebrow">{$t('wealth.composition.byOwner')}</span>
      <Segmented items={owners} max={4} size="thin" otherLabel={$t('wealth.composition.other')} format={fmt} ariaLabel={$t('wealth.composition.byOwner')} />
    </div>
  {/if}
</section>

<style>
  .box {
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    padding: var(--space-4);
    min-width: 0;
  }
  .comp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  .sub {
    color: var(--dim);
    font-size: 12px;
  }
  .owners {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .eyebrow {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
</style>
