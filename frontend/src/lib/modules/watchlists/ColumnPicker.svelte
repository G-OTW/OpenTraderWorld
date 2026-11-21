<script>
  // Header gear: which columns this list shows. Saved on the list, so every list keeps its
  // own table. Order follows the catalog, not the click order.
  import Icon from '$lib/ui/Icon.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { COLUMN_DEFS } from './views.js';
  import { t } from '$lib/i18n';

  let { columns = [], onchange = () => {} } = $props();
  let open = $state(false);

  function toggle(id) {
    const on = new Set(columns);
    if (on.has(id)) on.delete(id);
    else on.add(id);
    onchange(COLUMN_DEFS.map((c) => c.id).filter((c) => on.has(c)));
  }
</script>

<div class="picker" use:clickOutside={() => (open = false)}>
  <button
    class="icon"
    class:on={open}
    onclick={() => (open = !open)}
    aria-label={$t('watchlists.col.pickerAria')}
    title={$t('watchlists.col.pickerAria')}
    aria-expanded={open}
  >
    <Icon name="settings" size={14} />
  </button>
  {#if open}
    <div class="menu" role="menu">
      {#each COLUMN_DEFS as c (c.id)}
        <label class="opt">
          <input type="checkbox" checked={columns.includes(c.id)} onchange={() => toggle(c.id)} />
          {$t(c.key)}
        </label>
      {/each}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    display: inline-flex;
  }
  .icon.on {
    color: var(--text);
    background: var(--surface-hover);
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + var(--space-1));
    z-index: var(--z-dropdown);
    min-width: 200px;
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    padding: var(--space-1);
    display: flex;
    flex-direction: column;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    font-size: 12px;
    color: var(--text);
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    cursor: pointer;
    white-space: nowrap;
    text-transform: none;
    letter-spacing: normal;
  }
  .opt:hover {
    background: var(--surface-hover);
  }
</style>
