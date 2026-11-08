<script>
  // Which chain columns show, in which order. Saved per browser on every change.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { saveLayout, defaultLayout } from './columns.js';

  let { open = $bindable(false), layout = $bindable() } = $props();

  function commit(next) {
    layout = next;
    saveLayout(next);
  }
  function toggle(k) {
    const visible = layout.visible.includes(k) ? layout.visible.filter((x) => x !== k) : [...layout.visible, k];
    commit({ ...layout, visible });
  }
  function move(i, d) {
    const order = [...layout.order];
    const j = i + d;
    if (j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    commit({ ...layout, order });
  }
</script>

<Modal bind:open title={$t('options.columns.title')} size="sm">
  <ul class="cols">
    {#each layout.order as k, i (k)}
      <li>
        <label>
          <input type="checkbox" checked={layout.visible.includes(k)} onchange={() => toggle(k)} />
          <span>{$t(`options.col.${k}`)}</span>
        </label>
        <button class="icon" onclick={() => move(i, -1)} disabled={i === 0} aria-label={$t('options.columns.up')}><Icon name="chevron-up" size={14} /></button>
        <button class="icon" onclick={() => move(i, 1)} disabled={i === layout.order.length - 1} aria-label={$t('options.columns.down')}><Icon name="chevron-down" size={14} /></button>
      </li>
    {/each}
  </ul>
  {#snippet footer()}
    <button class="ghost" onclick={() => commit(defaultLayout())}>{$t('options.columns.reset')}</button>
    <button class="primary" onclick={() => (open = false)}>{$t('options.close')}</button>
  {/snippet}
</Modal>

<style>
  .cols {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  label {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }
  @media (max-width: 767px) {
    label {
      min-width: 0;
      min-height: 44px;
      overflow-wrap: anywhere;
    }
    li button.icon {
      flex: none;
      width: 44px;
      height: 44px;
    }
  }
</style>
