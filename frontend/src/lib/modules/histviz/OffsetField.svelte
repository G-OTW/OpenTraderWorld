<script>
  // A price given as a distance, for an order priced when it is sent: an amount, a percentage,
  // ticks of the instrument, or multiples of its ATR. How the distance is given rides on the
  // label row like the size's unit; an exit can be switched off. Built on the order ticket's
  // own field (TicketBox), so the Trade section of an alert reads like the ticket.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import TicketBox from './TicketBox.svelte';
  import { t } from '$lib/i18n';

  let {
    label,
    /** { kind: 'abs' | 'pct' | 'ticks' | 'atr', value } */
    offset = $bindable({ kind: 'pct', value: '' }),
    kinds = ['abs', 'pct', 'ticks', 'atr'],
    /** Show an on/off switch (an exit), bound to `on`. */
    switchable = false,
    on = $bindable(true),
    /** Beside the value: the price it comes to at the last preview, when known. */
    note = '',
    hint = ''
  } = $props();

  const options = $derived(kinds.map((k) => ({ value: k, label: $t(`histviz.alerts.trade.kind.${k}`) })));
  const unit = $derived($t(`histviz.alerts.trade.unit.${offset.kind}`));
</script>

<div class="fld" title={hint || undefined}>
  <div class="lab-row">
    <span class="lab">{label}</span>
    <span class="right">
      <span class="kind"><Dropdown bind:value={offset.kind} {options} ariaLabel={label} float /></span>
      {#if switchable}
        <button type="button" class="sw" role="switch" aria-checked={on} aria-label={label} onclick={() => (on = !on)}></button>
      {/if}
    </span>
  </div>
  <TicketBox off={switchable && !on}>
    <input type="number" step="any" bind:value={offset.value} disabled={switchable && !on} aria-label={label} />
    <span class="aside">{note || unit}</span>
  </TicketBox>
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
  .right {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  /* A text trigger, no frame of its own, so the row keeps a label's height. */
  .kind {
    min-width: 0;
  }
  .kind :global(.dd .trigger) {
    height: auto;
    padding: 0 var(--space-1);
    gap: var(--space-1);
    border-color: transparent;
    background: transparent;
    color: var(--muted);
    font-size: inherit;
  }
  .kind :global(.dd .trigger:hover),
  .kind :global(.dd .trigger[aria-expanded='true']) {
    color: var(--text);
  }
  .sw {
    position: relative;
    flex: none;
    width: 28px;
    height: 16px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: var(--border-control);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease);
  }
  .sw::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    transition: transform var(--dur-fast) var(--ease);
  }
  .sw[aria-checked='true'] {
    background: var(--accent);
  }
  .sw[aria-checked='true']::after {
    transform: translateX(12px);
  }
</style>
