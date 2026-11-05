<script>
  // An order's size: in units (with a step down and up), or worked out from an amount of money
  // (the order ticket's notional and risk, a trade on alert's quote and risk amounts). The way
  // the size is given rides on the label row, as small as the label. Shared by the order
  // ticket and the alert's Trade section, so both read and behave alike.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import TicketBox from './TicketBox.svelte';
  import { t } from '$lib/i18n';

  let {
    label,
    /** `units`, or one of `options`' other values: the size is then `amount`. */
    sizing = $bindable('units'),
    qty = $bindable(1),
    amount = $bindable(''),
    /** [{ value, label }] for the picker; empty or `locked` hides it. */
    options = [],
    locked = false,
    /** Beside the amount: the size it comes to, or why it cannot be worked out. */
    sized = '',
    /** Hover text of the amount box, and its accessible name. */
    amountHint = '',
    amountLabel = ''
  } = $props();

  const units = $derived(sizing === 'units' || locked);
  const bump = (d) => (qty = Math.max(1, (Number(qty) || 0) + d));
</script>

<div class="fld">
  <div class="lab-row">
    <span class="lab">{label}</span>
    {#if options.length > 1 && !locked}
      <span class="sizing"><Dropdown bind:value={sizing} {options} ariaLabel={$t('histviz.trade.sizingLabel')} float /></span>
    {/if}
  </div>
  {#if units}
    <TicketBox>
      <button type="button" class="swap" title={$t('histviz.trade.qtyLess')} aria-label={$t('histviz.trade.qtyLess')} onclick={() => bump(-1)}>
        <Icon name="minus" size={12} />
      </button>
      <input type="number" min="0" step="any" class="qty" bind:value={qty} aria-label={label} />
      <span class="aside">{$t('histviz.trade.units')}</span>
      <button type="button" class="swap" title={$t('histviz.trade.qtyMore')} aria-label={$t('histviz.trade.qtyMore')} onclick={() => bump(1)}>
        <Icon name="plus" size={12} />
      </button>
    </TicketBox>
  {:else}
    <TicketBox title={amountHint || undefined}>
      <input type="number" min="0" step="any" class="qty" bind:value={amount} aria-label={amountLabel || label} />
      <span class="aside">{sized}</span>
    </TicketBox>
  {/if}
</div>

<style>
  .fld {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .lab {
    color: var(--muted);
  }
  .lab-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }
  /* A text trigger, no frame of its own, so the row keeps a label's height. */
  .sizing {
    min-width: 0;
    max-width: 60%;
  }
  .sizing :global(.dd .trigger) {
    height: auto;
    padding: 0 var(--space-1);
    gap: var(--space-1);
    border-color: transparent;
    background: transparent;
    color: var(--muted);
    font-size: inherit;
  }
  .sizing :global(.dd .trigger:hover),
  .sizing :global(.dd .trigger[aria-expanded='true']) {
    color: var(--text);
  }
</style>
