<script>
  // Management view: datasets grouped asset type → ticker → timeframe, with size/count/range
  // and per-dataset append (gap-fill), export (CSV) and delete.
  // A filter bar narrows by provider / ticker / timeframe / minimum size before grouping.
  import { histdataApi, fmtBytes, groupDatasets, IMPORT_PROVIDER } from './api.js';
  import Button from '$lib/ui/Button.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ImportModal from './ImportModal.svelte';
  import { infiniteScroll } from '$lib/ui/infiniteScroll.js';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';

  let { datasets = [], onchanged } = $props();

  // Dataset pending deletion (drives the confirm modal); null = closed.
  let pendingDelete = $state(null);
  let importOpen = $state(false);

  /** An imported series has no provider behind it: nothing to append, and `source` is
   *  what answers "where did this come from". */
  const isImported = (d) => d.provider === IMPORT_PROVIDER;
  const originOf = (d) => (isImported(d) ? d.source || IMPORT_PROVIDER : d.provider);
  const tagsOf = (d) => (Array.isArray(d.tags) ? d.tags : []);

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
  let fTag = $state(saved.tag ?? '');
  // Size sort within each ticker: '' (default) | 'desc' (largest first) | 'asc' (smallest first).
  let fSizeSort = $state(['', 'desc', 'asc'].includes(saved.sizeSort) ? saved.sizeSort : '');

  let loaded = $state(false);
  $effect(() => {
    loaded = true;
  });
  $effect(() => {
    const snap = { provider: fProvider, timeframe: fTimeframe, ticker: fTicker, minSize: fMinSize, sizeSort: fSizeSort, tag: fTag };
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
  const tags = $derived([...new Set(datasets.flatMap(tagsOf))].sort());

  // Filter dropdowns: the "" entry is the no-filter row, so it is an option like any other.
  const opts = (all, values) => [{ value: '', label: all }, ...values.map((v) => ({ value: v, label: v }))];
  const providerOpts = $derived(opts($t('histdata.jobs.allProviders'), providers));
  const timeframeOpts = $derived(opts($t('histdata.jobs.allTimeframes'), timeframes));
  const tagOpts = $derived(opts($t('histdata.datasets.allTags'), tags));

  const filtered = $derived(
    datasets.filter((d) => {
      const minBytes = fMinSize ? Number(fMinSize) * 1024 * 1024 : 0;
      return (
        (!fProvider || d.provider === fProvider) &&
        (!fTimeframe || d.timeframe === fTimeframe) &&
        (!fTicker ||
          d.ticker.toLowerCase().includes(fTicker.toLowerCase()) ||
          (d.label ?? '').toLowerCase().includes(fTicker.toLowerCase())) &&
        (!fTag || tagsOf(d).includes(fTag)) &&
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
    void (fProvider + fTimeframe + fTicker + fMinSize + fSizeSort + fTag);
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

  const anyFilter = $derived(!!(fProvider || fTimeframe || fTicker || fMinSize || fSizeSort || fTag));
  function clearFilters() {
    fProvider = fTimeframe = fTicker = fMinSize = fSizeSort = fTag = '';
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

  // Exports run through fetch so the row can say it is working: the file is built
  // server-side in one go, and a link that saves nothing for thirty seconds reads as a
  // dead button. Keyed by dataset+format, so two rows can export at once.
  let exporting = $state([]);
  let exportError = $state('');
  const isExporting = (d, fmt) => exporting.includes(`${d.id}:${fmt}`);

  async function exportSet(d, fmt) {
    const key = `${d.id}:${fmt}`;
    if (exporting.includes(key)) return;
    exporting = [...exporting, key];
    exportError = '';
    try {
      await histdataApi.exportDataset(d.id, fmt);
    } catch (e) {
      exportError = e.message;
    } finally {
      exporting = exporting.filter((k) => k !== key);
    }
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
  <Dropdown
    bind:value={fProvider}
    options={providerOpts}
    title={$t('histdata.datasets.filterProvider')}
    ariaLabel={$t('histdata.datasets.filterProvider')}
  />
  <Dropdown
    bind:value={fTimeframe}
    options={timeframeOpts}
    title={$t('histdata.datasets.filterTimeframe')}
    ariaLabel={$t('histdata.datasets.filterTimeframe')}
  />
  <input class="tk" placeholder={$t('histdata.jobs.tickerPlaceholder')} bind:value={fTicker} />
  <input class="sz" type="number" min="0" placeholder={$t('histdata.datasets.minSizePlaceholder')} bind:value={fMinSize} />
  {#if tags.length}
    <Dropdown
      bind:value={fTag}
      options={tagOpts}
      title={$t('histdata.datasets.filterTag')}
      ariaLabel={$t('histdata.datasets.filterTag')}
    />
  {/if}
  <!-- Tri-state ('' → desc → asc). The direction lives in the label glyph, which a
       screen reader does not announce as sort state — aria-pressed carries the on/off. -->
  <button
    class="btn sizesort"
    class:on={!!fSizeSort}
    aria-pressed={!!fSizeSort}
    onclick={cycleSizeSort}
    title={$t('histdata.datasets.sortBySize')}
  >
    {sizeSortLabel}
  </button>
  {#if anyFilter}
    <Button onclick={clearFilters}>{$t('common.clear')}</Button>
    <span class="count">{filtered.length}/{datasets.length}</span>
  {/if}
  <!-- Import sits with the dataset actions, opposite the CSV export it mirrors. -->
  <Button icon="upload" onclick={() => (importOpen = true)}>{$t('histdata.import.open')}</Button>
</div>

<!-- The label stays put while it runs (no width jump); the spinner and the busy title
     carry the state, and aria-busy announces it. -->
{#snippet exportBtn(d, fmt, label, title)}
  {@const busy = isExporting(d, fmt)}
  <button
    class="exp"
    class:busy
    disabled={busy}
    aria-busy={busy}
    title={busy ? $t('histdata.datasets.exporting') : title}
    onclick={() => exportSet(d, fmt)}
  >
    {#if busy}<span class="spin" aria-hidden="true"></span>{/if}
    {label}
  </button>
{/snippet}

<div class="mgr">
  {#if !datasets.length}
    <p class="empty">{$t('histdata.datasets.emptyNone')}</p>
  {:else if !filtered.length}
    <p class="empty">{$t('histdata.datasets.emptyFiltered')}</p>
  {/if}
  {#if exportError}
    <p class="export-err">{exportError}</p>
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
                <span class="prov" class:imported={isImported(d)}>{originOf(d)}</span>
                <span class="range">{fmtRange(d)}</span>
                <span class="num">{$t('histdata.datasets.barsCount', { count: fmtNum(d.bar_count, 0) })}</span>
                <span class="num">{fmtBytes(d.size_bytes)}</span>
                <span class="st" data-st={d.status}>{d.status}</span>
                <span class="acts">
                  <!-- Appending asks a provider for newer bars; an imported series has
                       none, and the way to extend it is another file. -->
                  {#if !isImported(d)}
                    <button onclick={() => append(d)} title={$t('histdata.datasets.fetchNewerTitle')}>{$t('histdata.datasets.fetchNewer')}</button>
                  {/if}
                  {@render exportBtn(d, 'csv', $t('histdata.datasets.exportCsv'), $t('histdata.datasets.exportCsvTitle'))}
                  {@render exportBtn(d, 'parquet', $t('histdata.datasets.exportParquet'), $t('histdata.datasets.exportParquetTitle'))}
                  <button class="danger" onclick={() => (pendingDelete = d)}>{$t('histdata.datasets.delete')}</button>
                </span>
                {#if d.label || tagsOf(d).length}
                  <span class="meta">
                    {#if d.label}<span class="label">{d.label}</span>{/if}
                    {#each tagsOf(d) as tag (tag)}<span class="tag">{tag}</span>{/each}
                  </span>
                {/if}
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

<ImportModal bind:open={importOpen} {datasets} onimported={() => onchanged?.()} />

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
  /* Fixed width so the row doesn't jump between "All providers" and "yahoo". */
  .filters > :global(.dd) {
    width: 150px;
  }
  /* Rides the global .btn layer so it matches Clear and the controls it sits with;
     only the sort-is-on state is local. */
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
  /* A file, not a feed: named after where it came from, marked as not live. */
  .prov.imported {
    font-style: italic;
  }
  .meta {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }
  .meta .label {
    color: var(--text);
  }
  .meta .tag {
    color: var(--muted);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 0 6px;
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
  .exp {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  /* Busy, not unavailable: keep it lit and swap the cursor. */
  .exp.busy {
    opacity: 1;
    cursor: progress;
  }
  .spin {
    width: 11px;
    height: 11px;
    flex: none;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    opacity: 0.7;
    animation: spin 600ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .export-err {
    color: var(--red);
    font-size: var(--text-sm);
  }
</style>
