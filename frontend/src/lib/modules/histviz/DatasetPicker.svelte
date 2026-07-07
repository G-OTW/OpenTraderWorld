<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Searchable dataset picker. A search bar (ticker/provider) plus basic asset-type and
  // timeframe filters narrow the stored datasets; only matches are listed (no full tree).
  let { datasets = [], selectedId = null, onselect, ondownload } = $props();

  let q = $state('');
  let assetType = $state('');
  let timeframe = $state('');

  const assetTypes = $derived([...new Set(datasets.map((d) => d.asset_type))].sort());
  const timeframes = $derived([...new Set(datasets.map((d) => d.timeframe))].sort());

  const results = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    return datasets
      .filter((d) => {
        if (assetType && d.asset_type !== assetType) return false;
        if (timeframe && d.timeframe !== timeframe) return false;
        if (needle) {
          const hay = `${d.ticker} ${d.provider} ${d.asset_type} ${d.timeframe}`.toLowerCase();
          if (!hay.includes(needle)) return false;
        }
        return true;
      })
      .sort((a, b) => a.ticker.localeCompare(b.ticker) || a.timeframe.localeCompare(b.timeframe));
  });

  const filtered = $derived(!!(q.trim() || assetType || timeframe));

  function meta(d) {
    return d.bar_count
      ? $t('histviz.picker.metaBars', {
          timeframe: d.timeframe,
          provider: d.provider,
          bars: d.bar_count.toLocaleString()
        })
      : $t('histviz.picker.meta', { timeframe: d.timeframe, provider: d.provider });
  }
</script>

<div class="picker">
  <div class="head">
    <span>{$t('histviz.picker.dataset')}</span>
    <button class="download" onclick={() => ondownload?.()} title={$t('histviz.picker.downloadTitle')}>
      <Icon name="download" size={12} /> {$t('histviz.picker.download')}
    </button>
  </div>

  {#if !datasets.length}
    <p class="empty">{$t('histviz.picker.empty')}</p>
  {:else}
    <div class="search-wrap">
      <span class="search-ic"><Icon name="search" size={13} /></span>
      <input class="search" type="search" placeholder={$t('histviz.picker.searchPlaceholder')} bind:value={q} />
    </div>
    <div class="filters">
      <select bind:value={assetType} title={$t('histviz.picker.assetType')}>
        <option value="">{$t('histviz.picker.allTypes')}</option>
        {#each assetTypes as t (t)}<option value={t}>{t}</option>{/each}
      </select>
      <select bind:value={timeframe} title={$t('histviz.picker.timeframe')}>
        <option value="">{$t('histviz.picker.anyTf')}</option>
        {#each timeframes as t (t)}<option value={t}>{t}</option>{/each}
      </select>
    </div>

    <div class="results">
      {#if !filtered}
        <div class="placeholder">
          <Icon name="search" size={18} strokeWidth={1.7} />
          <span>{$t('histviz.picker.searchHint')}</span>
        </div>
      {:else if !results.length}
        <div class="placeholder">
          <Icon name="database" size={18} strokeWidth={1.7} />
          <span>{$t('histviz.picker.noMatches')}</span>
        </div>
      {:else}
        {#each results as d (d.id)}
          <button
            class="leaf"
            class:active={d.id === selectedId}
            disabled={!d.bar_count}
            title={d.bar_count ? '' : $t('histviz.picker.noBars')}
            onclick={() => onselect?.(d)}
          >
            <span class="ticker">{d.ticker}</span>
            <span class="meta">{meta(d)}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .head > span {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.05em;
    color: var(--muted);
    line-height: 1;
  }
  .download {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 var(--space-3);
    border-radius: 999px;
    border: 1px solid var(--accent);
    background: var(--accent);
    color: #fff;
    font-size: 0.75rem;
    font-weight: 600;
    line-height: 1;
    cursor: pointer;
    transition: background-color 0.12s ease, border-color 0.12s ease;
  }
  .download:hover {
    background: color-mix(in srgb, var(--accent) 88%, #fff);
    border-color: color-mix(in srgb, var(--accent) 88%, #fff);
  }
  .empty {
    color: var(--muted);
    font-size: 0.78rem;
  }
  /* Icon and input on one baseline: flex row keeps the icon vertically centered
     against the input regardless of the input's own height. */
  .search-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding-left: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2, var(--surface));
  }
  .search-wrap:focus-within {
    border-color: var(--accent);
  }
  .search-ic {
    display: inline-flex;
    flex: none;
    color: var(--muted);
    pointer-events: none;
  }
  .search {
    flex: 1;
    min-width: 0;
    width: 100%;
    border: none;
    background: transparent;
    padding-left: 0;
    font-size: 0.8rem;
  }
  .search:focus {
    outline: none;
  }
  .filters {
    display: flex;
    gap: var(--space-1);
  }
  .filters select {
    flex: 1;
    min-width: 0;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .results {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    overflow-y: auto;
  }
  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6) var(--space-3);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    color: var(--muted);
    font-size: 0.76rem;
    text-align: center;
  }
  .placeholder :global(.icon-svg) {
    opacity: 0.55;
  }
  .leaf {
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    cursor: pointer;
    transition: border-color 0.12s ease, background-color 0.12s ease;
  }
  .leaf .ticker {
    font-weight: 700;
    font-size: 0.85rem;
    color: var(--text);
  }
  .leaf .meta {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .leaf:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: color-mix(in srgb, var(--surface-2) 92%, var(--accent));
  }
  .leaf.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 9%, var(--surface-2));
  }
  .leaf.active .ticker {
    color: var(--accent);
  }
  .leaf:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
