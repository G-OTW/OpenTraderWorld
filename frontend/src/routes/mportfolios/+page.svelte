<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  // Managers' Portfolios — superinvestor 13F summaries cached on a schedule from Dataroma.
  // Two tabs: "Portfolios" (live cache — name search + ticker filter, click a row for holdings,
  // 📷 to save a snapshot) and "Snapshots" (the user's saved point-in-time copies, grouped by
  // portfolio). Attribution to Dataroma is shown in the footer per the source's request. Distinct
  // from the future user "portfolios" module.
  import { mportfoliosApi, fmtValue } from '$lib/modules/mportfolios/api.js';
  import MportfolioDetail from '$lib/modules/mportfolios/MportfolioDetail.svelte';
  import SnapshotDetail from '$lib/modules/mportfolios/SnapshotDetail.svelte';
  import { t } from '$lib/i18n';

  // Active tab persists per-browser in localStorage (matches histviz/histdata session state).
  const TAB_KEY = 'otw.mportfolios.tab.v1';
  const savedTab = (() => {
    try {
      const t = localStorage.getItem(TAB_KEY);
      return t === 'snapshots' ? t : 'portfolios';
    } catch {
      return 'portfolios';
    }
  })();
  let tab = $state(savedTab); // 'portfolios' | 'snapshots'

  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      /* non-fatal */
    }
  });

  // ---- Portfolios tab -----------------------------------------------------
  let portfolios = $state([]);
  let updatedAt = $state(null);
  let error = $state('');
  let loading = $state(false);
  let refreshing = $state(false);

  let q = $state('');
  let ticker = $state('');
  let selected = $state(null); // slug of the open detail modal
  let snappingSlug = $state(null); // slug currently being snapshotted (row spinner)

  // Alphabetical sort by name, per tab: '' (off) | 'asc' (A→Z) | 'desc' (Z→A).
  // On the Snapshots tab, an active alpha sort overrides the manual family order.
  let pAlpha = $state('');
  let snapAlpha = $state('');
  const cycleAlpha = (v) => (v === '' ? 'asc' : v === 'asc' ? 'desc' : '');
  const alphaLabel = (v) =>
    v === 'asc' ? $t('mportfolios.alpha.az') : v === 'desc' ? $t('mportfolios.alpha.za') : $t('mportfolios.alpha.off');

  const shownPortfolios = $derived.by(() => {
    if (!pAlpha) return portfolios;
    const dir = pAlpha === 'asc' ? 1 : -1;
    return [...portfolios].sort((a, b) => a.name.localeCompare(b.name) * dir);
  });

  let debounce;
  function load() {
    loading = true;
    error = '';
    mportfoliosApi
      .list({ q, ticker })
      .then((r) => {
        portfolios = r.portfolios;
        updatedAt = r.updated_at;
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  }

  $effect(() => {
    q;
    ticker;
    clearTimeout(debounce);
    debounce = setTimeout(load, 200);
    return () => clearTimeout(debounce);
  });

  async function refresh() {
    refreshing = true;
    error = '';
    try {
      await mportfoliosApi.refresh();
      load();
    } catch (e) {
      error = e.message;
    } finally {
      refreshing = false;
    }
  }

  /** Save a snapshot of the given live portfolio, then refresh the snapshots list. */
  async function snapshot(slug) {
    snappingSlug = slug;
    error = '';
    try {
      await mportfoliosApi.snapshot(slug);
      loadSnapshots();
    } catch (e) {
      error = e.message;
    } finally {
      snappingSlug = null;
    }
  }

  let updatedLabel = $derived(updatedAt ? new Date(updatedAt).toLocaleString() : null);

  // ---- Snapshots tab ------------------------------------------------------
  let snapshots = $state([]);
  let snapQ = $state('');
  let snapError = $state('');
  let snapLoading = $state(false);
  let openSnapId = $state(null); // snapshot id of the open detail modal
  let expanded = $state(new Set()); // source_slug values whose group is open

  let snapDebounce;
  function loadSnapshots() {
    snapLoading = true;
    snapError = '';
    mportfoliosApi
      .listSnapshots({ q: snapQ })
      .then((r) => (snapshots = r.snapshots))
      .catch((e) => (snapError = e.message))
      .finally(() => (snapLoading = false));
  }

  $effect(() => {
    snapQ;
    clearTimeout(snapDebounce);
    snapDebounce = setTimeout(loadSnapshots, 200);
    return () => clearTimeout(snapDebounce);
  });

  // Group snapshots by source portfolio (newest-first within a group by default).
  let groups = $derived.by(() => {
    const m = new Map();
    for (const s of snapshots) {
      if (!m.has(s.source_slug)) m.set(s.source_slug, { slug: s.source_slug, name: s.name, items: [] });
      m.get(s.source_slug).items.push(s);
    }
    const list = [...m.values()];
    if (snapAlpha) {
      const dir = snapAlpha === 'asc' ? 1 : -1;
      list.sort((a, b) => a.name.localeCompare(b.name) * dir);
    }
    return list;
  });

  function toggle(slug) {
    const next = new Set(expanded);
    next.has(slug) ? next.delete(slug) : next.add(slug);
    expanded = next;
  }

  async function deleteSnapshot(id) {
    snapError = '';
    try {
      await mportfoliosApi.deleteSnapshot(id);
      loadSnapshots();
    } catch (e) {
      snapError = e.message;
    }
  }

  async function deleteGroup(slug, name) {
    if (!confirm($t('mportfolios.snapshots.confirmDeleteGroup', { name }))) return;
    snapError = '';
    try {
      await mportfoliosApi.deleteSnapshotsBySlug(slug);
      loadSnapshots();
    } catch (e) {
      snapError = e.message;
    }
  }

  const fmtTs = (t) => new Date(t).toLocaleString();
</script>

<div class="page">
  <header>
    <h1>{$t('mportfolios.title')}</h1>
    {#if tab === 'portfolios'}
      <button class="reload" onclick={refresh} disabled={refreshing}>
        {#if refreshing}{$t('mportfolios.refreshing')}{:else}<Icon name="refresh-cw" size={13} /> {$t('common.refresh')}{/if}
      </button>
    {/if}
  </header>

  <div class="tabs">
    <button class="tab" class:active={tab === 'portfolios'} onclick={() => (tab = 'portfolios')}>{$t('mportfolios.tabs.portfolios')}</button>
    <button class="tab" class:active={tab === 'snapshots'} onclick={() => (tab = 'snapshots')}>{$t('mportfolios.tabs.snapshots')}</button>
  </div>

  {#if tab === 'portfolios'}
    <div class="filters">
      <input class="search" type="search" placeholder={$t('mportfolios.list.searchPlaceholder')} bind:value={q} />
      <input class="ticker" type="search" placeholder={$t('mportfolios.list.tickerPlaceholder')} bind:value={ticker} />
      <button class="alpha" class:on={!!pAlpha} onclick={() => (pAlpha = cycleAlpha(pAlpha))} title={$t('mportfolios.list.sortByName')}>
        {alphaLabel(pAlpha)}
      </button>
    </div>

    {#if error}<p class="err" title={$t('mportfolios.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

    <div class="table-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th class="l">{$t('mportfolios.list.colManager')}</th>
            <th>{$t('mportfolios.list.colPeriod')}</th>
            <th>{$t('mportfolios.list.colValue')}</th>
            <th>{$t('mportfolios.list.colHoldings')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each shownPortfolios as p (p.slug)}
            <tr onclick={() => (selected = p.slug)}>
              <td class="l name">{p.name}</td>
              <td>{p.period || '—'}</td>
              <td>{fmtValue(p.value_num, p.value_text)}</td>
              <td>{p.stock_count}</td>
              <td class="snap-cell">
                <button
                  class="icon"
                  title={$t('mportfolios.list.saveSnapshot')}
                  disabled={snappingSlug === p.slug}
                  onclick={(e) => {
                    e.stopPropagation();
                    snapshot(p.slug);
                  }}
                >
                  {#if snappingSlug === p.slug}…{:else}<Icon name="camera" size={13} />{/if}
                </button>
              </td>
            </tr>
          {/each}
          {#if !loading && portfolios.length === 0}
            <tr><td colspan="5" class="empty">{ticker ? $t('mportfolios.list.emptyFiltered', { ticker: ticker.toUpperCase() }) : $t('mportfolios.list.empty')}</td></tr>
          {/if}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="filters">
      <input class="search" type="search" placeholder={$t('mportfolios.snapshots.searchPlaceholder')} bind:value={snapQ} />
      <button class="alpha" class:on={!!snapAlpha} onclick={() => (snapAlpha = cycleAlpha(snapAlpha))} title={$t('mportfolios.snapshots.sortFamiliesByName')}>
        {alphaLabel(snapAlpha)}
      </button>
    </div>

    {#if snapError}<p class="err" title={$t('mportfolios.clickToCopy')} use:copyLog={snapError}>{snapError}</p>{/if}

    <div class="groups">
      {#each groups as g (g.slug)}
        <div class="group">
          <div class="group-head">
            <button class="group-toggle" onclick={() => toggle(g.slug)}>
              <span class="caret" class:open={expanded.has(g.slug)}><Icon name="chevron-right" size={13} /></span>
              <span class="group-name">{g.name}</span>
              <span class="count">{g.items.length}</span>
            </button>
            <button class="del" title={$t('mportfolios.snapshots.deleteGroupTitle')} onclick={() => deleteGroup(g.slug, g.name)}><Icon name="trash" size={14} /></button>
          </div>
          {#if expanded.has(g.slug)}
            <ul class="snap-list">
              {#each g.items as s (s.id)}
                <li>
                  <span class="ts"><Icon name="camera" size={11} /> {fmtTs(s.taken_at)}</span>
                  <span class="val">{fmtValue(s.value_num, s.value_text)}</span>
                  {#if s.period}<span class="val">{s.period}</span>{/if}
                  <span class="hold">{$t('mportfolios.snapshots.holdingsCount', { count: s.stock_count })}</span>
                  <button class="details" onclick={() => (openSnapId = s.id)}>{$t('mportfolios.snapshots.details')}</button>
                  <button class="del" title={$t('mportfolios.snapshots.deleteTitle')} onclick={() => deleteSnapshot(s.id)}><Icon name="trash" size={14} /></button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/each}
      {#if !snapLoading && groups.length === 0}
        <p class="empty">{$t('mportfolios.snapshots.empty')}</p>
      {/if}
    </div>
  {/if}

  <footer class="attribution">
    {#if updatedLabel}<span class="muted">{$t('mportfolios.updatedAt', { time: updatedLabel })}</span>{/if}
    <span class="muted">{$t('mportfolios.dataProvidedBy')}</span>
    <a href="https://www.dataroma.com" target="_blank" rel="noopener">Dataroma</a>.
  </footer>
</div>

<MportfolioDetail bind:slug={selected} onsnapshot={snapshot} />
<SnapshotDetail bind:id={openSnapId} />

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  h1 {
    font-size: 1.4rem;
    font-weight: 700;
  }
  .reload {
    margin-left: auto;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
  }
  .reload:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
  }
  .tab {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }
  .tab.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .filters {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .search {
    flex: 1 1 320px;
  }
  .ticker {
    flex: 0 1 240px;
  }
  .alpha {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: 0.85rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .alpha.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .table-wrap {
    flex: 1;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  th,
  td {
    padding: var(--space-2) var(--space-3);
    text-align: right;
    border-bottom: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    position: sticky;
    top: 0;
    background: var(--surface);
  }
  .l {
    text-align: left;
  }
  tbody tr {
    cursor: pointer;
  }
  .name {
    font-weight: 600;
    color: var(--text);
  }
  .snap-cell {
    width: 1%;
    white-space: nowrap;
  }
  .empty {
    text-align: center;
    color: var(--muted);
    padding: var(--space-6);
  }
  /* Snapshots tab */
  .groups {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow-y: auto;
  }
  .group {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .group-head {
    display: flex;
    align-items: center;
    background: var(--surface-2);
  }
  .group-toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.9rem;
    cursor: pointer;
    text-align: left;
  }
  .caret {
    display: inline-block;
    transition: transform 0.1s;
    color: var(--muted);
  }
  .caret.open {
    transform: rotate(90deg);
  }
  .group-name {
    font-weight: 600;
  }
  .count {
    color: var(--muted);
    font-size: 0.8rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 var(--space-2);
  }
  .snap-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .snap-list li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--border);
    font-size: 0.85rem;
  }
  .ts {
    flex: 1;
    color: var(--text);
  }
  .val,
  .hold {
    color: var(--muted);
  }
  .details {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    color: var(--text);
    font-size: 0.8rem;
    cursor: pointer;
  }
  .del {
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 0.85rem;
    padding: 2px 6px;
    border-radius: var(--radius);
  }
  .del:hover {
    background: var(--surface);
  }
  .attribution {
    font-size: 0.8rem;
  }
  .attribution a {
    color: var(--accent);
    text-decoration: none;
  }
  .muted {
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
</style>
