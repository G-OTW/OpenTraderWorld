<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Searchable dataset picker: a trigger showing the current selection opens a panel with a
  // search input and the dataset catalog grouped by asset type. Filters on ticker /
  // timeframe / provider / asset type. Closes on pick, outside click, or Escape.
  let { datasets = [], value = $bindable(null) } = $props();

  let open = $state(false);
  let query = $state('');
  let active = $state(0);
  let root = $state(null);
  let input = $state(null);

  const usable = $derived(datasets.filter((d) => d.bar_count > 0));
  const selected = $derived(usable.find((d) => d.id === value) ?? null);

  // Flat filtered list (for keyboard nav), then regrouped by asset type for display.
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return usable;
    return usable.filter((d) =>
      `${d.ticker} ${d.timeframe} ${d.provider} ${d.asset_type}`.toLowerCase().includes(q)
    );
  });
  const groups = $derived.by(() => {
    const map = new Map();
    for (const d of filtered) {
      if (!map.has(d.asset_type)) map.set(d.asset_type, []);
      map.get(d.asset_type).push(d);
    }
    return [...map.entries()]
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([label, sets]) => ({
        label,
        sets: [...sets].sort((a, b) => a.ticker.localeCompare(b.ticker))
      }));
  });
  const flat = $derived(groups.flatMap((g) => g.sets));

  function openMenu() {
    open = true;
    query = '';
    active = Math.max(0, flat.findIndex((d) => d.id === value));
    queueMicrotask(() => input?.focus());
  }
  function pick(d) {
    value = d.id;
    open = false;
  }
  function onKey(e) {
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, flat.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (flat[active]) pick(flat[active]);
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

  const fmtBars = (n) => $t('backtest.dataset.bars', { count: Number(n).toLocaleString() });
</script>

<svelte:document onclick={onDocClick} />

<div class="picker" bind:this={root} onkeydown={onKey} role="presentation">
  <button type="button" class="trigger" class:placeholder={!selected} onclick={() => (open ? (open = false) : openMenu())}>
    {#if selected}
      <span class="tk">{selected.ticker}</span>
      <span class="chip">{selected.timeframe}</span>
      <span class="meta">{selected.provider} · {fmtBars(selected.bar_count)}</span>
    {:else}
      <span class="meta">{$t('backtest.dataset.select')}</span>
    {/if}
    <span class="caret"><Icon name="chevron-down" size={12} /></span>
  </button>

  {#if open}
    <div class="menu">
      <input
        bind:this={input}
        bind:value={query}
        placeholder={$t('backtest.dataset.searchPlaceholder')}
        aria-label={$t('backtest.dataset.searchAriaLabel')}
      />
      <div class="opts">
        {#each groups as g (g.label)}
          <div class="glabel">{g.label}</div>
          {#each g.sets as d (d.id)}
            {@const i = flat.indexOf(d)}
            <button
              type="button"
              class="opt"
              class:active={i === active}
              class:sel={d.id === value}
              onmouseenter={() => (active = i)}
              onclick={() => pick(d)}
            >
              <span class="tk">{d.ticker}</span>
              <span class="chip">{d.timeframe}</span>
              <span class="meta">{d.provider}</span>
              <span class="bars">{fmtBars(d.bar_count)}</span>
            </button>
          {/each}
        {/each}
        {#if !flat.length}
          <p class="none">{$t('backtest.dataset.noMatch')}</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }
  .trigger {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
  }
  .trigger:hover {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  .trigger.placeholder .meta {
    color: var(--muted);
  }
  .tk {
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .meta {
    color: var(--muted);
    font-size: 0.78rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .caret {
    margin-left: auto;
    color: var(--muted);
    font-size: 0.7rem;
  }
  .menu {
    position: absolute;
    z-index: 30;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
    padding: var(--space-2);
  }
  .menu input {
    width: 100%;
    margin-bottom: var(--space-2);
  }
  .opts {
    max-height: 300px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .glabel {
    font-size: 0.65rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    padding: var(--space-2) var(--space-2) var(--space-1);
    position: sticky;
    top: 0;
    background: var(--surface);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    padding: var(--space-2);
    border-radius: var(--radius);
    cursor: pointer;
    font: inherit;
    font-size: 0.82rem;
  }
  .opt.active {
    background: var(--surface-2);
  }
  .opt.sel .tk {
    color: var(--accent);
  }
  .bars {
    margin-left: auto;
    color: var(--muted);
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
  }
  .none {
    color: var(--muted);
    font-size: 0.82rem;
    padding: var(--space-2);
  }
</style>
