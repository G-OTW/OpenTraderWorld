<script>
  // Management view: datasets grouped asset type → ticker → timeframe, with size/count/range
  // and per-dataset append (gap-fill), export (CSV) and delete.
  // A filter bar narrows by provider / ticker / timeframe / minimum size before grouping.
  import { histdataApi, fmtBytes, groupDatasets } from './api.js';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { infiniteScroll } from '$lib/ui/infiniteScroll.js';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';

  let { datasets = [], onchanged } = $props();

  // Dataset pending deletion (drives the confirm modal); null = closed.
  let pendingDelete = $state(null);

  // Filters persist across refresh (per-browser).
  const FILTERS_KEY = 'otw.histdata.datasetFilters.v1';
  function loadFilters() {
    try {
      return JSON.parse(localStorage.getItem(FILTERS_KEY) || '{}');
    } catch {
      return {};
    }
  }
  const saved = loadFilters();

  let fProvider = $state(saved.provider ?? '');
  let fTimeframe = $state(saved.timeframe ?? '');
  let fTicker = $state(saved.ticker ?? '');
  let fMinSize = $state(saved.minSize ?? ''); // in MB
  // Size sort within each ticker: '' (default) | 'desc' (largest first) | 'asc' (smallest first).
  let fSizeSort = $state(['', 'desc', 'asc'].includes(saved.sizeSort) ? saved.sizeSort : '');

  let loaded = $state(false);
  $effect(() => {
    loaded = true;
  });
  $effect(() => {
    const snap = { provider: fProvider, timeframe: fTimeframe, ticker: fTicker, minSize: fMinSize, sizeSort: fSizeSort };
    if (!loaded) return;
    try {
      localStorage.setItem(FILTERS_KEY, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  const uniq = (key) => [...new Set(datasets.map((d) => d[key]).filter(Boolean))].sort();
  const providers = $derived(uniq('provider'));
  const timeframes = $derived(uniq('timeframe'));

  const filtered = $derived(
    datasets.filter((d) => {
      const minBytes = fMinSize ? Number(fMinSize) * 1024 * 1024 : 0;
      return (
        (!fProvider || d.provider === fProvider) &&
        (!fTimeframe || d.timeframe === fTimeframe) &&
        (!fTicker || d.ticker.toLowerCase().includes(fTicker.toLowerCase())) &&
        (!minBytes || d.size_bytes >= minBytes)
      );
    })
  );

  // Default grouping (alphabetical asset type → ticker). When a size sort is active we
  // reorder both the sets within each ticker AND the ticker rows (by their largest set)
  // AND the asset-type sections (by their largest set) — otherwise the size order would
  // be invisible since most tickers hold a single dataset.
  const groups = $derived.by(() => {
    const base = groupDatasets(filtered);
    if (!fSizeSort) return base;
    const dir = fSizeSort === 'asc' ? 1 : -1;
    const maxSize = (sets) => sets.reduce((m, d) => Math.max(m, d.size_bytes ?? 0), 0);
    return base
      .map((sec) => ({
        asset_type: sec.asset_type,
        tickers: sec.tickers
          .map((tk) => ({
            ticker: tk.ticker,
            sets: [...tk.sets].sort((a, b) => (a.size_bytes - b.size_bytes) * dir)
          }))
          .sort((a, b) => (maxSize(a.sets) - maxSize(b.sets)) * dir)
      }))
      .sort(
        (a, b) =>
          (maxSize(a.tickers.flatMap((t) => t.sets)) -
            maxSize(b.tickers.flatMap((t) => t.sets))) *
          dir
      );
  });

  // Infinite scroll over dataset rows. `groups` is a nested asset→ticker→sets structure in
  // display order; we cap the number of rendered *sets* at `visibleCount` and trim the nested
  // groups to match, dropping now-empty tickers/sections. Reset to one page when the filtered
  // set changes so filtering never leaves us scrolled past the end of a shorter list.
  const PAGE = 40;
  let visibleCount = $state(PAGE);
  $effect(() => {
    void (fProvider + fTimeframe + fTicker + fMinSize + fSizeSort);
    visibleCount = PAGE;
  });
  const totalSets = $derived(filtered.length);
  const hasMore = $derived(visibleCount < totalSets);

  const pagedGroups = $derived.by(() => {
    let budget = visibleCount;
    const out = [];
    for (const sec of groups) {
      if (budget <= 0) break;
      const tickers = [];
      for (const tk of sec.tickers) {
        if (budget <= 0) break;
        const sets = tk.sets.slice(0, budget);
        budget -= sets.length;
        if (sets.length) tickers.push({ ticker: tk.ticker, sets });
      }
      if (tickers.length) out.push({ asset_type: sec.asset_type, tickers });
    }
    return out;
  });

  const anyFilter = $derived(!!(fProvider || fTimeframe || fTicker || fMinSize || fSizeSort));
  function clearFilters() {
    fProvider = fTimeframe = fTicker = fMinSize = fSizeSort = '';
  }
  function cycleSizeSort() {
    fSizeSort = fSizeSort === '' ? 'desc' : fSizeSort === 'desc' ? 'asc' : '';
  }
  const sizeSortLabel = $derived(
    fSizeSort === 'desc'
      ? $t('histdata.datasets.sizeDesc')
      : fSizeSort === 'asc'
        ? $t('histdata.datasets.sizeAsc')
        : $t('histdata.datasets.sizeNone')
  );

  function fmtRange(d) {
    if (!d.range_from) return $t('histdata.datasets.noRange');
    // Deliberately UTC, not dateKey(): these are OHLC bar timestamps. A bar's date is the
    // exchange's date, and must not shift with the viewer's timezone.
    const f = (s) => new Date(s).toISOString().slice(0, 10);
    return `${f(d.range_from)} → ${f(d.range_to)}`;
  }

  async function append(d) {
    await histdataApi.append(d.id);
    onchanged?.();
  }
  async function confirmDelete() {
    const d = pendingDelete;
    pendingDelete = null;
    if (!d) return;
    await histdataApi.remove(d.id);
    onchanged?.();
  }
</script>

<div class="filters">
  <select bind:value={fProvider} title={$t('histdata.datasets.filterProvider')}>
    <option value="">{$t('histdata.jobs.allProviders')}</option>
    {#each providers as p (p)}<option value={p}>{p}</option>{/each}
  </select>
  <select bind:value={fTimeframe} title={$t('histdata.datasets.filterTimeframe')}>
    <option value="">{$t('histdata.jobs.allTimeframes')}</option>
    {#each timeframes as tf (tf)}<option value={tf}>{tf}</option>{/each}
  </select>
  <input class="tk" placeholder={$t('histdata.jobs.tickerPlaceholder')} bind:value={fTicker} />
  <input class="sz" type="number" min="0" placeholder={$t('histdata.datasets.minSizePlaceholder')} bind:value={fMinSize} />
  <!-- Tri-state ('' → desc → asc). The direction lives in the label glyph, which a
       screen reader does not announce as sort state — aria-pressed carries the on/off. -->
  <button
    class="sizesort"
    class:on={!!fSizeSort}
    aria-pressed={!!fSizeSort}
    onclick={cycleSizeSort}
    title={$t('histdata.datasets.sortBySize')}
  >
    {sizeSortLabel}
  </button>
  {#if anyFilter}
    <button class="clear" onclick={clearFilters}>{$t('common.clear')}</button>
    <span class="count">{filtered.length}/{datasets.length}</span>
  {/if}
</div>

<div class="mgr">
  {#if !datasets.length}
    <p class="empty">{$t('histdata.datasets.emptyNone')}</p>
  {:else if !filtered.length}
    <p class="empty">{$t('histdata.datasets.emptyFiltered')}</p>
  {/if}
  {#each pagedGroups as g (g.asset_type)}
    <section class="type">
      <h3>{g.asset_type}</h3>
      {#each g.tickers as tk (tk.ticker)}
        <div class="ticker">
          <div class="tname">{tk.ticker}</div>
          <div class="sets">
            {#each tk.sets as d (d.id)}
              <div class="set" class:err={d.status === 'error'}>
                <span class="tf">{d.timeframe}</span>
                <span class="prov">{d.provider}</span>
                <span class="range">{fmtRange(d)}</span>
                <span class="num">{$t('histdata.datasets.barsCount', { count: fmtNum(d.bar_count, 0) })}</span>
                <span class="num">{fmtBytes(d.size_bytes)}</span>
                <span class="st" data-st={d.status}>{d.status}</span>
                <span class="acts">
                  <button onclick={() => append(d)} title={$t('histdata.datasets.fetchNewerTitle')}>{$t('histdata.datasets.fetchNewer')}</button>
                  <a class="btn" href={histdataApi.exportUrl(d.id)} download>{$t('histdata.datasets.export')}</a>
                  <button class="danger" onclick={() => (pendingDelete = d)}>{$t('histdata.datasets.delete')}</button>
                </span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </section>
  {/each}
  {#if hasMore}
    <div
      class="sentinel"
      use:infiniteScroll={{ onLoadMore: () => (visibleCount += PAGE), disabled: !hasMore }}
    >
      {$t('common.loadingMore')}
    </div>
  {/if}
</div>

<ConfirmModal
  open={!!pendingDelete}
  title={$t('histdata.datasets.deleteTitle')}
  message={pendingDelete
    ? $t('histdata.datasets.deleteConfirmBody', { provider: pendingDelete.provider, ticker: pendingDelete.ticker, timeframe: pendingDelete.timeframe })
    : ''}
  confirmLabel={$t('histdata.datasets.delete')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .filters input.tk {
    text-transform: none;
    width: 120px;
  }
  .filters input.sz {
    text-transform: none;
    width: 80px;
  }
  .clear,
  .sizesort {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .sizesort.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .count {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .mgr {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .empty {
    color: var(--muted);
  }
  .sentinel {
    text-align: center;
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2);
  }
  .type h3 {
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
    color: var(--muted);
    margin-bottom: var(--space-2);
  }
  .ticker {
    margin-bottom: var(--space-2);
  }
  .tname {
    font-weight: var(--fw-medium);
    margin-bottom: var(--space-1);
  }
  .sets {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .set {
    display: grid;
    grid-template-columns: 48px 90px 1fr auto auto 70px auto;
    align-items: center;
    gap: var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-base);
  }
  .set.err {
    border-color: var(--red);
  }
  .tf {
    font-weight: var(--fw-medium);
  }
  .prov,
  .range,
  .num {
    color: var(--muted);
  }
  .st {
    text-transform: capitalize;
    font-size: var(--text-xs);
  }
  .st[data-st='complete'] {
    color: var(--green);
  }
  .st[data-st='partial'] {
    color: var(--amber);
  }
  .st[data-st='error'] {
    color: var(--red);
  }
  .acts {
    display: flex;
    gap: var(--space-2);
  }
  button,
  .btn {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
    text-decoration: none;
  }
  .danger {
    color: var(--red);
    border-color: var(--red);
  }
</style>
