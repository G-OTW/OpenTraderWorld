<script>
  import { moduleForPath, visibleModules } from './registry';
  import { installedIds } from './installed.js';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let open = $state(false);
  let filter = $state('');
  let filterEl = $state();

  const current = $derived(moduleForPath($page.url.pathname));
  const modules = $derived(visibleModules($installedIds));
  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    if (!f) return modules;
    return modules.filter(
      (m) =>
        m.name.toLowerCase().includes(f) ||
        (m.descKey ? $t(m.descKey) : '').toLowerCase().includes(f)
    );
  });

  function toggle() {
    open = !open;
    filter = '';
    if (open) setTimeout(() => filterEl?.focus(), 0);
  }

  function pick(m) {
    open = false;
    if (m.base !== current.base) goto(m.base);
  }

  // Enter opens the single (or first) match; Escape closes.
  function onFilterKey(e) {
    if (e.key === 'Escape') open = false;
    if (e.key === 'Enter' && shown.length > 0) pick(shown[0]);
  }

  function onWindowClick(e) {
    if (!e.target.closest('.switcher')) open = false;
  }
</script>

<svelte:window on:click={onWindowClick} />

<div class="switcher">
  <button class="current" onclick={toggle} aria-haspopup="listbox" aria-expanded={open}>
    <span class="icon"><Icon name={current.icon} /></span>
    <span class="name">{current.name}</span>
    <span class="chevron" class:open><Icon name="chevron-down" size={14} /></span>
  </button>

  {#if open}
    <div class="menu">
      <input
        class="filter"
        placeholder={$t('switcher.find')}
        bind:this={filterEl}
        bind:value={filter}
        onkeydown={onFilterKey}
      />
      <ul role="listbox">
        {#each shown as m (m.id)}
          <li>
            <button class="item" class:active={m.id === current.id} onclick={() => pick(m)} role="option" aria-selected={m.id === current.id}>
              <span class="icon"><Icon name={m.icon} /></span>
              <span class="label">
                <span class="item-name">{m.name}</span>
                {#if m.descKey}<span class="item-desc">{$t(m.descKey)}</span>{/if}
              </span>
              {#if m.id === current.id}<span class="check"><Icon name="check" size={14} /></span>{/if}
            </button>
          </li>
        {/each}
        {#if shown.length === 0}
          <li class="none">{$t('switcher.none')}</li>
        {/if}
      </ul>
    </div>
  {/if}
</div>

<style>
  .switcher {
    position: relative;
  }

  .current {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-size: 0.9rem;
    font-weight: 600;
  }
  .current:hover {
    border-color: var(--accent);
  }
  .current .icon {
    color: var(--accent);
    display: inline-flex;
    flex-shrink: 0;
  }
  .current .name {
    flex: 1;
    min-width: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chevron {
    color: var(--muted);
    display: inline-flex;
    flex-shrink: 0;
    transition: transform 0.15s;
  }
  .chevron.open {
    transform: rotate(180deg);
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 50;
    min-width: 320px;
    padding: var(--space-1);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .filter {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
  }
  .menu ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: min(60vh, 520px);
    overflow-y: auto;
  }
  .none {
    color: var(--muted);
    font-size: 0.82rem;
    padding: var(--space-2) var(--space-3);
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    text-align: left;
  }
  .item:hover {
    background: var(--surface-2);
  }
  .item.active {
    background: var(--surface-2);
  }
  .item .icon {
    color: var(--accent);
    width: 1.2rem;
    display: inline-flex;
    justify-content: center;
  }
  .label {
    display: flex;
    flex-direction: column;
    flex: 1;
  }
  .item-name {
    font-size: 0.88rem;
    font-weight: 600;
  }
  .item-desc {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .check {
    color: var(--accent);
  }
</style>
