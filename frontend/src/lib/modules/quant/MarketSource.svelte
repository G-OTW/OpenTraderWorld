<script>
  // Which provider account a derivatives tab reads: connectors granted to Quant that list
  // futures contracts or option chains (Interactive Brokers, Massive), plus the shared
  // button to create or grant one.
  import { connectorsApi, allows, isReady } from '$lib/connectors/api.js';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';

  let { value = $bindable(null), providers = ['ibkr', 'massive'], provider = $bindable(null) } = $props();
  let connectors = $state([]);

  async function load() {
    try {
      connectors = await connectorsApi.list();
    } catch {
      connectors = [];
    }
  }
  $effect(() => {
    load();
  });

  const usable = $derived(connectors.filter((c) => providers.includes(c.provider) && isReady(c) && allows(c, 'quant')));
  $effect(() => {
    if (usable.length && !usable.some((c) => c.id === value)) value = usable[0].id;
  });
  $effect(() => {
    provider = usable.find((c) => c.id === value)?.provider ?? null;
  });
</script>

<div class="qp-field src">
  <span class="qp-label">{$t('quant.market.source')}</span>
  <div class="row">
    {#if usable.length}
      <Dropdown bind:value ariaLabel={$t('quant.market.source')} options={usable.map((c) => ({ value: c.id, label: c.name }))} />
    {:else}
      <span class="qp-hint">{$t('quant.market.noSource')}</span>
    {/if}
    <ConnectorButton module="quant" onchanged={load} />
  </div>
</div>

<style>
  .src {
    flex-basis: 320px;
    min-width: 0;
  }
  /* Dropdown and connector button share one row at control height. */
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .row > :global(.dd) {
    flex: 1 1 auto;
    min-width: 0;
  }
  .row > .qp-hint {
    flex: 1 1 auto;
  }
  .row > :global(.cbtn) {
    flex: none;
    height: var(--control-h);
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

</style>
