<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { onMount, onDestroy } from 'svelte';
  import { newsApi } from '$lib/modules/news/api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import FeedForm from '$lib/modules/news/FeedForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { relativeTime } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let feeds = $state([]); // sources of the selected dashboard
  let items = $state([]);
  // Display timezone from Settings → Defaults ('' = browser-local fallback).
  let timezone = $state('');
  let sourceNames = $state([]);
  let sourceTypes = $state([]);

  // Dashboards
  let dashboards = $state([]);
  let selectedId = $state(''); // currently viewed dashboard
  const selected = $derived(dashboards.find((d) => d.id === selectedId) || null);
  const favorites = $derived(dashboards.filter((d) => d.favorite));
  let showDashMenu = $state(false);

  // Sidebar feed search — filters the left-hand feed list only, by name.
  // Independent from the news-item filters on the right.
  let feedQuery = $state('');
  let kindFilter = $state(''); // '' | 'api' | 'rss'
  let sortBy = $state('name'); // 'name' | 'count'
  let sortDir = $state(1); // 1 = ascending, -1 = reversed
  const filteredFeeds = $derived.by(() => {
    let out = feeds;
    const q = feedQuery.trim().toLowerCase();
    if (q) out = out.filter((f) => (f.name || '').toLowerCase().includes(q));
    if (kindFilter) out = out.filter((f) => f.kind === kindFilter);
    out = [...out].sort((a, b) => {
      const c =
        sortBy === 'count'
          ? (a.item_count || 0) - (b.item_count || 0)
          : (a.name || '').localeCompare(b.name || '', undefined, { sensitivity: 'base' });
      return c * sortDir;
    });
    return out;
  });

  let refreshingAll = $state(false);

  // Filters
  let q = $state('');
  let fSourceNames = $state([]); // multi-select; empty = all sources
  let fSourceType = $state('');
  let fSince = $state('');
  let fUntil = $state('');
  let advanced = $state(false);
  let showSourceMenu = $state(false);
  let viewMode = $state('full'); // 'full' | 'line' (compact: 2-line items)

  function toggleSource(name) {
    fSourceNames = fSourceNames.includes(name)
      ? fSourceNames.filter((n) => n !== name)
      : [...fSourceNames, name];
    applyFilters();
  }

  // ── UI preferences (persisted in localStorage, per-browser) ──
  // Visual + filter state survives a page refresh. Dashboards/sources
  // themselves live in the DB; this is only the view configuration.
  const PREFS_KEY = 'otw.news.prefs.v1';
  let prefsLoaded = false; // gate the save effect until after we restore

  function loadPrefs() {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (p.viewMode === 'full' || p.viewMode === 'line') viewMode = p.viewMode;
      if (typeof p.autoRefresh === 'boolean') autoRefresh = p.autoRefresh;
      if (typeof p.advanced === 'boolean') advanced = p.advanced;
      if (['', 'rss', 'api'].includes(p.kindFilter)) kindFilter = p.kindFilter;
      if (p.sortBy === 'name' || p.sortBy === 'count') sortBy = p.sortBy;
      if (p.sortDir === 1 || p.sortDir === -1) sortDir = p.sortDir;
      if (typeof p.q === 'string') q = p.q;
      if (typeof p.fSourceType === 'string') fSourceType = p.fSourceType;
      if (typeof p.fSince === 'string') fSince = p.fSince;
      if (typeof p.fUntil === 'string') fUntil = p.fUntil;
      if (Array.isArray(p.fSourceNames)) fSourceNames = p.fSourceNames.filter((x) => typeof x === 'string');
      if (typeof p.selectedId === 'string') return p.selectedId; // restored last-open dashboard
    } catch {
      /* corrupt prefs — ignore, use defaults */
    }
    return '';
  }

  // Persist on any change once initial load is done. Reading each value here
  // registers it as a dependency of the effect.
  $effect(() => {
    const snapshot = {
      viewMode,
      autoRefresh,
      advanced,
      kindFilter,
      sortBy,
      sortDir,
      q,
      fSourceType,
      fSince,
      fUntil,
      fSourceNames: [...fSourceNames],
      selectedId
    };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snapshot));
    } catch {
      /* quota / unavailable — non-fatal */
    }
  });

  // Feed form modal
  let showForm = $state(false);
  let editingFeed = $state(null);

  // Live updates
  let liveCount = $state(0); // new items arrived since last load
  let es;

  // Periodic auto-refresh: while the tab is visible, reload the current
  // dashboard's items + sources every AUTO_REFRESH_MS. Paused when hidden (no
  // point polling a backgrounded tab) and skipped while a modal is open so it
  // doesn't fight with the user. Independent from the SSE "new items" pill.
  const AUTO_REFRESH_MS = 60_000;
  let autoRefresh = $state(true);
  let autoTimer;

  function modalOpen() {
    return showForm || promptOpen || confirmOpen;
  }
  async function autoTick() {
    if (!autoRefresh || document.hidden || modalOpen() || !selectedId) return;
    await Promise.all([loadItems(), loadFeeds()]);
  }

  onMount(async () => {
    const lastId = loadPrefs();
    // Load the configured display timezone (non-fatal if it fails).
    settingsApi
      .getDefaults()
      .then((d) => (timezone = d.default_timezone || ''))
      .catch(() => {});
    await loadDashboards();
    // Reopen the last-viewed dashboard if it still exists; otherwise fall back
    // to the default (or the first one).
    selectedId =
      (lastId && dashboards.some((d) => d.id === lastId) && lastId) ||
      (dashboards.find((d) => d.is_default) || dashboards[0])?.id ||
      '';
    prefsLoaded = true; // enable persistence now that restore is complete
    await Promise.all([loadFeeds(), loadItems(), loadSources()]);
    es = newsApi.stream(() => {
      // A poll produced new items — show a "new items" pill; user clicks to load.
      liveCount += 1;
    });
    autoTimer = setInterval(autoTick, AUTO_REFRESH_MS);
  });
  onDestroy(() => {
    es?.close();
    clearInterval(autoTimer);
  });

  async function loadDashboards() {
    dashboards = await newsApi.listDashboards();
  }
  async function loadFeeds() {
    feeds = selectedId ? await newsApi.dashboardSources(selectedId) : [];
  }
  async function loadSources() {
    const s = await newsApi.sources();
    sourceNames = s.source_names;
    sourceTypes = s.source_types;
  }
  // Infinite scroll: 200 per page, keyset cursor on the last item's
  // (sort_key, id). `loadItems` (re)loads page 1; `loadMore` appends the next.
  const PAGE = 200;
  let hasMore = $state(false);
  let loadingMore = $state(false);

  // Svelte action: call `cb` whenever the node enters the viewport (rootMargin
  // pre-loads a bit before the user actually hits the bottom).
  function onVisible(node, cb) {
    const io = new IntersectionObserver(
      (entries) => entries[0]?.isIntersecting && cb(),
      { root: null, rootMargin: '400px' }
    );
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  function baseFilter() {
    return {
      q,
      source_names: fSourceNames.join(','),
      source_type: fSourceType,
      dashboard_id: selectedId,
      since: fSince ? new Date(fSince).toISOString() : '',
      until: fUntil ? new Date(fUntil).toISOString() : '',
      limit: PAGE
    };
  }
  async function loadItems() {
    const page = await newsApi.listItems(baseFilter());
    items = page;
    hasMore = page.length === PAGE;
    liveCount = 0;
  }
  async function loadMore() {
    if (loadingMore || !hasMore || items.length === 0) return;
    loadingMore = true;
    try {
      const last = items[items.length - 1];
      const page = await newsApi.listItems({
        ...baseFilter(),
        before_key: last.sort_key,
        before_id: last.id
      });
      items = [...items, ...page];
      hasMore = page.length === PAGE;
    } finally {
      loadingMore = false;
    }
  }

  // ── Dashboard actions ──
  async function selectDashboard(id) {
    selectedId = id;
    showDashMenu = false;
    await Promise.all([loadFeeds(), loadItems()]);
  }
  // Prompt + confirm dialogs (replace native prompt()/confirm()).
  let promptOpen = $state(false);
  let promptTitle = $state('');
  let promptFields = $state([]);
  let promptConfirmLabel = $state('OK');
  let onPromptConfirm = $state(() => {});

  let confirmOpen = $state(false);
  let confirmTitle = $state('');
  let confirmMessage = $state('');
  let confirmLabel = $state('OK');
  let confirmDanger = $state(false);
  let onConfirmYes = $state(() => {});

  function askPrompt({ title, label, value = '', placeholder = '', confirmLabel: cl = 'OK' }, onconfirm) {
    promptTitle = title;
    promptFields = [{ key: 'name', label, value, placeholder, required: true }];
    promptConfirmLabel = cl;
    onPromptConfirm = onconfirm;
    promptOpen = true;
  }
  function askConfirm({ title, message, confirmLabel: cl = 'OK', danger = false }, onyes) {
    confirmTitle = title;
    confirmMessage = message;
    confirmLabel = cl;
    confirmDanger = danger;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  function newDashboard() {
    askPrompt(
      {
        title: $t('news.dashboard.newTitle'),
        label: $t('news.dashboard.nameLabel'),
        placeholder: $t('news.dashboard.namePlaceholder'),
        confirmLabel: $t('news.dashboard.create')
      },
      async ({ name }) => {
        const d = await newsApi.createDashboard(name.trim());
        await loadDashboards();
        await selectDashboard(d.id);
      }
    );
  }
  async function toggleStarted(d) {
    await newsApi.updateDashboard(d.id, { started: !d.started });
    await loadDashboards();
  }
  async function toggleFavorite(d) {
    await newsApi.updateDashboard(d.id, { favorite: !d.favorite });
    await loadDashboards();
  }
  async function makeDefault(d) {
    await newsApi.setDefaultDashboard(d.id);
    await loadDashboards();
  }
  function renameDashboard(d) {
    askPrompt(
      {
        title: $t('news.dashboard.renameTitle'),
        label: $t('news.dashboard.nameLabel'),
        value: d.name,
        confirmLabel: $t('news.dashboard.rename')
      },
      async ({ name }) => {
        const nm = name.trim();
        if (!nm || nm === d.name) return;
        await newsApi.updateDashboard(d.id, { name: nm });
        await loadDashboards();
      }
    );
  }
  function removeDashboard(d) {
    askConfirm(
      {
        title: $t('news.dashboard.deleteTitle'),
        message: $t('news.dashboard.deleteMessage', { name: d.name }),
        confirmLabel: $t('news.dashboard.delete'),
        danger: true
      },
      async () => {
        await newsApi.deleteDashboard(d.id);
        await loadDashboards();
        if (selectedId === d.id) {
          selectedId = (dashboards.find((x) => x.is_default) || dashboards[0])?.id || '';
          await Promise.all([loadFeeds(), loadItems()]);
        }
      }
    );
  }
  async function refreshDashboard() {
    if (!selectedId) return;
    refreshingAll = true;
    try {
      await newsApi.refreshDashboard(selectedId);
      await Promise.all([loadItems(), loadFeeds(), loadSources()]);
    } finally {
      refreshingAll = false;
    }
  }

  function applyFilters() {
    loadItems();
  }
  function clearFilters() {
    q = fSourceType = fSince = fUntil = '';
    fSourceNames = [];
    loadItems();
  }

  // ── Feed actions ──
  function newFeed() {
    editingFeed = null;
    showForm = true;
  }
  function editFeed(f) {
    editingFeed = f;
    showForm = true;
  }
  async function onSaved() {
    showForm = false;
    await Promise.all([loadFeeds(), loadSources(), loadDashboards()]);
  }
  async function refresh(f) {
    f._refreshing = true;
    feeds = [...feeds];
    try {
      await newsApi.refreshFeed(f.id);
      await Promise.all([loadItems(), loadFeeds(), loadSources()]);
    } finally {
      f._refreshing = false;
      feeds = [...feeds];
    }
  }
  // Remove a source from the current dashboard. If it lives in no other
  // dashboard, offer to delete it (and its items) entirely.
  function removeFeed(f) {
    if (!selectedId) return;
    askConfirm(
      {
        title: $t('news.feed.removeTitle'),
        message: $t('news.feed.removeMessage', { name: f.name || $t('news.feed.unnamed') }),
        confirmLabel: $t('news.feed.remove'),
        danger: true
      },
      async () => {
        await newsApi.removeDashboardSource(selectedId, f.id);
        await Promise.all([loadFeeds(), loadItems(), loadDashboards()]);
      }
    );
  }

  function fmtDate(s) {
    if (!s) return '';
    const d = new Date(s);
    // Render in the timezone configured in Settings → Defaults; fall back to the
    // browser's own zone if none is set or it's invalid.
    const opts = { dateStyle: 'medium', timeStyle: 'short' };
    if (timezone) opts.timeZone = timezone;
    try {
      return d.toLocaleString(undefined, opts);
    } catch {
      // Bad/unknown timezone string — degrade to browser-local.
      return d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
    }
  }
  // Errors about the response shape (bad JSON / items_path / RSS) are tagged
  // "format:" by the backend. Hide those until the feed has actually fetched
  // something at least once — they're just config-in-progress noise before
  // then. Hard errors (network, HTTP) are never tagged and always show.
  // The "format:" marker is internal; strip it from whatever we display.
  function displayError(f) {
    const e = f.last_error;
    if (!e) return '';
    const isFormat = e.startsWith('format:');
    if (isFormat && !f.last_fetched_at) return '';
    return isFormat ? e.slice('format:'.length).trim() : e;
  }

  // Compact relative time, e.g. "3m ago". relativeTime() returns an i18n key plus its
  // params rather than text, so the shared helper stays free of translations.
  // The one call site is guarded by {:else if f.last_success_at}, so `s` is never null.
  function timeSince(s) {
    const r = relativeTime(s);
    return $t(r.key, r.params);
  }
  function host(url) {
    try {
      return new URL(url).hostname.replace(/^www\./, '');
    } catch {
      return '';
    }
  }
</script>

<div class="news-module">
  <!-- Sidebar: feeds management -->
  <aside class="sidebar">
    <div class="side-head">
      <span>{$t('news.sidebar.feeds')}</span>
      <div class="head-actions">
        <button class="add" onclick={refreshDashboard} disabled={refreshingAll || !selectedId} title={$t('news.sidebar.refreshDashboard')}>{refreshingAll ? '…' : '⟳'}</button>
        <button class="add" onclick={newFeed} title={$t('news.sidebar.addSource')}><Icon name="plus" size={16} /></button>
      </div>
    </div>

    <!-- Favorited dashboards: horizontal scrolling shortcut strip. -->
    {#if favorites.length}
      <div class="fav-strip">
        {#each favorites as d (d.id)}
          <button
            class="fav-tag"
            class:active={d.id === selectedId}
            onclick={() => selectDashboard(d.id)}
            title={d.started ? $t('news.dashboard.started') : $t('news.dashboard.stopped')}
          >
            <span class="dot" class:on={d.started}></span>{d.name}
          </button>
        {/each}
      </div>
    {/if}

    <!-- All dashboards: dropdown with status + per-dashboard controls. -->
    <div class="dash-picker">
      <button class="dash-current" onclick={() => (showDashMenu = !showDashMenu)}>
        <span class="dot" class:on={selected?.started}></span>
        <span class="dash-name">{selected?.name || $t('news.dashboard.none')}</span>
        <span class="caret"><Icon name="chevron-down" size={12} /></span>
      </button>
      {#if selected}
        <button
          class="play"
          class:on={selected.started}
          onclick={() => toggleStarted(selected)}
          title={selected.started ? $t('news.dashboard.stopPolling') : $t('news.dashboard.startPolling')}
        ><Icon name={selected.started ? 'pause' : 'play'} size={13} /></button>
      {/if}
      {#if showDashMenu}
        <div class="dash-menu">
          {#each dashboards as d (d.id)}
            <div class="dash-row" class:active={d.id === selectedId}>
              <button class="dash-pick" onclick={() => selectDashboard(d.id)}>
                <span class="dot" class:on={d.started}></span>
                <span class="dash-name">{d.name}</span>
                {#if d.is_default}<span class="badge">{$t('news.dashboard.default')}</span>{/if}
                <span class="count">{d.source_count}</span>
              </button>
              <div class="dash-row-actions">
                <button onclick={() => toggleStarted(d)} title={d.started ? $t('news.dashboard.stop') : $t('news.dashboard.start')}><Icon name={d.started ? 'pause' : 'play'} size={13} /></button>
                <button onclick={() => toggleFavorite(d)} title={d.favorite ? $t('news.dashboard.unfavorite') : $t('news.dashboard.favorite')}><span class:fav={d.favorite}><Icon name="star" size={13} /></span></button>
                <button onclick={() => makeDefault(d)} disabled={d.is_default} title={$t('news.dashboard.setDefault')}><Icon name="home" size={13} /></button>
                <button onclick={() => renameDashboard(d)} title={$t('news.dashboard.rename')}><Icon name="pencil" size={14} /></button>
                <button onclick={() => removeDashboard(d)} title={$t('news.dashboard.deleteTitle')}><Icon name="trash" size={14} /></button>
              </div>
            </div>
          {/each}
          <button class="dash-new" onclick={newDashboard}><Icon name="plus" size={13} /> {$t('news.dashboard.newDashboard')}</button>
        </div>
      {/if}
    </div>

    <div class="feed-search">
      <input placeholder={$t('news.sidebar.filterFeeds')} bind:value={feedQuery} />
      {#if feedQuery}
        <button class="clear-feed-q" onclick={() => (feedQuery = '')} title={$t('common.clear')}><Icon name="x" size={13} /></button>
      {/if}
    </div>
    <div class="feed-controls">
      <div class="kind-tabs">
        <button class:active={kindFilter === ''} onclick={() => (kindFilter = '')}>{$t('news.sidebar.all')}</button>
        <button class:active={kindFilter === 'rss'} onclick={() => (kindFilter = 'rss')}>RSS</button>
        <button class:active={kindFilter === 'api'} onclick={() => (kindFilter = 'api')}>API</button>
      </div>
      <div class="sort-controls">
        <select bind:value={sortBy} title={$t('news.sidebar.sortBy')}>
          <option value="name">{$t('news.sidebar.sortName')}</option>
          <option value="count">{$t('news.sidebar.sortItems')}</option>
        </select>
        <button
          class="dir"
          onclick={() => (sortDir = -sortDir)}
          title={sortBy === 'count'
            ? sortDir === 1 ? $t('news.sidebar.fewestFirst') : $t('news.sidebar.mostFirst')
            : sortDir === 1 ? 'A–Z' : 'Z–A'}
        >
          {sortBy === 'count' ? (sortDir === 1 ? '↑' : '↓') : sortDir === 1 ? 'A→Z' : 'Z→A'}
        </button>
      </div>
    </div>
    <div class="feed-list">
      {#each filteredFeeds as f (f.id)}
        {@const err = displayError(f)}
        <div class="feed">
          <div class="feed-main">
            <span class="kind {f.kind}">{f.kind}</span>
            <span class="feed-name">{f.name || $t('news.feed.unnamed')}</span>
            <span class="item-count" title={$t('news.feed.itemCount', { count: f.item_count, s: f.item_count === 1 ? '' : 's' })}>{f.item_count}</span>
          </div>
          <!-- Deliberately not <ErrorText>: this is one of three mutually exclusive status
               lines on a feed row, truncated to fit. Giving every row in a 20-feed list
               role="alert" would make the sidebar shout on each render. -->
          {#if err}
            <div class="feed-error" title={$t('news.feed.errorTitle', { err })} use:copyLog={err}><Icon name="alert-triangle" size={12} /> {err.slice(0, 60)}</div>
          {:else if f.last_success_at}
            <div class="feed-ok" title={$t('news.feed.updatedTitle', { date: fmtDate(f.last_success_at) })}><Icon name="check" size={12} /> {$t('news.feed.updatedAgo', { time: timeSince(f.last_success_at) })}</div>
          {:else}
            <div class="feed-pending">{$t('news.feed.neverFetched')}</div>
          {/if}
          <div class="feed-actions">
            <button onclick={() => refresh(f)} disabled={f._refreshing} title={$t('news.feed.refreshNow')}>{#if f._refreshing}…{:else}<Icon name="refresh-cw" size={13} />{/if}</button>
            <button onclick={() => editFeed(f)} title={$t('news.feed.edit')}><Icon name="pencil" size={14} /></button>
            <button onclick={() => removeFeed(f)} title={$t('news.feed.removeTitle')}><Icon name="trash" size={14} /></button>
          </div>
        </div>
      {/each}
      {#if !selectedId}
        <p class="empty">{$t('news.sidebar.noDashboard')}</p>
      {:else if feeds.length === 0}
        <p class="empty">{$t('news.sidebar.noSources')}</p>
      {:else if filteredFeeds.length === 0}
        <p class="empty">{$t('news.sidebar.noFeedsMatch', { query: feedQuery })}</p>
      {/if}
    </div>
  </aside>

  <!-- Main: news items + filters -->
  <main class="workarea">
    <div class="filters">
      <input class="search" placeholder={$t('news.filters.searchPlaceholder')} bind:value={q} onkeydown={(e) => e.key === 'Enter' && applyFilters()} />
      <select bind:value={fSourceType} onchange={applyFilters}>
        <option value="">{$t('news.filters.allTypes')}</option>
        {#each sourceTypes as t}<option value={t}>{t}</option>{/each}
      </select>

      <!-- Multi-select sources: checkboxes in a dropdown. -->
      <div class="src-picker">
        <button class="src-toggle" onclick={() => (showSourceMenu = !showSourceMenu)}>
          {fSourceNames.length === 0
            ? $t('news.filters.allSources')
            : fSourceNames.length === 1
              ? fSourceNames[0]
              : $t('news.filters.nSources', { count: fSourceNames.length })}
          <span class="caret"><Icon name="chevron-down" size={12} /></span>
        </button>
        {#if showSourceMenu}
          <div class="src-menu">
            {#if fSourceNames.length}
              <button class="src-clear" onclick={() => { fSourceNames = []; applyFilters(); }}>{$t('news.filters.clearSelection')}</button>
            {/if}
            {#each sourceNames as n}
              <label class="src-opt">
                <input type="checkbox" checked={fSourceNames.includes(n)} onchange={() => toggleSource(n)} />
                <span>{n}</span>
              </label>
            {/each}
            {#if sourceNames.length === 0}
              <p class="src-empty">{$t('news.filters.noSourcesYet')}</p>
            {/if}
          </div>
        {/if}
      </div>

      <button class="apply" onclick={applyFilters}>{$t('news.filters.filter')}</button>
      <button class="link" onclick={() => (advanced = !advanced)}>{advanced ? $t('news.filters.simple') : $t('news.filters.advanced')}</button>
      <button class="link" onclick={clearFilters}>{$t('common.clear')}</button>
      <button
        class="link view"
        onclick={() => (viewMode = viewMode === 'full' ? 'line' : 'full')}
        title={viewMode === 'full' ? $t('news.filters.compactView') : $t('news.filters.fullView')}
      ><Icon name={viewMode === 'full' ? 'list' : 'grid'} size={14} /></button>
      <button
        class="link auto"
        class:on={autoRefresh}
        onclick={() => (autoRefresh = !autoRefresh)}
        title={autoRefresh ? $t('news.filters.autoOnHint') : $t('news.filters.autoOffHint')}
      >⟳ {$t('news.filters.auto')} {autoRefresh ? $t('news.filters.on') : $t('news.filters.off')}</button>
    </div>

    {#if advanced}
      <div class="filters adv">
        <label>{$t('news.filters.from')} <input type="datetime-local" bind:value={fSince} onchange={applyFilters} /></label>
        <label>{$t('news.filters.to')} <input type="datetime-local" bind:value={fUntil} onchange={applyFilters} /></label>
      </div>
    {/if}

    {#if liveCount > 0}
      <button class="live-pill" onclick={loadItems}><Icon name="arrow-up" size={12} /> {$t('news.filters.updatesClickToLoad', { count: liveCount, s: liveCount > 1 ? 's' : '' })}</button>
    {/if}

    <div class="items" class:line={viewMode === 'line'}>
      {#each items as it (it.id)}
        <article class="item">
          <div class="item-head">
            <span class="src">{it.source_name}</span>
            <span class="type {it.source_type}">{it.source_type}</span>
            <span class="when">{fmtDate(it.published_at) || fmtDate(it.fetched_at)}</span>
          </div>
          {#if it.url}
            <a class="title" href={it.url} target="_blank" rel="noreferrer noopener">{it.title || $t('news.item.untitled')}</a>
          {:else}
            <span class="title">{it.title || $t('news.item.untitled')}</span>
          {/if}
          {#if viewMode === 'full'}
            {#if it.summary}<p class="summary">{it.summary}</p>{/if}
            {#if it.url}<span class="host">{host(it.url)}</span>{/if}
          {/if}
        </article>
      {/each}
      {#if items.length === 0}
        <div class="no-items">
          <p>{$t('news.item.noneMatch')}</p>
          <p class="dim">{$t('news.item.addAndRefreshHint')}</p>
        </div>
      {:else if hasMore}
        <!-- Sentinel: when it scrolls into view, fetch the next page. -->
        <div class="sentinel" use:onVisible={loadMore}>
          {loadingMore ? $t('common.loading') : ''}
        </div>
      {:else}
        <p class="end">{$t('news.item.end')}</p>
      {/if}
    </div>
  </main>
</div>

<Modal bind:open={showForm} title={editingFeed ? $t('news.feed.editSource') : $t('news.feed.addSource')}>
  <FeedForm feed={editingFeed} dashboardId={selectedId} onsaved={onSaved} oncancel={() => (showForm = false)} />
</Modal>

<PromptModal
  bind:open={promptOpen}
  title={promptTitle}
  fields={promptFields}
  confirmLabel={promptConfirmLabel}
  onconfirm={(v) => onPromptConfirm(v)}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={confirmTitle}
  message={confirmMessage}
  confirmLabel={confirmLabel}
  danger={confirmDanger}
  onconfirm={() => onConfirmYes()}
/>

<style>
  .news-module {
    display: grid;
    grid-template-columns: 300px 1fr;
    height: 100%;
    min-height: 0;
  }
  .sidebar {
    border-right: 1px solid var(--border);
    background: var(--surface);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .side-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .add {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-md);
  }
  .add:hover {
    color: var(--text);
  }
  .add:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .head-actions {
    display: flex;
    gap: 2px;
  }

  /* ── Dashboards ── */
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--red);
    flex-shrink: 0;
    display: inline-block;
  }
  .dot.on {
    background: var(--green);
  }
  .fav-strip {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    padding: var(--space-2) var(--space-2) 0;
    scrollbar-width: thin;
  }
  .fav-tag {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 14px;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 3px 10px;
    white-space: nowrap;
  }
  .fav-tag:hover {
    color: var(--text);
  }
  .fav-tag.active {
    border-color: var(--accent);
    color: var(--text);
    background: var(--surface-2);
  }
  .dash-picker {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: var(--space-2) var(--space-2) 0;
  }
  .dash-current {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 8px;
    min-width: 0;
  }
  .dash-current .dash-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    text-align: left;
  }
  .dash-current .caret {
    color: var(--muted);
  }
  .play {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 9px;
  }
  .play.on {
    color: var(--green);
    border-color: var(--green);
  }
  .play:hover {
    color: var(--text);
  }
  .dash-menu {
    position: absolute;
    top: 100%;
    left: var(--space-2);
    right: var(--space-2);
    z-index: var(--z-dropdown);
    margin-top: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    max-height: 60vh;
    overflow-y: auto;
    padding: 4px;
  }
  .dash-row {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .dash-row.active {
    background: var(--surface-2);
  }
  .dash-pick {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 6px;
    min-width: 0;
  }
  .dash-pick .dash-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    font-size: var(--text-xs);
    text-transform: uppercase;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 0 4px;
  }
  .dash-pick .count {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .dash-row-actions {
    display: flex;
    gap: 0;
    padding-right: 4px;
  }
  .dash-row-actions button {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 3px 4px;
    border-radius: 4px;
  }
  .dash-row-actions button:hover:not(:disabled) {
    color: var(--text);
    background: var(--bg);
  }
  .dash-row-actions button:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .dash-new {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-top: 1px solid var(--border);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    margin-top: 4px;
    padding: 8px 6px 4px;
  }
  .dash-new:hover {
    color: var(--text);
  }
  .feed-controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-2) 0;
  }
  .kind-tabs,
  .sort-controls {
    display: flex;
    gap: 2px;
  }
  .kind-tabs button,
  .dir {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 3px 8px;
  }
  .kind-tabs button:hover,
  .dir:hover {
    color: var(--text);
  }
  .kind-tabs button.active {
    background: var(--surface-2);
    color: var(--text);
    border-color: var(--accent);
  }
  .feed-search {
    position: relative;
    padding: var(--space-2) var(--space-2) 0;
  }
  .feed-search input {
    width: 100%;
  }
  .clear-feed-q {
    position: absolute;
    right: calc(var(--space-2) + 6px);
    top: calc(var(--space-2) + 6px);
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0;
  }
  .clear-feed-q:hover {
    color: var(--text);
  }
  .feed-list {
    overflow-y: auto;
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .feed {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    background: var(--bg);
  }
  .feed.disabled {
    opacity: 0.55;
  }
  .feed-main {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .feed-name {
    color: var(--text);
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-count {
    margin-left: auto;
    flex-shrink: 0;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    background: var(--surface-2);
    border-radius: 8px;
    padding: 0 6px;
    min-width: 18px;
    text-align: center;
  }
  .kind {
    font-size: var(--text-xs);
    text-transform: uppercase;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
  }
  /* Feed type is categorical identity, not status. The old raw hues were written for the
     dark-only app: as small uppercase text on --surface-2 they measured 2.2:1 (amber) and
     3.7:1 (blue) once the light theme existed. --amber-ink and --accent clear 4.5:1 in both. */
  .kind.rss {
    color: var(--amber-ink);
  }
  .kind.api {
    color: var(--accent);
  }
  .feed-error {
    color: var(--red);
    font-size: var(--text-xs);
    margin-top: 4px;
  }
  .feed-ok {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-top: 4px;
  }
  .feed-pending {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-top: 4px;
    font-style: italic;
  }
  .feed-actions {
    display: flex;
    gap: 2px;
    margin-top: 6px;
  }
  .feed-actions button {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 2px 5px;
    border-radius: 4px;
  }
  .feed-actions button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-4);
    line-height: 1.5;
  }

  .workarea {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
  }
  .filters.adv {
    background: var(--surface);
  }
  .filters.adv label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .search {
    flex: 0 1 240px;
    min-width: 140px;
  }
  .apply {
    background: var(--accent);
    border: none;
    border-radius: var(--radius);
    color: var(--accent-contrast);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 12px;
  }
  .link {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .link:hover {
    color: var(--text);
  }
  .link.auto.on {
    color: var(--green);
  }
  .src-picker {
    position: relative;
  }
  .src-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 8px;
    max-width: 180px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .src-toggle .caret {
    color: var(--muted);
    margin-left: auto;
  }
  .src-menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: var(--z-dropdown);
    margin-top: 4px;
    min-width: 200px;
    max-height: 50vh;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    padding: 4px;
  }
  .src-opt {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border-radius: var(--radius);
    cursor: pointer;
    color: var(--text);
    font-size: var(--text-sm);
  }
  .src-opt:hover {
    background: var(--surface-2);
  }
  .src-opt input {
    width: auto;
    margin: 0;
  }
  .src-clear {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 5px 6px 7px;
    margin-bottom: 4px;
  }
  .src-clear:hover {
    color: var(--text);
  }
  .src-empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: 6px;
    margin: 0;
  }
  .live-pill {
    align-self: center;
    margin: 10px auto 0;
    background: var(--accent);
    border: none;
    border-radius: 14px;
    color: var(--accent-contrast);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 5px 14px;
  }
  .items {
    overflow-y: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .item {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 14px;
    background: var(--surface);
  }
  .item-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }
  .src {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-semibold);
  }
  .type {
    font-size: var(--text-xs);
    text-transform: uppercase;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
  }
  .when {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-left: auto;
  }
  .title {
    color: var(--text);
    font-size: var(--text-md);
    font-weight: var(--fw-semibold);
    text-decoration: none;
    display: block;
  }
  a.title:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  .summary {
    color: var(--muted);
    font-size: var(--text-base);
    margin: 6px 0 0;
    line-height: 1.5;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .host {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-top: 6px;
    display: inline-block;
  }

  /* Compact "line" view: 2 lines — meta (source/type/date) + title only. */
  .items.line {
    gap: 4px;
  }
  .items.line .item {
    padding: 7px 12px;
  }
  .items.line .item-head {
    margin-bottom: 2px;
  }
  .items.line .title {
    font-size: var(--text-base);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .no-items {
    text-align: center;
    color: var(--muted);
    padding: 40px;
  }
  .dim {
    font-size: var(--text-sm);
    opacity: 0.7;
  }
  .sentinel {
    min-height: 1px;
    text-align: center;
    color: var(--muted);
    font-size: var(--text-sm);
    padding: 8px 0;
  }
  .end {
    text-align: center;
    color: var(--muted);
    font-size: var(--text-xs);
    opacity: 0.6;
    padding: 8px 0 0;
  }
  .fav {
    color: var(--amber);
    display: inline-flex;
  }
  .fav :global(svg) {
    fill: currentColor;
  }
</style>
