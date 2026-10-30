<script>
  // What-if lab: the trader's own closed trades replayed under one change at a time.
  // Exits is the exit-signal lab; Size, Filter, Reverse and Fees share the before and after
  // table, the curves and the per-trade table. Every tab reads only.
  import { onMount } from 'svelte';
  import { journalApi } from './api.js';
  import { LAB_TABS } from './lab.js';
  import { t } from '$lib/i18n';
  import Tabs from '$lib/ui/Tabs.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ExitLab from './ExitLab.svelte';
  import WhatIfPanel from './WhatIfPanel.svelte';
  import MarketBar from './MarketBar.svelte';

  // `tab` and `exitSelection` live in the page so a trip to another view keeps them.
  // `initial` = a scenario handed over by the strategy card ("Open in lab").
  let {
    categoryId = '',
    feeSchedules = [],
    tab = $bindable('exits'),
    exitSelection = $bindable(null),
    initial = null,
    ongofees = null
  } = $props();

  let catalog = $state(null);
  let err = $state('');
  onMount(() => {
    journalApi
      .labCatalog()
      .then((c) => (catalog = c))
      .catch((e) => (err = e.message));
  });

  const tabs = $derived(LAB_TABS.map((id) => ({ id, label: $t(`journal.lab.tab.${id}`) })));

  // The candles the exits and an indicator filter read are downloaded here, on the lab's
  // own scope; a fresh measure remounts the exit lab so its picker reads the new coverage.
  const marketFilter = $derived({ category_id: categoryId || undefined });
  let marketVersion = $state(0);
</script>

<div class="lab">
  <MarketBar filter={marketFilter} onmeasured={() => (marketVersion += 1)} />
  <Tabs {tabs} bind:value={tab} ariaLabel={$t('journal.lab.title')} />
  <ErrorText error={err} />
  {#if tab === 'exits'}
    {#key marketVersion}
      <ExitLab
        {categoryId}
        bind:selected={exitSelection}
        extraSignals={initial?.kind === 'exits' ? initial.signals : null}
      />
    {/key}
  {:else}
    {#key tab}
      <WhatIfPanel
        kind={tab}
        {categoryId}
        {feeSchedules}
        families={catalog?.families ?? []}
        minSample={catalog?.min_sample ?? 20}
        {initial}
        {ongofees}
      />
    {/key}
  {/if}
</div>

<style>
  .lab {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
</style>
