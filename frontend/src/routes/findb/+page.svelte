<script>
  import Icon from '$lib/ui/Icon.svelte';
  // FinanceDatabase module. Two tabs:
  //  - Search: live full-text/fuzzy search over ~300k instruments, filterable by asset type;
  //    each result can be saved to favorites (optionally into a folder).
  //  - Favorites: saved instruments grouped by folder, with folder management.
  // The catalog is bulk-loaded on first install; until then we show an install gate that
  // polls /findb/status while the import runs in the background.
  import { onMount, onDestroy } from 'svelte';
  import {
    findbApi,
    ASSET_TYPES,
    SORTS,
    filtersFor,
    typeLabel,
    debounce
  } from '$lib/modules/findb/api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ComboSelect from '$lib/ui/ComboSelect.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';

  // Facets rendered as a searchable combobox (long value lists); others stay plain selects.
  const COMBO_FILTERS = new Set(['exchange', 'currency']);

  const PAGE = 40;

  // UI state we keep across reloads (which tab, which favorites folder).
  const UI_KEY = 'findb.ui';
  const savedUi = (() => {
    try {
      return JSON.parse(localStorage.getItem(UI_KEY)) ?? {};
    } catch {
      return {};
    }
  })();

  let meta = $state({ installed: false, importing: false, count: 0, version: '' });
  let loading = $state(true);
  let tab = $state(savedUi.tab === 'favorites' ? 'favorites' : 'search'); // 'search' | 'favorites'

  // search state
  let q = $state('');
  let type = $state('');
  let sort = $state('relevance');
  let filters = $state({}); // { exchange, currency, country, sector, industry, category, family }
  let facetCache = $state({}); // key -> distinct values (scoped to current type)
  let results = $state([]);
  let searching = $state(false); // initial load of a new query
  let loadingMore = $state(false);
  let hasMore = $state(false);
  let offset = $state(0);
  let sentinel = $state(null); // IntersectionObserver target at the list bottom
  let observer;

  // Filters relevant for the current asset type (universal + contextual).
  const activeFilters = $derived(filtersFor(type));

  // favorites + folders
  let favorites = $state([]);
  let folders = $state([]);
  // Selected folder in the favorites left pane: a folder id, '' = All, or 'unfiled'.
  let selectedFolder = $state(savedUi.folder ?? '');
  // Favorites are rendered incrementally (the list can be large). Show this many, grow on scroll.
  const FAV_PAGE = 50;
  let favLimit = $state(FAV_PAGE);
  let favSentinel = $state(null);
  let favObserver;

  // save-to-folder modal
  let saving = $state(null); // the instrument being saved
  let saveFolder = $state(''); // selected folder id, '' = Unfiled, '__new__' = create one
  let saveNote = $state('');
  let saveNewFolder = $state(''); // name for an inline new folder (when saveFolder === '__new__')

  // folder editor
  let folderModal = $state(false);
  let editingFolder = $state(null);
  let folderName = $state('');

  let pollTimer;

  onMount(async () => {
    await refreshStatus();
    if (meta.installed) await loadFavorites();
    loading = false;
  });
  onDestroy(() => {
    clearInterval(pollTimer);
    observer?.disconnect();
    favObserver?.disconnect();
  });

  async function refreshStatus() {
    meta = await findbApi.status();
    if (meta.importing && !pollTimer) {
      pollTimer = setInterval(async () => {
        meta = await findbApi.status();
        if (!meta.importing) {
          clearInterval(pollTimer);
          pollTimer = null;
          if (meta.installed) await loadFavorites();
        }
      }, 1500);
    }
  }

  async function install() {
    await findbApi.install();
    await refreshStatus();
  }

  async function loadFavorites() {
    [favorites, folders] = await Promise.all([findbApi.listFavorites(), findbApi.listFolders()]);
  }

  // Current query params (everything except paging), for both first page and load-more.
  function baseParams() {
    return { q: q.trim(), type, sort, ...activeOnly(filters) };
  }
  // Drop empty filter values so we only send set ones.
  function activeOnly(obj) {
    const out = {};
    for (const [k, v] of Object.entries(obj)) if (v) out[k] = v;
    return out;
  }
  // True when there is anything to search/browse (a term or any filter set).
  const hasCriteria = $derived(!!q.trim() || type || Object.values(filters).some(Boolean));

  // Run a fresh search (resets paging). Debounced for the text box.
  async function firstPage() {
    if (!hasCriteria) {
      results = [];
      hasMore = false;
      return;
    }
    searching = true;
    offset = 0;
    try {
      const r = await findbApi.search({ ...baseParams(), limit: PAGE, offset: 0 });
      results = r.results;
      hasMore = r.has_more;
      offset = r.results.length;
    } finally {
      searching = false;
    }
  }
  const runSearch = debounce(firstPage, 250);

  async function loadMore() {
    if (loadingMore || !hasMore) return;
    loadingMore = true;
    try {
      const r = await findbApi.search({ ...baseParams(), limit: PAGE, offset });
      results = [...results, ...r.results];
      hasMore = r.has_more;
      offset += r.results.length;
    } finally {
      loadingMore = false;
    }
  }

  function onInput() {
    runSearch();
  }

  // Re-search immediately when type / sort / filters change.
  $effect(() => {
    // touch dependencies
    type;
    sort;
    JSON.stringify(filters);
    firstPage();
  });

  // When the asset type changes, drop contextual filters that no longer apply and
  // refresh the dropdown options for the ones that remain.
  $effect(() => {
    const allowed = new Set(activeFilters.map((f) => f.key));
    const next = {};
    for (const [k, v] of Object.entries(filters)) if (allowed.has(k)) next[k] = v;
    if (Object.keys(next).length !== Object.keys(filters).length) filters = next;
    loadFacets();
  });

  // Lazy-load distinct values for each active filter (scoped to the current type).
  async function loadFacets() {
    for (const f of activeFilters) {
      const cacheKey = `${f.key}:${type}`;
      if (facetCache[cacheKey]) continue;
      try {
        const values = await findbApi.facet(f.key, type);
        facetCache = { ...facetCache, [cacheKey]: values };
      } catch {
        /* ignore */
      }
    }
  }
  const facetValues = (key) => facetCache[`${key}:${type}`] ?? [];

  // Infinite scroll: observe a sentinel near the list bottom.
  $effect(() => {
    if (!sentinel) return;
    observer?.disconnect();
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) loadMore();
      },
      { rootMargin: '300px' }
    );
    observer.observe(sentinel);
    return () => observer?.disconnect();
  });

  // Drop a restored folder id that no longer exists (deleted in another session).
  $effect(() => {
    if (
      selectedFolder &&
      selectedFolder !== 'unfiled' &&
      folders.length &&
      !folders.some((f) => f.id === selectedFolder)
    )
      selectedFolder = '';
  });

  // Persist tab + selected folder so a refresh returns to the same view.
  $effect(() => {
    try {
      localStorage.setItem(UI_KEY, JSON.stringify({ tab, folder: selectedFolder }));
    } catch {
      /* ignore */
    }
  });

  // Set of favorited instrument ids, for the star toggle on results.
  const favIds = $derived(new Set(favorites.map((f) => f.instrument_id)));

  function openSave(inst) {
    saving = inst;
    const existing = favorites.find((f) => f.instrument_id === inst.id);
    saveFolder = existing?.folder_id ?? '';
    saveNote = existing?.note ?? '';
    saveNewFolder = '';
  }

  async function confirmSave() {
    // When "New folder…" is picked, create it first and file the favorite into it.
    let folderId = saveFolder || null;
    if (saveFolder === '__new__') {
      const name = saveNewFolder.trim();
      if (!name) return;
      const folder = await findbApi.addFolder({ name, color: '' });
      folderId = folder.id;
    }
    await findbApi.addFavorite({
      instrument_id: saving.id,
      folder_id: folderId,
      note: saveNote
    });
    saving = null;
    await loadFavorites();
  }

  async function unfav(instrumentId) {
    const f = favorites.find((x) => x.instrument_id === instrumentId);
    if (f) {
      await findbApi.removeFavorite(f.id);
      await loadFavorites();
    }
  }

  // --- folders ---
  function openFolder(f = null) {
    editingFolder = f;
    folderName = f?.name ?? '';
    folderModal = true;
  }
  async function saveFolderForm() {
    const payload = { name: folderName.trim(), color: '' };
    if (!payload.name) return;
    if (editingFolder) await findbApi.updateFolder(editingFolder.id, payload);
    else await findbApi.addFolder(payload);
    folderModal = false;
    await loadFavorites();
  }
  let deletingFolder = $state(null); // folder pending delete-confirm
  function delFolder(f) {
    deletingFolder = f;
  }
  async function confirmDelFolder() {
    const f = deletingFolder;
    deletingFolder = null;
    if (!f) return;
    await findbApi.removeFolder(f.id);
    if (selectedFolder === f.id) selectedFolder = '';
    await loadFavorites();
  }

  // Count of favorites per folder id, plus the unfiled bucket, for the left-pane badges.
  const counts = $derived.by(() => {
    const valid = new Set(folders.map((f) => f.id));
    const c = { unfiled: 0 };
    for (const f of folders) c[f.id] = 0;
    for (const fav of favorites) {
      if (fav.folder_id && valid.has(fav.folder_id)) c[fav.folder_id]++;
      else c.unfiled++;
    }
    return c;
  });

  // The folder object currently selected (null for All / Unfiled pseudo-folders).
  const activeFolder = $derived(folders.find((f) => f.id === selectedFolder) ?? null);

  // Favorites shown in the right pane for the current left-pane selection.
  const paneItems = $derived.by(() => {
    const valid = new Set(folders.map((f) => f.id));
    if (selectedFolder === '') return favorites;
    if (selectedFolder === 'unfiled')
      return favorites.filter((f) => !f.folder_id || !valid.has(f.folder_id));
    return favorites.filter((f) => f.folder_id === selectedFolder);
  });

  // Reset the incremental window whenever the visible set changes (folder switch, add/remove).
  $effect(() => {
    paneItems.length;
    selectedFolder;
    favLimit = FAV_PAGE;
  });
  const favShown = $derived(paneItems.slice(0, favLimit));
  const favHasMore = $derived(favLimit < paneItems.length);

  // Infinite scroll for favorites: grow the window when the bottom sentinel comes into view.
  $effect(() => {
    if (!favSentinel) return;
    favObserver?.disconnect();
    favObserver = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) favLimit += FAV_PAGE;
      },
      { rootMargin: '300px' }
    );
    favObserver.observe(favSentinel);
    return () => favObserver?.disconnect();
  });
</script>

<div class="findb">
  {#if loading}
    <div class="sk-page" aria-busy="true">
      <Skeleton rows={6} height="2.4rem" gap="var(--space-3)" />
    </div>
  {:else if !meta.installed}
    <!-- Install gate -->
    <div class="gate">
      <h1>FinanceDatabase</h1>
      <p class="muted">
        {$t('findb.gate.description')}
      </p>
      {#if meta.importing}
        <p class="importing">{$t('findb.gate.importing')}</p>
        <div class="spinner"></div>
      {:else}
        <p class="muted">
          {$t('findb.gate.installHint')}
        </p>
        <button class="primary" onclick={install}>{$t('findb.gate.installButton')}</button>
      {/if}
    </div>
  {:else}
    <header class="bar">
      <div class="tabs">
        <button class:active={tab === 'search'} onclick={() => (tab = 'search')}>{$t('findb.tabs.search')}</button>
        <button class:active={tab === 'favorites'} onclick={() => (tab = 'favorites')}>
          {$t('findb.tabs.favorites')} {favorites.length ? `(${favorites.length})` : ''}
        </button>
      </div>
      <span class="count muted">{$t('findb.header.instrumentCount', { count: fmtNum(meta.count, 0) })}</span>
    </header>

    {#if tab === 'search'}
      <div class="search-controls">
        <input
          class="search"
          placeholder={$t('findb.search.placeholder')}
          bind:value={q}
          oninput={onInput}
        />
        <select bind:value={type} aria-label={$t('findb.search.assetType')}>
          {#each ASSET_TYPES as t}
            <option value={t.value}>{t.label}</option>
          {/each}
        </select>
        <select bind:value={sort} aria-label={$t('findb.search.sortBy')}>
          {#each SORTS as s}
            <option value={s.value}>{s.label}</option>
          {/each}
        </select>
      </div>

      <div class="filters">
        {#each activeFilters as f (f.key)}
          {#if COMBO_FILTERS.has(f.key)}
            <ComboSelect
              label={f.label}
              options={facetValues(f.key)}
              value={filters[f.key] ?? ''}
              placeholder={$t('findb.search.searchFilterPlaceholder', { label: f.label.toLowerCase() })}
              onchange={(v) => (filters = { ...filters, [f.key]: v })}
            />
          {:else}
            <select
              class:set={filters[f.key]}
              value={filters[f.key] ?? ''}
              onchange={(e) => (filters = { ...filters, [f.key]: e.currentTarget.value })}
              aria-label={f.label}
            >
              <option value="">{$t('findb.search.filterAny', { label: f.label })}</option>
              {#each facetValues(f.key) as v}
                <option value={v}>{v}</option>
              {/each}
            </select>
          {/if}
        {/each}
        {#if Object.values(filters).some(Boolean)}
          <button class="link" onclick={() => (filters = {})}>{$t('findb.search.clearFilters')}</button>
        {/if}
      </div>

      {#if !hasCriteria}
        <p class="muted hint">{$t('findb.search.hint')}</p>
      {:else if searching}
        <p class="muted">{$t('findb.search.searching')}</p>
      {:else if results.length === 0}
        <p class="muted">{$t('findb.search.noMatches')}</p>
      {:else}
        <div class="result-meta muted">
          {$t('findb.search.resultCount', { count: results.length, plus: hasMore ? '+' : '', s: results.length === 1 ? '' : 's' })}
        </div>
        <div class="tbl scroll">
          <div class="row head">
            <span>{$t('findb.table.symbol')}</span>
            <span>{$t('findb.table.type')}</span>
            <span>{$t('findb.table.name')}</span>
            <span>{$t('findb.table.details')}</span>
            <span></span>
          </div>
          {#each results as r (r.id)}
            {@const details = [r.exchange, r.currency, r.country, r.sector, r.category]
              .filter(Boolean)
              .join(' · ')}
            <div class="row">
              <strong class="c-sym" title={r.symbol}>{r.symbol}</strong>
              <span class="tag">{typeLabel(r.asset_type)}</span>
              <span class="c-name" title={r.name}>{r.name}</span>
              <span class="c-sub muted" title={details}>{details}</span>
              {#if favIds.has(r.id)}
                <button class="star on" title={$t('findb.favorites.editFavorite')} onclick={() => openSave(r)}><Icon name="star" size={14} /></button>
              {:else}
                <button class="star" title={$t('findb.favorites.saveToFavorites')} onclick={() => openSave(r)}><Icon name="star" size={14} /></button>
              {/if}
            </div>
          {/each}
          {#if hasMore}
            <div bind:this={sentinel} class="sentinel">
              {loadingMore ? $t('findb.search.loadingMore') : ''}
            </div>
          {/if}
        </div>
      {/if}
    {:else if favorites.length === 0}
      <!-- Favorites: empty -->
      <div class="fav-bar">
        <button onclick={() => openFolder()}>{$t('findb.folders.newFolder')}</button>
      </div>
      <p class="muted">{$t('findb.favorites.empty')}</p>
    {:else}
      <!-- Favorites: folder left pane + detail right pane -->
      <div class="fav-layout">
        <div class="fav-col">
          <div class="pane-head">
            <h3>{$t('findb.folders.title')}</h3>
            <button class="link" onclick={() => openFolder()}>{$t('findb.folders.newShort')}</button>
          </div>
          <aside class="folder-pane">
          <button
            class="folder-item"
            class:active={selectedFolder === ''}
            onclick={() => (selectedFolder = '')}
          >
            <span class="fi-name">{$t('findb.folders.all')}</span>
            <span class="fi-count">{favorites.length}</span>
          </button>
          {#each folders as f (f.id)}
            <button
              class="folder-item"
              class:active={selectedFolder === f.id}
              onclick={() => (selectedFolder = f.id)}
            >
              <span class="fi-name">{f.name}</span>
              <span class="fi-count">{counts[f.id] ?? 0}</span>
            </button>
          {/each}
          {#if counts.unfiled > 0}
            <button
              class="folder-item"
              class:active={selectedFolder === 'unfiled'}
              onclick={() => (selectedFolder = 'unfiled')}
            >
              <span class="fi-name">{$t('findb.folders.unfiled')}</span>
              <span class="fi-count">{counts.unfiled}</span>
            </button>
          {/if}
          </aside>
        </div>

        <div class="fav-detail">
          <div class="detail-head">
            <h3>
              {#if selectedFolder === ''}{$t('findb.folders.allFavorites')}
              {:else if selectedFolder === 'unfiled'}{$t('findb.folders.unfiled')}
              {:else}{activeFolder?.name ?? ''}{/if}
            </h3>
            {#if activeFolder}
              <span class="folder-actions">
                <button class="link" onclick={() => openFolder(activeFolder)}>{$t('findb.folders.rename')}</button>
                <button class="link danger" onclick={() => delFolder(activeFolder)}>{$t('findb.folders.delete')}</button>
              </span>
            {/if}
          </div>
          {#if paneItems.length === 0}
            <p class="muted small">{$t('findb.folders.emptyState')}</p>
          {:else}
            <div class="tbl scroll">
              <div class="row head">
                <span>{$t('findb.table.symbol')}</span>
                <span>{$t('findb.table.type')}</span>
                <span>{$t('findb.table.name')}</span>
                <span>{$t('findb.table.note')}</span>
                <span></span>
              </div>
              {#each favShown as f (f.id)}
                <div class="row">
                  <strong class="c-sym" title={f.symbol}>{f.symbol}</strong>
                  <span class="tag">{typeLabel(f.asset_type)}</span>
                  <span class="c-name" title={f.name}>{f.name}</span>
                  <span class="c-sub muted" title={f.note ?? ''}>{f.note ?? ''}</span>
                  <button class="star on" title={$t('findb.favorites.editFavorite')} onclick={() => openSave({ id: f.instrument_id, symbol: f.symbol })}><Icon name="star" size={14} /></button>
                </div>
              {/each}
              {#if favHasMore}
                <div bind:this={favSentinel} class="sentinel">{$t('findb.search.loadingMore')}</div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</div>

<!-- Save-to-favorites modal -->
{#if saving}
  <Modal open={true} title={$t('findb.saveModal.title', { symbol: saving.symbol })} size="sm" onclose={() => (saving = null)}>
    <label class="field">
      <span>{$t('findb.saveModal.folder')}</span>
      <select bind:value={saveFolder}>
        <option value="">{$t('findb.folders.unfiled')}</option>
        {#each folders as f}<option value={f.id}>{f.name}</option>{/each}
        <option value="__new__">{$t('findb.folders.newInline')}</option>
      </select>
    </label>
    {#if saveFolder === '__new__'}
      <label class="field">
        <span>{$t('findb.saveModal.newFolderName')}</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={saveNewFolder} autofocus placeholder={$t('findb.saveModal.folderNamePlaceholder')} />
      </label>
    {/if}
    <label class="field">
      <span>{$t('findb.saveModal.note')}</span>
      <input bind:value={saveNote} placeholder={$t('findb.saveModal.notePlaceholder')} />
    </label>
    <div class="actions">
      <button onclick={() => (saving = null)}>{$t('common.cancel')}</button>
      <button
        class="primary"
        onclick={confirmSave}
        disabled={saveFolder === '__new__' && !saveNewFolder.trim()}
      >{$t('common.save')}</button>
    </div>
  </Modal>
{/if}

<!-- Folder editor -->
{#if folderModal}
  <Modal
    open={true}
    title={editingFolder ? $t('findb.folders.renameTitle') : $t('findb.folders.newTitle')}
    size="sm"
    onclose={() => (folderModal = false)}
  >
    <label class="field">
      <span>{$t('findb.saveModal.name')}</span>
      <input bind:value={folderName} autofocus placeholder={$t('findb.saveModal.folderNamePlaceholder')} />
    </label>
    <div class="actions">
      <button onclick={() => (folderModal = false)}>{$t('common.cancel')}</button>
      <button class="primary" onclick={saveFolderForm}>{$t('common.save')}</button>
    </div>
  </Modal>
{/if}

<!-- Delete-folder confirmation -->
<ConfirmModal
  open={!!deletingFolder}
  title={$t('findb.folders.deleteTitle')}
  message={deletingFolder
    ? $t('findb.folders.deleteMessage', { name: deletingFolder.name })
    : ''}
  confirmLabel={$t('findb.folders.delete')}
  danger
  onconfirm={confirmDelFolder}
  oncancel={() => (deletingFolder = null)}
/>

<style>
  .sk-page {
    padding: var(--space-4);
  }
  .findb {
    padding: var(--space-4);
    max-width: 1032px;
    margin: 0 auto;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-base);
  }
  .gate {
    text-align: center;
    padding: var(--space-8) var(--space-4);
  }
  .gate h1 {
    margin-bottom: var(--space-2);
  }
  .importing {
    color: var(--accent);
    margin-top: var(--space-4);
  }
  .spinner {
    width: 28px;
    height: 28px;
    margin: var(--space-4) auto 0;
    border: 3px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }
  .tabs button {
    background: none;
    border: none;
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    border-bottom: 2px solid transparent;
    cursor: pointer;
  }
  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .search-controls {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }
  .search {
    flex: 1;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    margin-bottom: var(--space-4);
  }
  .filters select {
    max-width: 200px;
  }
  .filters select.set {
    border-color: var(--accent);
    color: var(--accent);
  }
  .hint {
    padding: var(--space-6) 0;
    text-align: center;
  }
  .result-meta {
    font-size: var(--text-sm);
    margin-bottom: var(--space-2);
  }
  .scroll {
    max-height: calc(100vh - 240px);
    overflow-y: auto;
    padding-right: var(--space-1);
  }
  .sentinel {
    display: block;
    text-align: center;
    color: var(--muted);
    background: none;
    border: none;
    padding: var(--space-3);
    font-size: var(--text-base);
    min-height: 1px;
  }
  /* Shared instrument table: one grid template drives header + rows so columns align.
     Long Name/Details cells truncate with ellipsis; full text shows on hover (title). */
  .tbl {
    --tbl-cols: 110px 64px minmax(0, 1.4fr) minmax(0, 1fr) 32px;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .row {
    display: grid;
    grid-template-columns: var(--tbl-cols);
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .row > * {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.head {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .c-sym {
    font-weight: var(--fw-semibold);
  }
  .c-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .c-sub {
    font-size: var(--text-sm);
  }
  .tag {
    font-size: var(--text-xs);
    text-transform: uppercase;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
    justify-self: start;
    overflow: visible;
  }
  .star {
    background: none;
    border: none;
    font-size: var(--text-lg);
    color: var(--muted);
    cursor: pointer;
  }
  .star.on {
    color: var(--amber);
  }
  .fav-bar {
    margin-bottom: var(--space-4);
  }
  .fav-layout {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: var(--space-4);
    align-items: start;
  }
  .folder-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    background: var(--surface);
  }
  .fav-col {
    min-width: 0;
  }
  .fav-detail {
    min-width: 0;
  }
  .folder-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
  }
  .folder-item:hover {
    background: var(--surface-2);
  }
  .folder-item.active {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .fi-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fi-count {
    font-size: var(--text-xs);
    color: var(--muted);
    flex-shrink: 0;
  }
  /* Folder-pane header: a sibling above the folder card, mirroring .detail-head so the
     card's top border lines up with the favorites table's top border. */
  .pane-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .pane-head h3,
  .detail-head h3 {
    font-size: var(--text-md);
  }
  .detail-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .folder-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .folder-actions {
    display: flex;
    gap: var(--space-2);
  }
  .link {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
  }
  .link.danger {
    color: var(--red);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
  button.primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button:not(.primary):not(.star):not(.link):not(.tabs button) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
  }
  .count {
    font-size: var(--text-base);
  }
  .star.on :global(svg) {
    fill: currentColor;
  }
</style>
