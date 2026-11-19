<script>
  // The robot's money: the capital it trades with, the share of it in one position, its
  // leverage and its entries per position size the strategy's every decision on the server.
  // How many positions and orders a day it may take cap what it sends. Every field is
  // optional. A broker robot sends one entry per position, so it has no entries field.
  import Input from '$lib/ui/Input.svelte';
  import { t } from '$lib/i18n';

  let { capital = $bindable({}), currency = '', broker = false } = $props();

  $effect(() => {
    if (!capital) capital = {};
  });
  // A broker robot sends one entry per position: a value left from the simulated choice goes.
  $effect(() => {
    if (broker && capital?.pyramiding) capital.pyramiding = '';
  });
  const cap = $derived.by(() => {
    const a = Number(capital?.allocation);
    const p = Number(capital?.percent_in || 100);
    return Number.isFinite(a) && a > 0 ? (a * p) / 100 : null;
  });
</script>

{#if capital}
  <div class="grid">
    <Input
      label={$t('robots.capital.allocation')}
      bind:value={capital.allocation}
      inputmode="decimal"
      hint={$t('robots.capital.allocationHint')}
    />
    <Input
      label={$t('robots.capital.currency')}
      bind:value={capital.currency}
      placeholder={currency}
      hint={$t('robots.capital.currencyHint')}
    />
    <Input
      label={$t('robots.capital.percentIn')}
      bind:value={capital.percent_in}
      inputmode="decimal"
      hint={$t('robots.capital.percentInHint')}
    />
    <Input
      label={$t('robots.capital.leverage')}
      bind:value={capital.leverage}
      inputmode="decimal"
      hint={$t('robots.capital.leverageHint')}
    />
    {#if !broker}
      <Input
        label={$t('robots.capital.pyramiding')}
        type="number"
        bind:value={capital.pyramiding}
        hint={$t('robots.capital.pyramidingHint')}
      />
    {/if}
    <Input
      label={$t('robots.capital.maxPositions')}
      type="number"
      bind:value={capital.max_positions}
      hint={$t('robots.capital.maxPositionsHint')}
    />
    <Input
      label={$t('robots.capital.maxOrders')}
      type="number"
      bind:value={capital.max_orders_per_day}
      hint={$t('robots.capital.maxOrdersHint')}
    />
  </div>
  {#if cap !== null}
    <p class="note">{$t('robots.capital.capNote', { cap: cap.toLocaleString(), currency: capital.currency || currency })}</p>
  {:else}
    <p class="note">{$t('robots.capital.noCap')}</p>
  {/if}
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  .note {
    margin: var(--space-3) 0 0;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  @media (max-width: 560px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
