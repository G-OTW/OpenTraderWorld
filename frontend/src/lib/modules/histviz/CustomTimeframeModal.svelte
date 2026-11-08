<script>
  // Custom timeframes: any period a served timeframe divides (2m, 7m, 10m, 3h, 2w…). The list
  // is global (chart settings); the chart consolidates each one from its base on read.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { consolidationBase, byDuration } from './pane.js';

  let { open = $bindable(false), served = [], custom = [], onchange, onpick } = $props();

  let count = $state(2);
  let unit = $state('m');

  const label = $derived(Number.isInteger(+count) && +count > 0 ? `${+count}${unit}` : '');
  const base = $derived(label ? consolidationBase(served, label) : null);
  const problem = $derived.by(() => {
    if (!label) return $t('histviz.customTf.badCount');
    if (served.includes(label)) return $t('histviz.customTf.served', { tf: label });
    if (custom.includes(label)) return $t('histviz.customTf.exists', { tf: label });
    if (!base) return $t('histviz.customTf.noBase', { tf: label });
    return '';
  });

  function add() {
    if (problem) return;
    onchange?.([...custom, label].sort(byDuration));
    onpick?.(label);
    open = false;
  }
</script>

<Modal bind:open title={$t('histviz.customTf.title')} size="sm">
  <p class="hint">{$t('histviz.customTf.hint')}</p>
  <div class="row">
    <input type="number" min="1" step="1" bind:value={count} aria-label={$t('histviz.customTf.count')} />
    <div class="unit">
      <Dropdown
        bind:value={unit}
        ariaLabel={$t('histviz.customTf.unit')}
        float
        options={['m', 'h', 'd', 'w'].map((u) => ({ value: u, label: $t(`histviz.customTf.unit.${u}`) }))}
      />
    </div>
    <Button variant="primary" disabled={!!problem} onclick={add}>{$t('histviz.customTf.add')}</Button>
  </div>
  <p class="status" class:bad={!!problem}>
    {problem || $t('histviz.customTf.builtFrom', { tf: label, base })}
  </p>

  {#if custom.length}
    <p class="grp">{$t('histviz.customTf.yours')}</p>
    <ul>
      {#each custom as tf (tf)}
        {@const from = consolidationBase(served, tf)}
        <li>
          <span class="tf">{tf}</span>
          <span class="from">{from ? $t('histviz.customTf.fromShort', { base: from }) : $t('histviz.customTf.unavailable')}</span>
          <button
            class="rm"
            title={$t('histviz.customTf.remove')}
            aria-label={$t('histviz.customTf.remove')}
            onclick={() => onchange?.(custom.filter((x) => x !== tf))}><Icon name="trash" size={12} /></button
          >
        </li>
      {/each}
    </ul>
  {/if}
</Modal>

<style>
  .hint {
    margin: 0 0 var(--space-3);
    font-size: var(--fs-desc);
    color: var(--dim);
    line-height: 1.4;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .row input {
    width: 80px;
  }
  .unit {
    flex: 1;
    min-width: 0;
  }
  .status {
    margin: var(--space-2) 0 0;
    font-size: var(--fs-desc);
    color: var(--muted);
  }
  .status.bad {
    color: var(--amber);
  }
  .grp {
    margin: var(--space-4) 0 var(--space-2);
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
  }
  ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--fs-body);
  }
  .tf {
    font-family: var(--mono);
    min-width: 48px;
  }
  .from {
    flex: 1;
    color: var(--dim);
  }
  .rm {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .rm:hover {
    color: var(--red);
  }
</style>
