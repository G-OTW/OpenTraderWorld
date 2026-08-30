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
  import Button from '$lib/ui/Button.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ComboSelect from '$lib/ui/ComboSelect.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
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
    // Only once the catalog is in: facets fetched against an empty table cache an empty
    // list, and the install can finish while this page is open (the gate polls).
    if (meta.installed) loadFacets();
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

  // Set of favorited instrument ids, for the star toggle on results. A favorite whose symbol
  // left the catalog has no id (null) and matches nothing — skip it.
  const favIds = $derived(new Set(favorites.map((f) => f.instrument_id).filter((id) => id != null)));

  // Folder picker in the save modal: Unfiled, then the folders, then "New folder…".
  const folderOptions = $derived([
    { value: '', label: $t('findb.folders.unfiled') },
    ...folders.map((f) => ({ value: f.id, label: f.name })),
    { value: '__new__', label: $t('findb.folders.newInline') }
  ]);

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

  async function removeFav(id) {
    await findbApi.removeFavorite(id);
    await loadFavorites();
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
    <!-- Install gate: a panel that states what the catalog is and what installing costs
         (size, one-time, offline afterwards), rather than a centered line of grey text. -->
    <div class="gate">
      <section class="gate-card">
        <header class="gate-head">
          <span class="gate-icon" aria-hidden="true"><Icon name="database" size={20} strokeWidth={1.5} /></span>
          <div>
            <p class="eyebrow">{$t('findb.gate.eyebrow')}</p>
            <h1>FinanceDatabase</h1>
          </div>
        </header>
        <p class="lead">{$t('findb.gate.description')}</p>

        <div class="stats">
          <div class="stat">
            <span class="v">{$t('findb.gate.statInstrumentsValue')}</span>
            <span class="l">{$t('findb.gate.statInstruments')}</span>
          </div>
          <div class="stat">
            <span class="v">{$t('findb.gate.statSizeValue')}</span>
            <span class="l">{$t('findb.gate.statSize')}</span>
          </div>
          <div class="stat">
            <span class="v">{$t('findb.gate.statOfflineValue')}</span>
            <span class="l">{$t('findb.gate.statOffline')}</span>
          </div>
        </div>

        <div class="gate-foot">
          {#if meta.importing}
            <div class="progress" role="status" aria-label={$t('findb.gate.importing')}>
              <span class="bar"></span>
            </div>
            <p class="importing">{$t('findb.gate.importing')}</p>
          {:else}
            <Button variant="primary" icon="download" onclick={install}>
              {$t('findb.gate.installButton')}
            </Button>
            <p class="fine">{$t('findb.gate.installHint')}</p>
          {/if}
        </div>

        <a
          class="src"
          href="https://github.com/JerBouma/FinanceDatabase"
          target="_blank"
          rel="noreferrer"
        >
          FinanceDatabase — JerBouma
          <Icon name="external-link" size={11} />
        </a>
      </section>
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
        <Dropdown
          bind:value={type}
          options={ASSET_TYPES}
          ariaLabel={$t('findb.search.assetType')}
        />
        <Dropdown bind:value={sort} options={SORTS} ariaLabel={$t('findb.search.sortBy')} />
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
            <div class="facet" class:set={filters[f.key]}>
              <Dropdown
                value={filters[f.key] ?? ''}
                onpick={(v) => (filters = { ...filters, [f.key]: v })}
                ariaLabel={f.label}
                options={[
                  { value: '', label: $t('findb.search.filterAny', { label: f.label }) },
                  ...facetValues(f.key).map((v) => ({ value: v, label: v }))
                ]}
              />
            </div>
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
                {@const gone = f.instrument_id == null}
                <div class="row" class:gone>
                  <strong class="c-sym" title={f.symbol}>{f.symbol}</strong>
                  <span class="tag">{typeLabel(f.asset_type)}</span>
                  <!-- The catalog is a snapshot: a symbol can leave it. The favorite stays (it
                       returns by itself if a later snapshot lists it again) but has no instrument
                       to edit — only to drop. -->
                  <span class="c-name" title={gone ? $t('findb.favorites.missing') : f.name}>
                    {gone ? $t('findb.favorites.missing') : f.name}
                  </span>
                  <span class="c-sub muted" title={f.note ?? ''}>{f.note ?? ''}</span>
                  {#if gone}
                    <button class="star danger" title={$t('findb.favorites.missingRemove')} onclick={() => removeFav(f.id)}><Icon name="trash" size={14} /></button>
                  {:else}
                    <button class="star on" title={$t('findb.favorites.editFavorite')} onclick={() => openSave({ id: f.instrument_id, symbol: f.symbol })}><Icon name="star" size={14} /></button>
                  {/if}
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
    <div class="form">
      <div class="field">
        <span>{$t('findb.saveModal.folder')}</span>
        <Dropdown
          bind:value={saveFolder}
          options={folderOptions}
          ariaLabel={$t('findb.saveModal.folder')}
        />
      </div>
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
    </div>
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
  /* --- install gate --- */
  .gate {
    display: flex;
    justify-content: center;
    padding: var(--space-8) var(--space-4);
  }
  .gate-card {
    width: 100%;
    max-width: 560px;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
  }
  .gate-head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-6) var(--space-6) var(--space-4);
  }
  .gate-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 40px;
    height: 40px;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    color: var(--accent);
  }
  .eyebrow {
    margin: 0 0 var(--space-1);
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
  }
  .gate h1 {
    margin: 0;
    font-size: var(--fs-page-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .lead {
    margin: 0;
    padding: 0 var(--space-6) var(--space-6);
    max-width: 54ch;
    color: var(--dim);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  /* What installing actually costs, as metric cells rather than a sentence. */
  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border-top: var(--hairline) solid var(--border);
    border-bottom: var(--hairline) solid var(--border);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--pad-metric);
    min-width: 0;
  }
  .stat + .stat {
    border-left: var(--hairline) solid var(--border);
  }
  .stat .v {
    font-family: var(--mono);
    font-size: var(--fs-metric-value);
    color: var(--text);
  }
  .stat .l {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .gate-foot {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-6);
  }
  .fine {
    margin: 0;
    max-width: 54ch;
    color: var(--faint);
    font-size: var(--fs-desc);
    line-height: var(--lh-base);
  }
  /* Import has no server-side progress figure: an honest indeterminate sweep. */
  .progress {
    width: 100%;
    height: 2px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .progress .bar {
    display: block;
    width: 32%;
    height: 100%;
    background: var(--accent);
    animation: sweep 1.5s var(--ease) infinite;
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(320%);
    }
  }
  .importing {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .src {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-6);
    border-top: var(--hairline) solid var(--border);
    color: var(--faint);
    font-size: var(--fs-desc);
    text-decoration: none;
  }
  .src:hover {
    color: var(--muted);
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
    border-bottom: 1.5px solid transparent;
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
  /* The two pickers keep a fixed width so the text box takes the rest of the row. */
  .search-controls > :global(.dd) {
    width: 160px;
    flex-shrink: 0;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    margin-bottom: var(--space-4);
  }
  .facet {
    width: 200px;
  }
  /* A set facet keeps the accent rule the native select carried. */
  .facet.set :global(.trigger) {
    border-left: 1.5px solid var(--accent);
    color: var(--text);
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
  /* Continuous instrument table: rows share hairline filets, no gaps, no radius. */
  .tbl {
    --tbl-cols: 110px 64px minmax(0, 1.4fr) minmax(0, 1fr) 32px;
    display: flex;
    flex-direction: column;
    gap: 0;
    border: 0.5px solid var(--border);
  }
  .row {
    display: grid;
    grid-template-columns: var(--tbl-cols);
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--bg);
    border-bottom: 0.5px solid var(--border);
    border-radius: 0;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .row:last-child {
    border-bottom: none;
  }
  .row:not(.head):hover {
    background: var(--surface-2);
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
    /* Above the rows it scrolls under, below the filter menus that open over the table
       (--z-sticky would outrank the dropdown popovers). */
    z-index: 1;
    background: var(--surface-2);
    color: var(--dim);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .c-sym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
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
    border: 0.5px solid var(--border);
    border-radius: 0;
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
    color: var(--accent);
  }
  .star.danger {
    color: var(--red);
  }
  /* A favorite the current snapshot no longer lists: kept, but visibly inert. */
  .row.gone .c-sym,
  .row.gone .tag {
    opacity: 0.55;
  }
  .row.gone .c-name {
    color: var(--muted);
    font-style: italic;
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
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-2);
    background: var(--bg);
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
    border: none;
    border-left: 1.5px solid transparent;
    border-radius: 0;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
  }
  .folder-item:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .folder-item.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
    color: var(--text);
  }
  .fi-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fi-count {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
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
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
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
  /* Modal form: labels stay with their own control, fields keep their distance. */
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .form .field {
    gap: var(--space-2);
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
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
  }
  button:not(.primary):not(.star):not(.link):not(.tabs button):hover {
    background: var(--surface-2);
  }
  .count {
    font-size: var(--text-base);
    font-family: var(--mono);
  }
  .star.on :global(svg) {
    fill: currentColor;
  }
</style>
