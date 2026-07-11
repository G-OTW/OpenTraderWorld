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
  // Index of the keyboard-highlighted row. Enter used to fire `shown[0]` blind —
  // you couldn't see what you were about to open. Now the highlight is visible
  // and the arrows move it.
  let cursor = $state(0);

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

  // Typing narrows the list under the cursor; snap it back to the top match.
  $effect(() => {
    filter;
    cursor = 0;
  });

  function toggle() {
    open = !open;
    filter = '';
    cursor = 0;
    if (open) setTimeout(() => filterEl?.focus(), 0);
  }

  function pick(m) {
    open = false;
    if (m.base !== current.base) goto(m.base);
  }

  // Arrows move the highlight (wrapping), Enter opens it, Escape closes.
  function onFilterKey(e) {
    const last = shown.length - 1;
    if (e.key === 'Escape') {
      open = false;
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      cursor = cursor >= last ? 0 : cursor + 1;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      cursor = cursor <= 0 ? last : cursor - 1;
    } else if (e.key === 'Home') {
      e.preventDefault();
      cursor = 0;
    } else if (e.key === 'End') {
      e.preventDefault();
      cursor = last;
    } else if (e.key === 'Enter' && shown[cursor]) {
      pick(shown[cursor]);
    }
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
      <!-- The input keeps focus; the highlighted row is announced through
           aria-activedescendant, which is how a combobox is meant to work. -->
      <input
        class="filter"
        placeholder={$t('switcher.find')}
        role="combobox"
        aria-expanded="true"
        aria-controls="switcher-list"
        aria-activedescendant={shown[cursor] ? `switcher-opt-${shown[cursor].id}` : undefined}
        bind:this={filterEl}
        bind:value={filter}
        onkeydown={onFilterKey}
      />
      <ul id="switcher-list" role="listbox">
        {#each shown as m, i (m.id)}
          <li>
            <button
              id="switcher-opt-{m.id}"
              class="item"
              class:active={m.id === current.id}
              class:cursor={i === cursor}
              onclick={() => pick(m)}
              onmouseenter={() => (cursor = i)}
              role="option"
              tabindex="-1"
              aria-selected={m.id === current.id}
            >
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
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
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
    transition: transform var(--dur-fast) var(--ease);
  }
  .chevron.open {
    transform: rotate(180deg);
  }

  /* Level 2: it floats over the page, so it carries a shadow and no border. */
  .menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: var(--z-dropdown);
    min-width: 320px;
    padding: var(--space-1);
    background: var(--surface);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .filter {
    font-size: var(--text-sm);
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
    font-size: var(--text-xs);
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
  /* Hover and the keyboard cursor land on the same visual: one highlight, whether
     you got there with the mouse or the arrows. `.active` (the current module) is
     marked by the check, not by the background. */
  .item.cursor {
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
    min-width: 0;
  }
  .item-name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .item-desc {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .check {
    color: var(--accent);
  }
</style>
