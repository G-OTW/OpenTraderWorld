<script>
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  // A grouped, searchable picker meant to replace a long grouped <select>. Shows the current
  // selection as a trigger button; clicking opens a panel with a search box on top and a
  // height-capped, scrollable list of options grouped under sticky headers. Typing filters across
  // all groups; empty query shows the full grouped list (so it still reads like the old dropdown).
  // Keyboard: ↑/↓ move, Enter picks, Esc closes. Closes on outside-click.
  //
  // `groups`: [{ label, items: [{ value, label, hint? }] }]. `value` is the selected option value.
  let {
    value = $bindable(''),
    groups = [],
    placeholder = '',
    ariaLabel = '',
    disabled = false,
    onchange = () => {}
  } = $props();

  let open = $state(false);
  let query = $state('');
  let active = $state(0); // index into the flat `visible` list
  let root = $state(null);
  let input = $state(null);
  let listEl = $state(null);

  const allItems = $derived(groups.flatMap((g) => g.items));
  const selected = $derived(allItems.find((it) => it.value === value) ?? null);

  // Grouped view respecting the query: each group keeps only its matching items; empty groups drop.
  const view = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const match = (it) =>
      !q || it.label.toLowerCase().includes(q) || (it.hint ?? '').toLowerCase().includes(q);
    return groups
      .map((g) => ({ label: g.label, items: g.items.filter(match) }))
      .filter((g) => g.items.length);
  });
  // Flat list of selectable items in view order, for keyboard navigation + Enter.
  const visible = $derived(view.flatMap((g) => g.items));

  function openMenu() {
    if (disabled) return;
    open = true;
    query = '';
    active = Math.max(0, visible.findIndex((it) => it.value === value));
    queueMicrotask(() => input?.focus());
  }
  function pick(v) {
    value = v;
    onchange(v);
    open = false;
  }
  function scrollActiveIntoView() {
    queueMicrotask(() => listEl?.querySelector('.opt.active')?.scrollIntoView({ block: 'nearest' }));
  }
  function onKey(e) {
    if (!open) {
      if (e.key === 'Enter' || e.key === 'ArrowDown') {
        e.preventDefault();
        openMenu();
      }
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, visible.length - 1);
      scrollActiveIntoView();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
      scrollActiveIntoView();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (visible[active]) pick(visible[active].value);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      open = false;
    }
  }
  // Reset highlight to the top of the list whenever the query changes the result set.
  $effect(() => {
    query;
    active = 0;
  });

  function onDocClick(e) {
    if (root && !root.contains(e.target)) open = false;
  }
</script>

<svelte:document onclick={onDocClick} />

<div class="picker" bind:this={root} onkeydown={onKey} role="presentation">
  <button
    type="button"
    class="trigger"
    class:open
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel}
    onclick={() => (open ? (open = false) : openMenu())}
  >
    <span class="trig-label">{selected?.label ?? placeholder}</span>
    <span class="caret"><Icon name="chevron-down" size={13} /></span>
  </button>

  {#if open}
    <div class="menu" role="listbox">
      <div class="search-wrap">
        <Icon name="search" size={13} />
        <input
          class="search"
          bind:this={input}
          bind:value={query}
          placeholder={placeholder || $t('common.search')}
          aria-label={ariaLabel || $t('common.search')}
        />
      </div>
      <div class="list" bind:this={listEl}>
        {#each view as g (g.label)}
          <div class="group">
            <div class="group-head">{g.label}</div>
            {#each g.items as it (it.value)}
              {@const idx = visible.indexOf(it)}
              <button
                type="button"
                class="opt"
                class:active={idx === active}
                class:sel={it.value === value}
                role="option"
                aria-selected={it.value === value}
                onmouseenter={() => (active = idx)}
                onclick={() => pick(it.value)}
              >
                <span class="opt-label">{it.label}</span>
                {#if it.hint}<span class="opt-hint">{it.hint}</span>{/if}
              </button>
            {/each}
          </div>
        {/each}
        {#if !visible.length}
          <div class="none">{$t('common.noMatches')}</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    display: inline-block;
    min-width: 0;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-1);
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    height: 30px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .trigger:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  }
  /* Open: just tint the border. No halo — the panel below is the open-state cue, and a ring here
     would stack with the search field's border into a doubled fading edge. */
  .trigger.open {
    border-color: var(--accent);
  }
  .trigger:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .trigger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .trig-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .caret {
    color: var(--muted);
    display: inline-flex;
    flex-shrink: 0;
  }

  .menu {
    position: absolute;
    z-index: 40;
    top: calc(100% + 4px);
    left: 0;
    min-width: max(100%, 220px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, var(--radius));
    box-shadow: var(--shadow-2, 0 16px 40px -20px rgba(0, 0, 0, 0.5));
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .search-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0 var(--space-2);
    color: var(--muted);
  }
  /* Single focus indicator: the wrapper's border. The input itself is chrome-less so the global
     input:focus ring (a soft accent halo) doesn't stack a second fading border on top. */
  .search-wrap:focus-within {
    border-color: var(--accent);
  }
  .search {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    height: 30px;
    padding: 0;
  }
  .search:focus,
  .picker .search:focus {
    outline: none;
    border: none;
    box-shadow: none;
  }

  .list {
    max-height: 280px;
    overflow-y: auto;
    margin: 0 calc(-1 * var(--space-1));
    padding: 0 var(--space-1);
  }
  .group + .group {
    margin-top: var(--space-1);
  }
  .group-head {
    position: sticky;
    top: 0;
    background: var(--surface);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    font-weight: var(--fw-semibold);
    padding: var(--space-1) var(--space-2) 3px;
    z-index: 1;
  }
  .opt {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-sm);
  }
  .opt.active {
    background: var(--surface-2);
  }
  .opt.sel {
    color: var(--accent);
  }
  .opt.sel::after {
    content: '✓';
    color: var(--accent);
    font-size: var(--text-xs);
  }
  .opt-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .opt-hint {
    color: var(--muted);
    font-family: var(--mono, ui-monospace, monospace);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  .none {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2);
    text-align: center;
  }
</style>
