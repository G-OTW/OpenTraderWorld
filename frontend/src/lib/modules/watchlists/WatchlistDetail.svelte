<script>
  // One watchlist's working area: the page header (list name, description edited in place,
  // auto-refresh and digest state as the eyebrow, refresh and list actions on the right),
  // one control row (views, filter, flags, auto-refresh, quote source, column gear) and the
  // view itself. The table is the product: 36px rows, sticky sortable headers, figures in
  // tabular mono, changes as signed deltas, the shared sparkline, the 52-week bar. Quotes
  // are read from the server cache; "Refresh" re-quotes server-side. While auto-sync is
  // on, the table quietly re-reads the cache so the background loop's updates appear
  // without a manual reload.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import Sparkline from '$lib/ui/charts/Sparkline.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import AddSymbolModal from './AddSymbolModal.svelte';
  import AlertsModal from './AlertsModal.svelte';
  import RangeBar from './RangeBar.svelte';
  import ColumnPicker from './ColumnPicker.svelte';
  import ItemEditModal from './ItemEditModal.svelte';
  import ListSettingsModal from './ListSettingsModal.svelte';
  import PromoteModal from './PromoteModal.svelte';
  import HeatmapView from './HeatmapView.svelte';
  import CompareView from './CompareView.svelte';
  import CorrelationView from './CorrelationView.svelte';
  import {
    COLUMN_DEFS,
    DEFAULT_COLUMNS,
    FLAGS,
    flagColor,
    distancePct,
    virtualPnl,
    rangePosition,
    fmtBig,
    fmtCap,
    groupBySection,
    moveRow
  } from './views.js';
  import { goto } from '$app/navigation';
  import { getPref, setPref } from '$lib/prefs.svelte.js';
  import {
    watchlistsApi,
    REFRESH_OPTIONS,
    FAST_REFRESH_OPTIONS,
    estimatedReqPerMin,
    RATE_WARN_PER_MIN,
    fmtQuote,
    fmtSignedPct,
    agoLabel
  } from './api.js';
  import { t, locale } from '$lib/i18n';

  // `connectorsTick` is bumped by the page when the broker changes elsewhere; the header's
  // own Connectors button reloads the source pickers here directly.
  let { id, connectorsTick = 0, onchanged = () => {}, ondeleted = () => {} } = $props();

  let wl = $state(null);
  let items = $state([]);
  let alerts = $state([]);
  let loading = $state(false);
  let error = $state('');
  let refreshing = $state(false);

  let showAdd = $state(false);
  let showEdit = $state(false);
  let editingDesc = $state(false);
  let descDraft = $state('');
  let descCancel = false; // Esc blurs the input: this tells the blur handler not to save
  let confirmDeleteList = $state(false);
  let pendingRemove = $state(null); // item awaiting the remove confirm
  let confirmRemoveOpen = $state(false);
  let noteItem = $state(null); // item whose note is being edited
  let noteOpen = $state(false);
  let tickerItem = $state(null); // item whose provider ticker is being edited
  let tickerOpen = $state(false);
  let alertItem = $state(null); // item whose price alerts are open
  let alertsOpen = $state(false);
  let alertPreset = $state(null); // a target turned into a prefilled level alert
  let editItem = $state(null); // item whose row fields (flag, section, targets, virtual) are open
  let editOpen = $state(false);
  let promoteItem = $state(null);
  let promoteOpen = $state(false);
  let settingsOpen = $state(false);

  // table | heatmap | compare | correlation
  let view = $state('table');
  const VIEWS = ['table', 'heatmap', 'compare', 'correlation'];
  // Chart coordinates per item id (absent = no chart for the default crypto path).
  let charts = $state({});
  // Upcoming events and per-item status from stored Fundamentals data.
  let events = $state([]);
  let eventStatus = $state({});
  // News & Research items since the list was last marked read, per item id.
  let newsCounts = $state({});
  let flagFilter = $state(null);
  let collapsed = $state(new Set());
  let renameFrom = $state(null);
  let renameOpen = $state(false);
  let dragId = $state(null);
  // The row last clicked: ringed so the eye keeps its place while a modal opens over it.
  let selectedId = $state(null);

  // Data-broker connectors granted to Watchlists, offered as custom quote sources. An
  // empty list (nothing granted yet) just leaves the select on Auto; the header's
  // Connectors button opens the broker so a source can be added without leaving the list.
  let connectors = $state([]);
  function loadConnectors() {
    watchlistsApi
      .connectors()
      .then((c) => (connectors = c ?? []))
      .catch(() => {});
  }
  $effect(() => {
    connectorsTick;
    loadConnectors();
  });

  let filter = $state('');
  let sortKey = $state(null); // null = the list's own order
  let sortDir = $state(1);

  // A tick that re-renders the "updated Xm ago" label while the page sits open.
  let now = $state(Date.now());

  async function load(quiet = false) {
    if (!quiet) {
      loading = true;
      error = '';
    }
    try {
      const r = await watchlistsApi.detail(id);
      wl = r.watchlist;
      items = r.items;
      alerts = r.alerts ?? [];
      charts = r.charts ?? {};
    } catch (e) {
      if (!quiet) error = e.message;
    } finally {
      loading = false;
    }
    // Not on the 30 s auto-sync re-read: events and news move slower than quotes.
    if (!quiet) loadExtras();
  }

  /** Events and news badges: secondary reads, a failure leaves the table as it is. */
  function loadExtras() {
    const listId = id;
    watchlistsApi
      .events(listId)
      .then((r) => {
        if (listId !== id) return;
        events = r.events ?? [];
        eventStatus = r.status ?? {};
      })
      .catch(() => {});
    watchlistsApi
      .newsCounts(listId)
      .then((r) => listId === id && (newsCounts = r.counts ?? {}))
      .catch(() => {});
  }

  async function markNewsRead() {
    try {
      await watchlistsApi.markNewsSeen(id);
      newsCounts = {};
    } catch (e) {
      error = e.message;
    }
  }

  // The news page keeps its search box in its saved view: setting it there is how a badge
  // opens News & Research already filtered on the symbol.
  function openNews(item) {
    const key = 'otw.news.prefs.v1';
    const prefs = getPref(key, {});
    setPref(key, { ...(prefs && typeof prefs === 'object' ? prefs : {}), q: item.symbol });
    goto('/news');
  }

  function chartHref(item) {
    const c = charts[item.id];
    if (!c) return null;
    const q = new URLSearchParams({ provider: c.provider, asset_type: c.asset_type, ticker: c.ticker, tf: '1d' });
    return `/histviz?${q}`;
  }

  function editRow(item) {
    editItem = item;
    editOpen = true;
  }

  function promote(item) {
    promoteItem = item;
    promoteOpen = true;
  }

  // A target's bell opens the alert builder on a level alert at that price: below for a
  // buy target, above for a sell target.
  function alertFromTarget(item, side) {
    const level = side === 'buy' ? item.target_buy : item.target_sell;
    alertPreset = { metric: 'price', direction: side === 'buy' ? 'below' : 'above', threshold: level, basis: 'anchor', window_secs: 0 };
    alertItem = item;
    alertsOpen = true;
  }

  function rowSaved(updated) {
    items = items.map((i) => (i.id === updated.id ? updated : i));
  }

  function toggleSection(name) {
    const next = new Set(collapsed);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    collapsed = next;
  }

  async function renameSection({ name }) {
    const from = renameFrom;
    renameFrom = null;
    if (from == null) return;
    try {
      items = await watchlistsApi.renameSection(id, from, name.trim());
    } catch (e) {
      error = e.message;
    }
  }

  // Native drag and drop: a row dropped on another row lands before it, in that row's
  // section; dropped on a section header it goes to the end of that section.
  async function dropOn(section, beforeId = null) {
    const moved = dragId;
    dragId = null;
    if (!moved || moved === beforeId) return;
    const flat = groups.flatMap((g) => g.items);
    const order = moveRow(flat, moved, section, beforeId);
    const prev = items;
    const byId = new Map(items.map((i) => [i.id, i]));
    items = order.map((o) => ({ ...byId.get(o.id), section: o.section }));
    try {
      items = await watchlistsApi.reorder(id, order);
    } catch (e) {
      items = prev;
      error = e.message;
    }
  }

  /** Live alerts on one symbol: what the row's bell counts. */
  const armedFor = (item) => alerts.filter((a) => a.item_id === item.id && a.enabled).length;

  function editAlerts(item) {
    alertPreset = null;
    alertItem = item;
    alertsOpen = true;
  }

  // Reload whenever the selected list changes.
  $effect(() => {
    id;
    wl = null;
    filter = '';
    sortKey = null;
    editingDesc = false;
    flagFilter = null;
    selectedId = null;
    events = [];
    eventStatus = {};
    newsCounts = {};
    view = 'table';
    load();
  });

  // While auto-sync is on, re-read the cache so background refreshes show up; the label
  // tick keeps "updated Xm ago" honest either way.
  $effect(() => {
    const syncing = wl?.sync_enabled;
    const timer = setInterval(() => {
      now = Date.now();
      if (syncing) load(true);
    }, 30_000);
    return () => clearInterval(timer);
  });

  async function refreshNow() {
    if (refreshing) return;
    refreshing = true;
    error = '';
    try {
      const r = await watchlistsApi.refresh(id);
      wl = r.watchlist;
      items = r.items;
      alerts = r.alerts ?? [];
      charts = r.charts ?? {};
      loadExtras();
    } catch (e) {
      error = e.message;
    } finally {
      refreshing = false;
    }
  }

  async function patchList(patch) {
    error = '';
    try {
      wl = await watchlistsApi.update(id, patch);
      onchanged();
    } catch (e) {
      error = e.message;
    }
  }

  async function removeList() {
    try {
      await watchlistsApi.remove(id);
      ondeleted();
    } catch (e) {
      error = e.message;
    }
  }

  async function confirmRemoveItem() {
    const item = pendingRemove;
    pendingRemove = null;
    if (!item) return;
    try {
      await watchlistsApi.removeItem(item.id);
      items = items.filter((i) => i.id !== item.id);
      onchanged();
    } catch (e) {
      error = e.message;
    }
  }

  function autofocus(node) {
    node.focus();
    node.select();
  }

  function startDesc() {
    descDraft = wl?.description ?? '';
    descCancel = false;
    editingDesc = true;
  }

  async function saveDesc() {
    editingDesc = false;
    if (descCancel) return;
    const next = descDraft.trim();
    if (next === (wl?.description ?? '')) return;
    await patchList({ description: next });
  }

  function editNote(item) {
    noteItem = item;
    noteOpen = true;
  }

  async function saveNote({ notes }) {
    const item = noteItem;
    noteItem = null;
    if (!item) return;
    try {
      const updated = await watchlistsApi.updateItem(item.id, { notes: notes.trim() });
      items = items.map((i) => (i.id === updated.id ? updated : i));
    } catch (e) {
      error = e.message;
    }
  }

  function editTicker(item) {
    tickerItem = item;
    tickerOpen = true;
  }

  // The server re-quotes on a source/ticker change, so the returned item carries the fresh
  // quote (or quote.error when the symbol doesn't resolve on the connector).
  async function saveTicker({ quote_source, quote_ticker }) {
    const item = tickerItem;
    tickerItem = null;
    if (!item) return;
    try {
      const updated = await watchlistsApi.updateItem(item.id, {
        quote_source,
        quote_ticker: quote_ticker.trim()
      });
      items = items.map((i) => (i.id === updated.id ? updated : i));
    } catch (e) {
      error = e.message;
    }
  }

  // Mirror of the server's per-item source resolution: explicit choice > list default >
  // auto, with the list default skipping asset classes its connector can't serve.
  function resolvedConnector(item) {
    const src = item.quote_source ?? '';
    if (src === 'auto') return null;
    const cid = src || wl?.connector_id;
    if (!cid) return null;
    const c = connectors.find((x) => x.id === cid);
    if (!c) return null;
    const at = item.asset_class === 'stock' ? 'equity' : item.asset_class;
    if (!c.asset_types?.includes(at)) return src ? c : null; // explicit mismatch still shows (server errors it)
    return c;
  }

  function toggleSort(key) {
    if (sortKey === key) {
      if (sortDir === 1) sortDir = -1;
      else (sortKey = null), (sortDir = 1); // third click restores the list's own order
    } else {
      sortKey = key;
      sortDir = key === 'symbol' ? 1 : -1; // numbers open with the biggest mover on top
    }
  }

  const SORT_VALUE = {
    symbol: (i) => i.symbol.toLowerCase(),
    price: (i) => i.quote?.price_usd,
    c24: (i) => i.quote?.change_24h,
    c3: (i) => i.quote?.change_3d,
    c7: (i) => i.quote?.change_7d,
    c30: (i) => i.quote?.change_30d,
    range52: (i) => rangePosition(i.quote?.price_usd, i.quote?.low_52w, i.quote?.high_52w),
    mcap: (i) => i.quote?.mcap,
    vol: (i) => i.quote?.volume,
    pe: (i) => i.quote?.pe,
    divy: (i) => i.quote?.div_yield,
    rsi: (i) => i.quote?.rsi_14,
    dist52h: (i) => distancePct(i.quote?.high_52w, i.quote?.price_usd)
  };

  let visible = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    let out = q
      ? items.filter(
          (i) => i.symbol.toLowerCase().includes(q) || i.name.toLowerCase().includes(q)
        )
      : [...items];
    if (flagFilter) out = out.filter((i) => i.flag === flagFilter);
    if (sortKey) {
      const val = SORT_VALUE[sortKey];
      out.sort((a, b) => {
        const va = val(a);
        const vb = val(b);
        if (va == null && vb == null) return 0;
        if (va == null) return 1; // quote-less rows sink regardless of direction
        if (vb == null) return -1;
        return (va < vb ? -1 : va > vb ? 1 : 0) * sortDir;
      });
    }
    return out;
  });

  let custom = $derived(!!wl?.connector_id);
  // Any custom source in play (list default or per-item pin) shows the "know your limits"
  // note; the free-source throttle warning covers the remaining auto-sourced items.
  let customInPlay = $derived(
    custom || items.some((i) => i.quote_source && i.quote_source !== 'auto')
  );
  let reqPerMin = $derived(
    wl ? estimatedReqPerMin(items, wl.refresh_secs, (i) => !!resolvedConnector(i)) : 0
  );
  let autoReqPerMin = $derived(
    wl
      ? estimatedReqPerMin(
          items.filter((i) => !resolvedConnector(i)),
          wl.refresh_secs
        )
      : 0
  );
  let rateRisk = $derived(!!wl?.sync_enabled && autoReqPerMin > RATE_WARN_PER_MIN);
  let intervalOptions = $derived(custom ? [...FAST_REFRESH_OPTIONS, ...REFRESH_OPTIONS] : REFRESH_OPTIONS);

  /** The provider behind the item's pinned source, so the symbol helper opens on it. */
  let tickerProvider = $derived(
    connectors.find((c) => c.id === tickerItem?.quote_source)?.provider ?? ''
  );

  // Source picker options for the per-item modal.
  let sourceOptions = $derived([
    { value: '', label: $t('watchlists.table.sourceDefault') },
    { value: 'auto', label: $t('watchlists.detail.providerAuto') },
    ...connectors.map((c) => ({ value: c.id, label: `${c.label} · ${c.name}` }))
  ]);

  let updatedAgo = $derived.by(() => {
    now; // re-derive on the tick
    return wl?.refreshed_at ? agoLabel(wl.refreshed_at, $locale) : null;
  });

  // The eyebrow over the list name: how the list keeps itself fresh, and whether the
  // daily digest goes out.
  let eyebrow = $derived.by(() => {
    if (!wl) return '';
    const opt = [...FAST_REFRESH_OPTIONS, ...REFRESH_OPTIONS].find((o) => o.secs === wl.refresh_secs);
    const sync = wl.sync_enabled
      ? $t('watchlists.header.syncEvery', { interval: opt ? $t(opt.key) : `${wl.refresh_secs} s` })
      : $t('watchlists.header.syncOff');
    const digest =
      wl.digest === 'daily'
        ? $t('watchlists.header.digestAt', { hour: `${String(wl.digest_hour ?? 21).padStart(2, '0')}:00 UTC` })
        : $t('watchlists.header.digestOff');
    return `${sync} · ${digest}`;
  });

  // Columns this list shows, in catalog order.
  let cols = $derived(COLUMN_DEFS.filter((c) => (wl?.columns ?? DEFAULT_COLUMNS).includes(c.id)));
  let groups = $derived(groupBySection(visible));
  let sections = $derived([...new Set(items.map((i) => i.section).filter(Boolean))]);
  // Reordering only means something on the list's own order, unfiltered.
  let canDrag = $derived(!sortKey && !filter.trim() && !flagFilter);
  let flagsInUse = $derived(FLAGS.filter((f) => items.some((i) => i.flag === f.id)));
  let anyNews = $derived(Object.values(newsCounts).some((n) => n > 0));
  // Events: the list-wide strip, and whether any symbol has event data at all.
  let upcoming = $derived(events.slice(0, 12));
  let anyEventData = $derived(Object.values(eventStatus).some((s) => s !== 'no_data'));
  const eventsFor = (item) => events.filter((e) => e.item_id === item.id).slice(0, 2);
  const shortDate = (d) => new Date(`${d}T00:00:00Z`).toLocaleDateString($locale, { month: 'short', day: 'numeric', timeZone: 'UTC' });
  let na = $derived($t('watchlists.col.na'));
  let order = $derived(items.map((i) => i.id));

  /** A quote number for the extra columns: null (read as n/a) when the source did not serve it. */
  function cellNum(v, c) {
    if (typeof v !== 'number' || !isFinite(v)) return null;
    if (c.id === 'mcap') return fmtCap(v);
    if (c.kind === 'big') return fmtBig(v);
    if (c.kind === 'pct') return `${v.toFixed(2)}%`;
    return v.toFixed(1);
  }
  /** A change rounded to what Delta prints (2 decimals), so +0.004% reads as a flat 0.00%
   *  without an up arrow and a gain tone it does not have. */
  const r2 = (v) => (typeof v === 'number' && isFinite(v) ? Math.round(v * 100) / 100 : v);
  /** RSI 14 outside 30..70 earns the amber dot, with its word for screen readers and hover. */
  const rsiState = (v) =>
    typeof v !== 'number' ? null : v > 70 ? 'overbought' : v < 30 ? 'oversold' : null;

  function saveColumns(next) {
    patchList({ columns: next });
  }

  // Header cells: everything but the free-text and drawing columns sorts.
  const UNSORTED = new Set(['spark', 'targets', 'notes']);
  const NUMERIC = new Set(['change', 'big', 'ratio', 'pct', 'dist52h', 'targets']);
  const thClass = (c) => (NUMERIC.has(c.kind) ? 'num' : '') + (c.kind === 'spark' ? ' sparkth' : '');

  function selectRow(e, item) {
    if (e.target.closest('button, a, input')) return;
    selectedId = selectedId === item.id ? null : item.id;
  }
</script>

{#snippet sortHead(key, label, numeric)}
  <button class="sort" class:numeric onclick={() => toggleSort(key)} aria-label={label}>
    <span>{label}</span>
    <span class="caret" class:active={sortKey === key}>
      <Icon name={sortKey === key && sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />
    </span>
  </button>
{/snippet}

{#if loading && !wl}
  <div class="loader" role="status" aria-label={$t('common.loading')}>
    <span class="spinner" aria-hidden="true"></span>
  </div>
{:else if wl}
  <div class="detail">
    <PageHeader {eyebrow} title={wl.name}>
      {#snippet actions()}
        <span class="updated">
          {#if refreshing}
            {$t('watchlists.detail.refreshing')}
          {:else if updatedAgo}
            {$t('watchlists.detail.updated', { ago: updatedAgo })}
          {:else}
            {$t('watchlists.detail.neverRefreshed')}
          {/if}
        </span>
        <Button icon="refresh-cw" loading={refreshing} onclick={refreshNow}>
          {$t('watchlists.detail.refresh')}
        </Button>
        <ConnectorButton module="watchlists" onchanged={() => (loadConnectors(), onchanged())} />
        <span class="iconset">
          <button class="icon" class:lit={wl.digest === 'daily'} onclick={() => (settingsOpen = true)} aria-label={$t('watchlists.settings.title')} title={$t('watchlists.settings.title')}>
            <Icon name="cog" size={15} />
          </button>
          <button class="icon" onclick={() => (showEdit = true)} aria-label={$t('watchlists.detail.editAria')} title={$t('watchlists.detail.editAria')}>
            <Icon name="pencil" size={15} />
          </button>
          <button class="icon danger" onclick={() => (confirmDeleteList = true)} aria-label={$t('watchlists.detail.deleteAria')} title={$t('watchlists.detail.deleteAria')}>
            <Icon name="trash" size={15} />
          </button>
        </span>
        <Button variant="primary" icon="plus" onclick={() => (showAdd = true)}>
          {$t('watchlists.detail.addSymbol')}
        </Button>
      {/snippet}
      <!-- One-liner under the title, edited in place: click it, Enter/blur saves, Esc drops. -->
      {#if editingDesc}
        <input
          class="descinput"
          use:autofocus
          bind:value={descDraft}
          maxlength="160"
          placeholder={$t('watchlists.detail.descAdd')}
          onkeydown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur();
            else if (e.key === 'Escape') ((descCancel = true), e.currentTarget.blur());
          }}
          onblur={saveDesc}
        />
      {:else}
        <button class="desc" class:empty={!wl.description} onclick={startDesc} title={wl.description || ''}>
          {wl.description || $t('watchlists.detail.descAdd')}
        </button>
      {/if}
    </PageHeader>

    {#if customInPlay}
      <div class="ratewarn">
        <Badge tone="warn" icon="alert-triangle">{$t('watchlists.detail.customProviderWarn')}</Badge>
      </div>
    {:else if rateRisk}
      <div class="ratewarn">
        <Badge tone="warn" icon="alert-triangle">{$t('watchlists.detail.rateWarning')}</Badge>
      </div>
    {/if}

    <ErrorText error={error} copyable />

    <!-- The control row: views first, then what scopes the table, then how it refreshes. -->
    <div class="controls">
      <div class="views" role="tablist" aria-label={$t('watchlists.view.aria')}>
        {#each VIEWS as v (v)}
          <button role="tab" class="chip" class:active={view === v} aria-selected={view === v} onclick={() => (view = v)}>
            {$t(`watchlists.view.${v}`)}
          </button>
        {/each}
      </div>
      {#if view === 'table' && items.length}
        <span class="filterbox">
          <Icon name="search" size={13} />
          <input
            class="filter"
            type="search"
            placeholder={$t('watchlists.detail.filterPlaceholder')}
            aria-label={$t('watchlists.detail.filterPlaceholder')}
            bind:value={filter}
          />
        </span>
        {#if flagsInUse.length}
          <div class="flagchips" role="group" aria-label={$t('watchlists.flag.filterAria')}>
            {#each flagsInUse as f (f.id)}
              <button
                class="flagchip"
                class:on={flagFilter === f.id}
                onclick={() => (flagFilter = flagFilter === f.id ? null : f.id)}
                title={$t(`watchlists.flag.${f.id}`)}
                aria-label={$t(`watchlists.flag.${f.id}`)}
                aria-pressed={flagFilter === f.id}
              >
                <span class="dot" style:background={f.color}></span>
              </button>
            {/each}
          </div>
        {/if}
        {#if anyNews}
          <Button size="sm" variant="ghost" icon="newspaper" onclick={markNewsRead}>{$t('watchlists.news.markRead')}</Button>
        {/if}
      {/if}
      <span class="spacer"></span>
      <label class="sync">
        <input
          type="checkbox"
          checked={wl.sync_enabled}
          onchange={(e) => patchList({ sync_enabled: e.currentTarget.checked })}
        />
        {$t('watchlists.detail.sync')}
      </label>
      <div class="interval">
        <Dropdown
          disabled={!wl.sync_enabled}
          value={String(wl.refresh_secs)}
          onpick={(v) => patchList({ refresh_secs: Number(v) })}
          ariaLabel={$t('watchlists.detail.sync')}
          options={intervalOptions.map((o) => ({ value: String(o.secs), label: $t(o.key) }))}
        />
      </div>
      {#if connectors.length > 0}
        <div class="interval source">
          <Dropdown
            title={$t('watchlists.detail.provider')}
            value={wl.connector_id ?? ''}
            onpick={(v) => patchList({ connector_id: v })}
            ariaLabel={$t('watchlists.detail.provider')}
            options={[
              { value: '', label: $t('watchlists.detail.providerAuto') },
              ...connectors.map((c) => ({ value: c.id, label: `${c.label} · ${c.name}` }))
            ]}
          />
        </div>
      {/if}
      {#if view === 'table' && items.length}
        <ColumnPicker columns={wl.columns ?? DEFAULT_COLUMNS} onchange={saveColumns} />
      {/if}
    </div>

    {#if items.length === 0}
      <EmptyState icon="star" description={$t('watchlists.detail.emptyItems')}>
        {#snippet action()}
          <Button variant="primary" icon="plus" onclick={() => (showAdd = true)}>
            {$t('watchlists.detail.addSymbol')}
          </Button>
        {/snippet}
      </EmptyState>
    {:else if view === 'heatmap'}
      <HeatmapView {items} onopen={(it) => it && chartHref(it) && goto(chartHref(it))} />
    {:else if view === 'compare'}
      <CompareView {id} {order} onrefresh={refreshNow} stamp={wl.refreshed_at} />
    {:else if view === 'correlation'}
      <CorrelationView {id} onrefresh={refreshNow} stamp={wl.refreshed_at} />
    {:else}
      <!-- List-wide upcoming strip: dates from stored Fundamentals data only. One row,
           scrolls sideways when it runs long. -->
      <div class="strip">
        <span class="striplbl">{$t('watchlists.events.upcoming')}</span>
        {#if upcoming.length}
          <span class="striprow">
            {#each upcoming as e (e.item_id + e.kind + e.date)}
              <Badge title={e.provider}>
                <b>{e.symbol}</b> {$t(`watchlists.events.${e.kind}`)} {shortDate(e.date)}
              </Badge>
            {/each}
          </span>
        {:else if anyEventData}
          <span class="stripnote">{$t('watchlists.events.noneUpcoming')}</span>
        {:else}
          <span class="stripnote">{$t('watchlists.events.noData')}</span>
        {/if}
      </div>

      <div class="tablewrap">
        <table class="wtbl">
          <thead>
            <tr>
              <th class="symth" aria-sort={sortKey === 'symbol' ? (sortDir === 1 ? 'ascending' : 'descending') : 'none'}>
                {@render sortHead('symbol', $t('watchlists.table.symbol'), false)}
              </th>
              <th class="num" aria-sort={sortKey === 'price' ? (sortDir === 1 ? 'ascending' : 'descending') : 'none'}>
                {@render sortHead('price', $t('watchlists.table.price'), true)}
              </th>
              {#each cols as c (c.id)}
                {#if UNSORTED.has(c.id)}
                  <th class={thClass(c)}>{$t(c.key)}</th>
                {:else}
                  <th class={thClass(c)} aria-sort={sortKey === c.id ? (sortDir === 1 ? 'ascending' : 'descending') : 'none'}>
                    {@render sortHead(c.id, $t(c.key), NUMERIC.has(c.kind))}
                  </th>
                {/if}
              {/each}
              <th class="actth"><span class="sr">{$t('watchlists.row.actions')}</span></th>
            </tr>
          </thead>
          <tbody>
            {#each groups as g (g.section ?? '')}
              {#if g.section !== null || groups.length > 1}
                {@const open = !collapsed.has(g.section ?? '')}
                <tr
                  class="sechead"
                  ondragover={(e) => canDrag && dragId && e.preventDefault()}
                  ondrop={(e) => (e.preventDefault(), dropOn(g.section))}
                >
                  <td colspan={cols.length + 3}>
                    <span class="secline">
                      <button class="secbtn" onclick={() => toggleSection(g.section ?? '')} aria-expanded={open}>
                        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />
                        <span class="secname">{g.section ?? $t('watchlists.section.ungrouped')}</span>
                        <span class="seccount">{g.items.length}</span>
                      </button>
                      {#if g.section !== null}
                        <button class="icon xs secedit" onclick={() => ((renameFrom = g.section), (renameOpen = true))} aria-label={$t('watchlists.section.rename')} title={$t('watchlists.section.rename')}>
                          <Icon name="pencil" size={11} />
                        </button>
                      {/if}
                    </span>
                  </td>
                </tr>
              {/if}
              {#if !collapsed.has(g.section ?? '')}
                {#each g.items as item (item.id)}
                  {@const vp = virtualPnl(item.virt_qty, item.virt_price, item.quote?.price_usd)}
                  {@const armed = armedFor(item)}
                  {@const href = chartHref(item)}
                  <tr
                    draggable={canDrag}
                    class:dragging={dragId === item.id}
                    class:sel={selectedId === item.id}
                    onclick={(e) => selectRow(e, item)}
                    ondragstart={(e) => ((dragId = item.id), e.dataTransfer?.setData('text/plain', item.id))}
                    ondragend={() => (dragId = null)}
                    ondragover={(e) => canDrag && dragId && e.preventDefault()}
                    ondrop={(e) => (e.preventDefault(), dropOn(item.section ?? null, item.id))}
                  >
                    <td class="symtd">
                      <div class="symcell">
                        <span class="grip" class:on={canDrag} aria-hidden="true" title={canDrag ? $t('watchlists.row.drag') : ''}>
                          <Icon name="grip-vertical" size={12} />
                        </span>
                        <span class="flagdot" style:background={item.flag ? flagColor(item.flag) : 'transparent'} title={item.flag ? $t(`watchlists.flag.${item.flag}`) : ''}></span>
                        <span class="symtext">
                          <span class="symrow">
                            <span class="sym">{item.symbol}</span>
                            {#if newsCounts[item.id] > 0}
                              <button class="badge accent newsb" onclick={() => openNews(item)} title={$t('watchlists.news.badgeTitle', { n: newsCounts[item.id], symbol: item.symbol })}>
                                <Icon name="newspaper" size={10} />{newsCounts[item.id]}
                              </button>
                            {/if}
                            {#each eventsFor(item) as e (e.kind + e.date)}
                              <span class="badge evt" title={e.provider}>{$t(`watchlists.events.${e.kind}`)} {shortDate(e.date)}</span>
                            {/each}
                          </span>
                          <span class="subtle">
                            <!-- The name gives way first; the source tag is an action and stays whole. -->
                            <span class="nm" title="{item.name}{item.exchange ? ` · ${item.exchange}` : ''}">
                              {item.name}{#if item.exchange}&nbsp;· {item.exchange}{:else if item.asset_class === 'crypto'}&nbsp;· crypto{/if}
                            </span>
                            {#if connectors.length > 0}
                              {@const rc = resolvedConnector(item)}
                              <button class="ticker" onclick={() => editTicker(item)} title={$t('watchlists.table.tickerTitle', { symbol: item.symbol })}>
                                {#if rc}{rc.label} · {item.quote_ticker || $t('watchlists.table.tickerAuto')}{:else}{$t('watchlists.table.tickerAuto')}{/if}
                                <Icon name="pencil" size={9} />
                              </button>
                            {/if}
                          </span>
                        </span>
                      </div>
                    </td>
                    <td class="num price" data-label={$t('watchlists.table.price')} title={item.quoted_at ? new Date(item.quoted_at).toLocaleString() : ''}>
                      <span class="pricecol">
                        <span class="pv">
                          {#if item.quote?.error}
                            <span class="quoteerr" title={item.quote.error}><Icon name="alert-triangle" size={12} /></span>
                          {/if}
                          {#if item.quote?.price_usd == null}<span class="na">{na}</span>{:else}{fmtQuote(item.quote.price_usd)}{/if}
                        </span>
                        {#if vp}
                          <span class="virt" title={$t('watchlists.virt.title', { qty: item.virt_qty, price: fmtQuote(item.virt_price), date: item.virt_date ?? '' })}>
                            <Delta value={vp.pnl} format={(a) => fmtQuote(a)} sensitive arrow={false} />
                            <Delta value={r2(vp.pct)} arrow={false} />
                          </span>
                        {/if}
                      </span>
                    </td>
                    {#each cols as c (c.id)}
                      {#if c.kind === 'change'}
                        <td class="num" data-label={$t(c.key)}><Delta value={r2(item.quote?.[c.field])} empty={na} /></td>
                      {:else if c.kind === 'spark'}
                        <td class="sparkcell"><Sparkline values={item.quote?.spark ?? []} width={96} height={28} label={$t('watchlists.table.trend')} /></td>
                      {:else if c.kind === 'range'}
                        <td class="rangecell" data-label={$t(c.key)}><RangeBar price={item.quote?.price_usd} low={item.quote?.low_52w} high={item.quote?.high_52w} /></td>
                      {:else if c.kind === 'dist52h'}
                        {@const d = distancePct(item.quote?.high_52w, item.quote?.price_usd)}
                        <td class="num" data-label={$t(c.key)}>{#if d == null}<span class="na" title={$t('watchlists.col.notServed')}>{na}</span>{:else}{fmtSignedPct(d)}{/if}</td>
                      {:else if c.kind === 'targets'}
                        {@const db = distancePct(item.quote?.price_usd, item.target_buy)}
                        {@const ds = distancePct(item.quote?.price_usd, item.target_sell)}
                        <td class="num targets" data-label={$t(c.key)}>
                          {#if item.target_buy == null && item.target_sell == null}
                            <button class="note addnote" onclick={() => editRow(item)}><Icon name="plus" size={11} /> {$t('watchlists.targets.set')}</button>
                          {:else}
                            <span class="tgts">
                              {#if item.target_buy != null}
                                <span class="tgt">
                                  <span class="tlbl">{$t('watchlists.targets.buy')}</span>
                                  <span class="tv">{fmtQuote(item.target_buy)}</span>
                                  <span class="td">{db == null ? na : fmtSignedPct(db, 1)}</span>
                                  <button class="icon xs tbell" onclick={() => alertFromTarget(item, 'buy')} aria-label={$t('watchlists.targets.alert')} title={$t('watchlists.targets.alert')}><Icon name="bell" size={10} /></button>
                                </span>
                              {/if}
                              {#if item.target_sell != null}
                                <span class="tgt">
                                  <span class="tlbl">{$t('watchlists.targets.sell')}</span>
                                  <span class="tv">{fmtQuote(item.target_sell)}</span>
                                  <span class="td">{ds == null ? na : fmtSignedPct(ds, 1)}</span>
                                  <button class="icon xs tbell" onclick={() => alertFromTarget(item, 'sell')} aria-label={$t('watchlists.targets.alert')} title={$t('watchlists.targets.alert')}><Icon name="bell" size={10} /></button>
                                </span>
                              {/if}
                            </span>
                          {/if}
                        </td>
                      {:else if c.kind === 'notes'}
                        <td class="notecell">
                          {#if item.notes}
                            <button class="note" onclick={() => editNote(item)} title={item.notes}>{item.notes}</button>
                          {:else}
                            <button class="note addnote" onclick={() => editNote(item)}>
                              <Icon name="plus" size={11} /> {$t('watchlists.table.addNote')}
                            </button>
                          {/if}
                        </td>
                      {:else if c.id === 'rsi'}
                        {@const v = item.quote?.rsi_14}
                        {@const st = rsiState(v)}
                        <td class="num" data-label={$t(c.key)} title={st ? $t(`watchlists.col.${st}`) : v == null ? $t('watchlists.col.notServed') : ''}>
                          {#if typeof v !== 'number'}
                            <span class="na">{na}</span>
                          {:else}
                            <span class="rsi">
                              {#if st}<span class="rdot" aria-hidden="true"></span><span class="sr">{$t(`watchlists.col.${st}`)}</span>{/if}
                              {v.toFixed(1)}
                            </span>
                          {/if}
                        </td>
                      {:else}
                        {@const v = cellNum(item.quote?.[c.field], c)}
                        <td class="num" data-label={$t(c.key)} title={v == null ? $t('watchlists.col.notServed') : ''}>
                          {#if v == null}<span class="na">{na}</span>{:else}{v}{/if}
                        </td>
                      {/if}
                    {/each}
                    <td class="actions">
                      <!-- Idle until it means something: the bell only shows on row hover while
                           the symbol has no armed alert, and stays lit once it has one. -->
                      <button
                        class="icon bell"
                        class:armed={armed > 0}
                        onclick={() => editAlerts(item)}
                        aria-label={$t('watchlists.alerts.openAria', { symbol: item.symbol })}
                        title={armed > 0 ? $t('watchlists.alerts.armedCount', { n: armed }) : $t('watchlists.alerts.addAria')}
                      >
                        <Icon name="bell" size={13} />
                        {#if armed > 1}<span class="count">{armed}</span>{/if}
                      </button>
                      {#if href}
                        <a class="icon rowact" {href} aria-label={$t('watchlists.row.chart')} title={$t('watchlists.row.chart')}>
                          <Icon name="candlestick" size={13} />
                        </a>
                      {:else}
                        <span class="icon rowact off" title={$t('watchlists.row.noChart')}><Icon name="candlestick" size={13} /></span>
                      {/if}
                      <button class="icon rowact" onclick={() => promote(item)} aria-label={$t('watchlists.row.promote')} title={$t('watchlists.row.promote')}>
                        <Icon name="briefcase" size={13} />
                      </button>
                      <button class="icon rowact" onclick={() => editRow(item)} aria-label={$t('watchlists.row.edit')} title={$t('watchlists.row.edit')}>
                        <Icon name="tag" size={13} />
                      </button>
                      <button class="icon rowact danger" onclick={() => ((pendingRemove = item), (confirmRemoveOpen = true))} aria-label={$t('watchlists.table.removeAria')} title={$t('watchlists.table.removeAria')}>
                        <Icon name="x" size={13} />
                      </button>
                    </td>
                  </tr>
                {/each}
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

  <AddSymbolModal
    bind:open={showAdd}
    watchlistId={id}
    onadded={() => (load(true), loadExtras(), onchanged())}
  />

  <PromptModal
    bind:open={showEdit}
    title={$t('watchlists.detail.editTitle')}
    confirmLabel={$t('common.save')}
    fields={[
      { key: 'name', label: $t('watchlists.detail.nameLabel'), value: wl.name, required: true },
      { key: 'description', label: $t('watchlists.detail.descLabel'), value: wl.description }
    ]}
    onconfirm={(v) => patchList({ name: v.name.trim(), description: v.description.trim() })}
  />

  <PromptModal
    bind:open={noteOpen}
    title={$t('watchlists.table.noteTitle', { symbol: noteItem?.symbol ?? '' })}
    confirmLabel={$t('common.save')}
    fields={[{ key: 'notes', label: '', value: noteItem?.notes ?? '' }]}
    onconfirm={saveNote}
    oncancel={() => (noteItem = null)}
  />

  <PromptModal
    bind:open={tickerOpen}
    title={$t('watchlists.table.tickerTitle', { symbol: tickerItem?.symbol ?? '' })}
    confirmLabel={$t('common.save')}
    fields={[
      {
        key: 'quote_source',
        label: $t('watchlists.detail.provider'),
        type: 'select',
        value: tickerItem?.quote_source ?? '',
        options: sourceOptions
      },
      {
        key: 'quote_ticker',
        label: $t('watchlists.table.quoteTicker'),
        value: tickerItem?.quote_ticker ?? '',
        placeholder: $t('watchlists.table.tickerAuto'),
        // Typed by hand and read by the connector, so it is spelled that provider's way.
        symbolHelp: tickerProvider
      }
    ]}
    onconfirm={saveTicker}
    oncancel={() => (tickerItem = null)}
  />

  <AlertsModal
    bind:open={alertsOpen}
    item={alertItem}
    {alerts}
    preset={alertPreset}
    onchanged={() => load(true)}
  />

  <ItemEditModal bind:open={editOpen} item={editItem} {sections} onsaved={rowSaved} />
  <PromoteModal bind:open={promoteOpen} item={promoteItem} />
  <ListSettingsModal bind:open={settingsOpen} {wl} onsaved={(next) => (wl = next)} />
  <PromptModal
    bind:open={renameOpen}
    title={$t('watchlists.section.rename')}
    confirmLabel={$t('common.save')}
    fields={[{ key: 'name', label: $t('watchlists.section.renameHint'), value: renameFrom ?? '' }]}
    onconfirm={renameSection}
    oncancel={() => (renameFrom = null)}
  />

  <ConfirmModal
    bind:open={confirmDeleteList}
    title={$t('watchlists.detail.deleteAria')}
    message={$t('watchlists.detail.confirmDelete')}
    confirmLabel={$t('watchlists.detail.deleteAria')}
    cancelLabel={$t('common.cancel')}
    danger
    onconfirm={removeList}
  />

  <ConfirmModal
    bind:open={confirmRemoveOpen}
    title={$t('watchlists.table.removeAria')}
    message={$t('watchlists.table.confirmRemove', { symbol: pendingRemove?.symbol ?? '' })}
    confirmLabel={$t('watchlists.table.removeAria')}
    cancelLabel={$t('common.cancel')}
    danger
    onconfirm={confirmRemoveItem}
    oncancel={() => (pendingRemove = null)}
  />
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  /* First read of a list (and every list switch): a spinner centered in the pane. */
  .loader {
    min-height: 50vh;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .spinner {
    width: 28px;
    height: 28px;
    border: 3px solid var(--accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 600ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  /* PageHeader brings its own bottom margin; the column gap already spaces it. */
  .detail > :global(.page-header) {
    margin-bottom: 0;
  }
  .updated {
    font-size: 12px;
    color: var(--dim);
    white-space: nowrap;
  }
  .iconset {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .icon.lit {
    color: var(--accent);
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .desc {
    display: block;
    margin-top: var(--space-1);
    background: none;
    border: none;
    padding: 0;
    font-size: 13px;
    line-height: 1.4;
    color: var(--muted);
    cursor: text;
    text-align: left;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .desc:hover {
    color: var(--text);
  }
  /* The invite only shows on hover/focus so an empty list stays clean. */
  .desc.empty {
    color: var(--faint);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .detail:hover .desc.empty,
  .desc.empty:focus-visible {
    opacity: 1;
  }
  .descinput {
    display: block;
    margin-top: var(--space-1);
    height: 28px;
    width: 100%;
    max-width: 520px;
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
  .ratewarn {
    display: flex;
  }

  /* ── Control row ── */
  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    min-width: 0;
  }
  .views {
    display: inline-flex;
    gap: var(--space-1);
    margin-right: var(--space-2);
  }
  /* One frame: the input owns the hairline; the search glyph sits inside its padding. */
  .filterbox {
    position: relative;
    display: inline-flex;
    align-items: center;
    width: 168px;
    color: var(--dim);
  }
  .filterbox > :global(svg) {
    position: absolute;
    left: var(--space-2);
    pointer-events: none;
  }
  .filter {
    width: 100%;
    height: 28px;
    padding: 0 var(--space-2) 0 calc(var(--space-2) + 18px);
    font-size: 12px;
  }
  .sync {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 12px;
    color: var(--muted);
    cursor: pointer;
    white-space: nowrap;
    text-transform: none;
    letter-spacing: normal;
  }
  .interval {
    width: 108px;
    font-size: 12px;
  }
  .interval.source {
    width: 176px;
  }
  .interval :global(.dd .trigger) {
    height: 28px;
    font-size: 12px;
  }
  .flagchips {
    display: inline-flex;
    gap: 2px;
  }
  .flagchip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .flagchip:hover {
    background: var(--surface-hover);
  }
  .flagchip.on {
    border-color: var(--accent);
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }

  /* ── Upcoming strip ── */
  .strip {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    font-size: 12px;
  }
  .striplbl {
    flex: none;
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .striprow {
    display: flex;
    gap: var(--space-2);
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: thin;
    padding-bottom: 2px;
  }
  .striprow :global(.badge b) {
    color: var(--text);
    font-weight: 600;
  }
  .stripnote {
    color: var(--muted);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── The table ── */
  .tablewrap {
    overflow: auto;
    max-height: max(420px, calc(100vh - 260px));
    min-width: 0;
    scrollbar-width: thin;
  }
  .wtbl {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: 13px;
  }
  .wtbl th {
    position: sticky;
    top: 0;
    z-index: 2;
    height: 32px;
    padding: 0 var(--space-2);
    background: var(--bg);
    border-bottom: var(--hairline) solid var(--border);
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    text-align: left;
    white-space: nowrap;
  }
  .wtbl th.num {
    text-align: right;
  }
  .wtbl th.symth {
    padding-left: var(--space-4);
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 0;
    height: auto;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }
  .sort.numeric {
    flex-direction: row-reverse;
  }
  .sort:hover {
    color: var(--text);
  }
  .caret {
    display: inline-flex;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .sort:hover .caret,
  .caret.active {
    opacity: 1;
  }
  .caret.active {
    color: var(--text);
  }
  .sparkth {
    width: 104px;
  }
  .actth {
    width: 1%;
  }
  .wtbl td {
    height: 36px;
    padding: 0 var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    vertical-align: middle;
    white-space: nowrap;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .wtbl td.num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .wtbl tbody tr:not(.sechead):hover td {
    background: var(--surface-hover);
  }
  .wtbl tbody tr.sel td {
    background: var(--accent-soft);
  }
  .wtbl tbody tr.sel {
    outline: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    outline-offset: -1px;
  }
  .na {
    color: var(--muted);
    font-family: var(--font);
    font-size: 12px;
  }

  /* Symbol cell: grip, flag dot, symbol over name. */
  .symtd {
    position: relative;
    padding-left: var(--space-4);
  }
  .symcell {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  /* The grip lives in the cell's left padding: no column width spent on it. */
  .grip {
    position: absolute;
    left: 1px;
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    color: var(--faint);
    opacity: 0;
    cursor: grab;
    transition: opacity var(--dur-fast) var(--ease);
  }
  tr:hover .grip.on {
    opacity: 1;
  }
  .flagdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex: none;
  }
  .symtext {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .symrow {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .sym {
    font-size: 13px;
    font-weight: 600;
    line-height: 16px;
    color: var(--text);
  }
  .subtle {
    display: flex;
    align-items: baseline;
    gap: var(--space-1);
    font-size: 11px;
    line-height: 14px;
    color: var(--dim);
    max-width: 220px;
    min-width: 0;
  }
  .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ticker {
    flex: none;
  }
  .badge.evt {
    padding: 0 var(--space-1);
    font-size: 9.5px;
    line-height: 14px;
  }
  .newsb {
    height: 16px;
    padding: 0 var(--space-1);
    font-size: 10px;
    font-family: var(--mono);
    gap: 2px;
    cursor: pointer;
    background: transparent;
  }
  .newsb:hover {
    background: var(--surface-hover);
  }
  .ticker {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: auto;
    padding: 0;
    background: none;
    border: none;
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--dim);
    cursor: pointer;
    text-decoration: underline dotted;
    text-underline-offset: 2px;
  }
  .ticker:hover {
    color: var(--text);
  }
  .ticker :global(svg) {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  tr:hover .ticker :global(svg) {
    opacity: 1;
  }

  /* Price: the quote, and under it the virtual position P/L when one is set. */
  .price {
    font-weight: 500;
  }
  .pricecol {
    display: inline-flex;
    flex-direction: column;
    align-items: flex-end;
    line-height: 16px;
  }
  .pv {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .virt {
    display: inline-flex;
    gap: var(--space-1);
    font-weight: 400;
    line-height: 14px;
  }
  .virt :global(.delta.sm) {
    font-size: 11px;
  }
  .quoteerr {
    color: var(--amber);
    display: inline-flex;
  }
  .sparkcell {
    width: 104px;
  }
  .rangecell {
    width: 120px;
  }
  .rsi {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .rdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
  }

  /* Targets: buy and sell levels stacked, each with its distance from the price. */
  .tgts {
    display: inline-flex;
    flex-direction: column;
    align-items: flex-end;
  }
  .tgt {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 11px;
    line-height: 14px;
  }
  .tlbl {
    font-family: var(--font);
    font-size: 9.5px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .tv {
    color: var(--text);
  }
  .td {
    color: var(--muted);
    min-width: 46px;
    text-align: right;
  }
  .icon.xs {
    width: 16px;
    height: 16px;
  }
  .tbell {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  tr:hover .tbell,
  .tbell:focus-visible {
    opacity: 1;
  }

  /* Fixed-width column: a long note wraps to 2 lines then ellipsises (full text in the
     native tooltip), so neither the column nor the row height follows the content. */
  .wtbl td.notecell {
    width: 168px;
    max-width: 168px;
    white-space: normal;
  }
  .note {
    background: none;
    border: none;
    padding: 0;
    height: auto;
    font-size: 12px;
    line-height: 1.35;
    color: var(--muted);
    cursor: pointer;
    text-align: left;
    white-space: normal;
    width: 100%;
    max-height: 2.7em;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow-wrap: anywhere;
  }
  .note:hover {
    color: var(--text);
  }
  .addnote {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    width: auto;
    white-space: nowrap;
    max-height: none;
    font-family: var(--font);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .addnote:focus-visible,
  tr:hover .addnote {
    opacity: 1;
  }

  /* Row actions: icons that wake on hover; the bell stays lit once an alert is armed. */
  .actions {
    width: 1%;
    text-align: right;
    padding-right: var(--space-1);
  }
  .actions .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    vertical-align: middle;
  }
  .bell {
    position: relative;
    opacity: 0;
    transition:
      opacity var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  tr:hover .bell,
  .bell:focus-visible,
  .bell.armed {
    opacity: 1;
  }
  .bell.armed {
    color: var(--accent);
  }
  .count {
    position: absolute;
    top: 2px;
    right: 1px;
    font-size: 8px;
    font-weight: 600;
    line-height: 1;
    font-family: var(--mono);
  }
  .rowact {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  tr:hover .rowact,
  .rowact:focus-visible {
    opacity: 1;
  }
  .rowact.off {
    cursor: not-allowed;
    color: var(--faint);
  }

  /* Section header: a hairline band, not a card. */
  .sechead td {
    height: 30px;
    padding: var(--space-2) var(--space-2) 0;
    background: transparent;
    border-bottom: var(--hairline) solid var(--border);
  }
  .secline {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .secbtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: auto;
    background: none;
    border: none;
    padding: 0;
    color: var(--dim);
    cursor: pointer;
  }
  .secbtn:hover {
    color: var(--text);
  }
  .secname {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .seccount {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
    color: var(--faint);
  }
  .secedit {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .sechead:hover .secedit,
  .secedit:focus-visible {
    opacity: 1;
  }
  tr.dragging {
    opacity: 0.4;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  /* Touch screens have no hover: the row's hover-only affordances stay visible. */
  @media (hover: none) {
    .bell,
    .rowact,
    .addnote,
    .tbell,
    .secedit,
    .desc.empty,
    .ticker :global(svg) {
      opacity: 1;
    }
  }
  @media (max-width: 1099px) {
    .ratewarn :global(.badge) {
      white-space: normal;
      max-width: 100%;
    }
  }
  /* Narrow: each quote row becomes a record (symbol and price on top, the shown columns
     with their labels under), and the sortable headers become a chip row. */
  @media (max-width: 900px) {
    .detail :global(.page-header .actions) {
      flex-wrap: wrap;
    }
    .views {
      flex: 1 1 100%;
      margin-right: 0;
    }
    .filterbox {
      flex: 1 1 200px;
      width: auto;
      min-width: 0;
    }
    .filter {
      height: 44px;
    }
    .sync {
      min-height: 44px;
    }
    .interval,
    .interval.source {
      flex: 1 1 140px;
      width: auto;
      min-width: 0;
    }
    .spacer {
      display: none;
    }
    .desc {
      min-height: 32px;
      white-space: normal;
      overflow-wrap: anywhere;
    }
    .desc.empty {
      opacity: 1;
    }
    .tablewrap {
      overflow: visible;
      max-height: none;
    }
    .wtbl,
    .wtbl thead,
    .wtbl tbody {
      display: block;
    }
    .wtbl thead tr {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      padding-bottom: var(--space-2);
      border-bottom: var(--hairline) solid var(--border);
    }
    .wtbl thead th {
      position: static;
      display: block;
      height: auto;
      padding: 0;
      border: none;
      background: transparent;
    }
    .wtbl thead th:not(:has(.sort)) {
      display: none;
    }
    .sort,
    .sort.numeric {
      flex-direction: row;
      min-height: 36px;
      padding: 0 var(--space-3);
      border: var(--hairline) solid var(--border-control);
      border-radius: var(--radius-sm);
    }
    .caret {
      opacity: 1;
    }
    .wtbl tbody tr {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: var(--space-2) var(--space-3);
      padding: var(--space-3) 0;
      border-bottom: var(--hairline) solid var(--border);
    }
    .wtbl tbody tr.sechead {
      display: block;
      padding: var(--space-2) 0 0;
    }
    .wtbl tbody td,
    .sechead td {
      display: block;
      height: auto;
      min-width: 0;
      width: auto;
      max-width: none;
      padding: 0;
      border: none;
      white-space: normal;
    }
    .wtbl tbody td.symtd {
      grid-column: 1 / 3;
    }
    .wtbl tbody td.price {
      grid-column: 3 / -1;
    }
    .sparkcell {
      grid-column: 1 / -1;
    }
    .wtbl tbody td.num {
      text-align: left;
    }
    .wtbl tbody td[data-label]::before {
      content: attr(data-label);
      display: block;
      font-family: var(--font);
      font-size: 10px;
      font-weight: 600;
      color: var(--dim);
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }
    .wtbl tbody td.price::before {
      text-align: right;
    }
    .wtbl tbody td.price {
      text-align: right;
    }
    .grip {
      display: none;
    }
    .subtle {
      max-width: none;
    }
    .nm {
      white-space: normal;
    }
    .tgts {
      align-items: flex-start;
    }
    .wtbl td.notecell {
      grid-column: 1 / 4;
      width: auto;
      max-width: none;
    }
    .note {
      min-height: 32px;
      max-height: none;
      -webkit-line-clamp: 3;
      line-clamp: 3;
    }
    .addnote,
    .bell,
    .tbell,
    .rowact {
      opacity: 1;
    }
    .wtbl tbody td.actions {
      grid-column: 1 / -1;
      display: flex;
      align-items: center;
      justify-content: flex-end;
      gap: var(--space-1);
    }
    .actions .icon {
      width: 40px;
      height: 40px;
    }
  }
  @media (max-width: 420px) {
    .wtbl tbody tr {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .wtbl tbody td.symtd,
    .wtbl tbody td.price {
      grid-column: 1 / -1;
    }
    .wtbl tbody td.price {
      text-align: left;
    }
    .wtbl tbody td.price::before {
      text-align: left;
    }
    .wtbl td.notecell {
      grid-column: 1 / -1;
    }
  }
</style>
