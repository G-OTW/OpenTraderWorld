<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Searchable single-select for a Historical Data dataset. Type to filter by ticker /
  // timeframe / provider, pick with click or Enter. Binds `value` to the dataset id.
  import { dsLabel } from './api.js';
  import { t } from '$lib/i18n';

  let {
    value = $bindable(null),
    datasets = [],
    placeholder = undefined,
    onchange = () => {}
  } = $props();

  let open = $state(false);
  let query = $state('');
  let active = $state(0);
  let root = $state(null);
  let input = $state(null);

  const selected = $derived(datasets.find((d) => d.id === value) ?? null);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return datasets;
    return datasets.filter((d) =>
      `${d.ticker} ${d.timeframe} ${d.provider ?? ''}`.toLowerCase().includes(q)
    );
  });

  function openMenu() {
    open = true;
    query = '';
    active = 0;
    queueMicrotask(() => input?.focus());
  }
  function pick(d) {
    value = d.id;
    onchange(d.id);
    open = false;
  }
  function onKey(e) {
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, filtered.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered[active]) pick(filtered[active]);
    } else if (e.key === 'Escape') {
      open = false;
    }
  }
  $effect(() => {
    query;
    active = 0;
  });

  function onDocClick(e) {
    if (root && !root.contains(e.target)) open = false;
  }
</script>

<svelte:document onclick={onDocClick} />

<div class="combo" class:set={value} bind:this={root} onkeydown={onKey} role="presentation">
  <button type="button" class="trigger" class:placeholder={!selected} onclick={() => (open ? (open = false) : openMenu())}>
    <span class="lead"><Icon name="database" size={15} strokeWidth={1.8} /></span>
    <span class="trig-body">
      {#if selected}
        <span class="trig-label">{selected.ticker} · {selected.timeframe}</span>
        {#if selected.provider}<span class="trig-sub">{selected.provider}</span>{/if}
      {:else}
        <span class="trig-label">{$t('quant.datasetPicker.choose')}</span>
      {/if}
    </span>
    <span class="caret" class:up={open}><Icon name="chevron-down" size={14} /></span>
  </button>
  {#if open}
    <div class="menu">
      <input
        class="combo-search"
        bind:this={input}
        bind:value={query}
        placeholder={placeholder ?? $t('quant.datasetPicker.searchPlaceholder')}
        aria-label={$t('quant.datasetPicker.searchAriaLabel')}
      />
      <ul class="opts">
        {#each filtered as d, i (d.id)}
          <li>
            <button
              type="button"
              class="opt"
              class:active={i === active}
              class:sel={d.id === value}
              onmouseenter={() => (active = i)}
              onclick={() => pick(d)}
            >
              {dsLabel(d)}{#if d.provider}<span class="prov"> · {d.provider}</span>{/if}
            </button>
          </li>
        {/each}
        {#if filtered.length === 0}
          <li class="none">{$t('quant.datasetPicker.noMatches')}</li>
        {/if}
      </ul>
    </div>
  {/if}
</div>

<style>
  .combo {
    position: relative;
    display: inline-block;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    color: var(--text);
    padding: 0 var(--space-3);
    height: 44px;
    font: inherit;
    font-size: var(--text-base);
    cursor: pointer;
    min-width: 260px;
    transition:
      border-color var(--dur-fast) var(--ease),
      background var(--dur-fast) var(--ease);
  }
  .trigger:hover {
    background: var(--surface);
  }
  .combo.set .trigger {
    border-color: var(--border-control);
    background: var(--surface);
  }
  .trigger:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: 0;
  }
  /* Leading icon chip. */
  .lead {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--accent);
  }
  .trigger.placeholder .lead {
    background: var(--surface-2);
    color: var(--muted);
  }
  .trig-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    text-align: left;
    line-height: 1.2;
  }
  .trig-label {
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trigger.placeholder .trig-label {
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .trig-sub {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .caret {
    flex: none;
    color: var(--muted);
    display: inline-flex;
    transition: transform 0.18s ease;
  }
  .caret.up {
    transform: rotate(180deg);
  }
  .menu {
    position: absolute;
    z-index: var(--z-dropdown);
    top: calc(100% + 6px);
    left: 0;
    min-width: 280px;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    padding: var(--space-2);
  }
  .combo-search {
    width: 100%;
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    font: inherit;
    font-size: var(--text-base);
    margin-bottom: var(--space-2);
  }
  .opts {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 260px;
    overflow-y: auto;
  }
  .opt {
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-base);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .opt.active {
    background: var(--surface-2);
  }
  .opt.sel {
    color: var(--accent);
  }
  .prov {
    color: var(--muted);
  }
  .none {
    color: var(--muted);
    font-size: var(--text-base);
    padding: var(--space-2) var(--space-3);
  }
</style>
