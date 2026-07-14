<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Multi-select dataset picker (2–8 for a portfolio run). Selected datasets show as removable
  // chips; a searchable panel adds more, disabling ones whose timeframe differs from the first
  // pick (a portfolio run requires one shared timeframe). Same look as DatasetSelect.
  let { datasets = [], values = $bindable([]) } = $props();

  let open = $state(false);
  let query = $state('');
  let active = $state(0);
  let root = $state(null);
  let input = $state(null);

  const MAX = 8;
  const usable = $derived(datasets.filter((d) => d.bar_count > 0));
  const selected = $derived(values.map((id) => usable.find((d) => d.id === id)).filter(Boolean));
  // Timeframe locked to the first selection; others of a different tf can't be added.
  const lockedTf = $derived(selected[0]?.timeframe ?? null);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const base = usable.filter((d) => !values.includes(d.id));
    if (!q) return base;
    return base.filter((d) =>
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

  const canAdd = (d) => values.length < MAX && (!lockedTf || d.timeframe === lockedTf);

  function openMenu() {
    open = true;
    query = '';
    active = 0;
    queueMicrotask(() => input?.focus());
  }
  function add(d) {
    if (!canAdd(d)) return;
    values = [...values, d.id];
    query = '';
    if (values.length >= MAX) open = false;
  }
  function remove(id) {
    values = values.filter((v) => v !== id);
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
      if (flat[active]) add(flat[active]);
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
  {#if selected.length}
    <div class="chips">
      {#each selected as d (d.id)}
        <span class="dchip">
          <span class="tk">{d.ticker}</span>
          <span class="tf">{d.timeframe}</span>
          <button type="button" class="rm" title={$t('common.remove')} onclick={() => remove(d.id)}>
            <Icon name="x" size={10} />
          </button>
        </span>
      {/each}
    </div>
  {/if}

  <button
    type="button"
    class="trigger"
    class:placeholder={!selected.length}
    disabled={values.length >= MAX}
    onclick={() => (open ? (open = false) : openMenu())}
  >
    <Icon name="plus" size={12} />
    <span class="meta">
      {selected.length ? $t('backtest.multi.addAnother') : $t('backtest.multi.addDatasets')}
      {#if selected.length}<span class="count">{selected.length}/{MAX}</span>{/if}
    </span>
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
      {#if lockedTf}
        <p class="tf-note">{$t('backtest.multi.tfLocked', { tf: lockedTf })}</p>
      {/if}
      <div class="opts">
        {#each groups as g (g.label)}
          <div class="glabel">{g.label}</div>
          {#each g.sets as d (d.id)}
            {@const i = flat.indexOf(d)}
            {@const allowed = canAdd(d)}
            <button
              type="button"
              class="opt"
              class:active={i === active}
              disabled={!allowed}
              onmouseenter={() => (active = i)}
              onclick={() => add(d)}
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
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .dchip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px var(--space-1) 2px var(--space-2);
    font-size: var(--text-xs);
  }
  .dchip .tk {
    font-weight: var(--fw-semibold);
  }
  .dchip .tf {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .rm {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 1px;
    border-radius: 999px;
  }
  .rm:hover {
    color: var(--red);
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
    font-size: var(--text-base);
    cursor: pointer;
    text-align: left;
  }
  .trigger:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  .trigger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .trigger.placeholder .meta {
    color: var(--muted);
  }
  .meta {
    color: var(--muted);
    font-size: var(--text-sm);
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .count {
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
  }
  .caret {
    margin-left: auto;
    color: var(--muted);
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
  .tf-note {
    font-size: var(--text-xs);
    color: var(--muted);
    margin-bottom: var(--space-1);
  }
  .opts {
    max-height: 280px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .glabel {
    font-size: 0.65rem;
    font-weight: var(--fw-semibold);
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
    font-size: var(--text-sm);
  }
  .opt:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .opt.active:not(:disabled) {
    background: var(--surface-2);
  }
  .tk {
    font-weight: var(--fw-semibold);
    letter-spacing: 0.02em;
  }
  .bars {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .none {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2);
  }
</style>
