<script>
  // One watchlist's working area: header (rename/delete, auto-sync controls, manual refresh,
  // last-updated), an add-symbol + filter toolbar, and the quotes table (sparkline, price,
  // 24h/3d/7d/30d changes, per-symbol note). Quotes are read from the server cache; "Refresh"
  // re-quotes server-side. While auto-sync is on, the table quietly re-reads the cache so the
  // background loop's updates appear without a manual reload.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import Sparkline from './Sparkline.svelte';
  import AddSymbolModal from './AddSymbolModal.svelte';
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
  import { t } from '$lib/i18n';

  let { id, onchanged = () => {}, ondeleted = () => {} } = $props();

  let wl = $state(null);
  let items = $state([]);
  let loading = $state(false);
  let error = $state('');
  let refreshing = $state(false);

  let showAdd = $state(false);
  let showEdit = $state(false);
  let confirmDeleteList = $state(false);
  let pendingRemove = $state(null); // item awaiting the remove confirm
  let confirmRemoveOpen = $state(false);
  let noteItem = $state(null); // item whose note is being edited
  let noteOpen = $state(false);
  let tickerItem = $state(null); // item whose provider ticker is being edited
  let tickerOpen = $state(false);

  // Historical Data connectors, offered as custom quote sources. Loaded once, lazily; an
  // empty list (module not installed / no connectors) just leaves the select on Auto.
  let connectors = $state([]);
  $effect(() => {
    watchlistsApi
      .connectors()
      .then((c) => (connectors = c ?? []))
      .catch(() => {});
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
    } catch (e) {
      if (!quiet) error = e.message;
    } finally {
      loading = false;
    }
  }

  // Reload whenever the selected list changes.
  $effect(() => {
    id;
    wl = null;
    filter = '';
    sortKey = null;
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
    c30: (i) => i.quote?.change_30d
  };

  let visible = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    let out = q
      ? items.filter(
          (i) => i.symbol.toLowerCase().includes(q) || i.name.toLowerCase().includes(q)
        )
      : [...items];
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

  // Source picker options for the per-item modal.
  let sourceOptions = $derived([
    { value: '', label: $t('watchlists.table.sourceDefault') },
    { value: 'auto', label: $t('watchlists.detail.providerAuto') },
    ...connectors.map((c) => ({ value: c.id, label: `${c.label} · ${c.name}` }))
  ]);

  let updatedAgo = $derived.by(() => {
    now; // re-derive on the tick
    return wl?.refreshed_at ? agoLabel(wl.refreshed_at) : null;
  });

  const COLS = [
    { key: 'c24', labelKey: 'watchlists.table.h24', field: 'change_24h' },
    { key: 'c3', labelKey: 'watchlists.table.d3', field: 'change_3d' },
    { key: 'c7', labelKey: 'watchlists.table.d7', field: 'change_7d' },
    { key: 'c30', labelKey: 'watchlists.table.d30', field: 'change_30d' }
  ];
</script>

{#if loading && !wl}
  <Skeleton height="320px" />
{:else if wl}
  <div class="detail">
    <header>
      <div class="titlerow">
        <h2>{wl.name}</h2>
        <button class="iconbtn" onclick={() => (showEdit = true)} aria-label={$t('watchlists.detail.editAria')}>
          <Icon name="pencil" size={14} />
        </button>
        <button class="iconbtn danger" onclick={() => (confirmDeleteList = true)} aria-label={$t('watchlists.detail.deleteAria')}>
          <Icon name="trash" size={14} />
        </button>
        <span class="spacer"></span>
        <span class="updated">
          {#if refreshing}
            {$t('watchlists.detail.refreshing')}
          {:else if updatedAgo}
            {$t('watchlists.detail.updated', { ago: updatedAgo })}
          {:else}
            {$t('watchlists.detail.neverRefreshed')}
          {/if}
        </span>
        <Button icon="refresh-cw" size="sm" loading={refreshing} onclick={refreshNow}>
          {$t('watchlists.detail.refresh')}
        </Button>
      </div>
      {#if wl.description}<p class="desc">{wl.description}</p>{/if}

      <div class="toolbar">
        <Button variant="primary" icon="plus" size="sm" onclick={() => (showAdd = true)}>
          {$t('watchlists.detail.addSymbol')}
        </Button>
        <input
          class="filter"
          type="search"
          placeholder={$t('watchlists.detail.filterPlaceholder')}
          bind:value={filter}
        />
        <span class="spacer"></span>
        <label class="sync">
          <input
            type="checkbox"
            checked={wl.sync_enabled}
            onchange={(e) => patchList({ sync_enabled: e.currentTarget.checked })}
          />
          {$t('watchlists.detail.sync')}
        </label>
        <select
          class="interval"
          disabled={!wl.sync_enabled}
          value={wl.refresh_secs}
          onchange={(e) => patchList({ refresh_secs: Number(e.currentTarget.value) })}
        >
          {#each intervalOptions as o (o.secs)}
            <option value={o.secs}>{$t(o.key)}</option>
          {/each}
        </select>
        {#if connectors.length > 0}
          <select
            class="interval"
            title={$t('watchlists.detail.provider')}
            value={wl.connector_id ?? ''}
            onchange={(e) => patchList({ connector_id: e.currentTarget.value })}
          >
            <option value="">{$t('watchlists.detail.providerAuto')}</option>
            {#each connectors as c (c.id)}
              <option value={c.id}>{c.label} · {c.name}</option>
            {/each}
          </select>
        {/if}
      </div>
      {#if customInPlay}
        <div class="ratewarn">
          <Badge tone="warn" icon="alert-triangle">{$t('watchlists.detail.customProviderWarn')}</Badge>
        </div>
      {:else if rateRisk}
        <div class="ratewarn">
          <Badge tone="warn" icon="alert-triangle">{$t('watchlists.detail.rateWarning')}</Badge>
        </div>
      {/if}
    </header>

    <ErrorText error={error} copyable />

    {#if items.length === 0}
      <EmptyState icon="star" description={$t('watchlists.detail.emptyItems')}>
        {#snippet action()}
          <Button variant="primary" icon="plus" onclick={() => (showAdd = true)}>
            {$t('watchlists.detail.addSymbol')}
          </Button>
        {/snippet}
      </EmptyState>
    {:else}
      <div class="tablewrap">
        <table class="tbl">
          <thead>
            <tr>
              <th>
                <button class="sort" onclick={() => toggleSort('symbol')}>
                  {$t('watchlists.table.symbol')}
                  {#if sortKey === 'symbol'}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}
                </button>
              </th>
              <th class="num">
                <button class="sort num" onclick={() => toggleSort('price')}>
                  {$t('watchlists.table.price')}
                  {#if sortKey === 'price'}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}
                </button>
              </th>
              {#each COLS as c (c.key)}
                <th class="num">
                  <button class="sort num" onclick={() => toggleSort(c.key)}>
                    {$t(c.labelKey)}
                    {#if sortKey === c.key}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}
                  </button>
                </th>
              {/each}
              <th class="sparkth">{$t('watchlists.table.trend')}</th>
              <th>{$t('watchlists.table.notes')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each visible as item (item.id)}
              <tr>
                <td>
                  <div class="symcell">
                    <span class="sym">{item.symbol}</span>
                    <span class="subtle">
                      {item.name}{#if item.exchange}&nbsp;· {item.exchange}{:else if item.asset_class === 'crypto'}&nbsp;· crypto{/if}
                    </span>
                    {#if connectors.length > 0}
                      {@const rc = resolvedConnector(item)}
                      <button class="note ticker" onclick={() => editTicker(item)} title={$t('watchlists.table.tickerTitle', { symbol: item.symbol })}>
                        {#if rc}{rc.label} · {item.quote_ticker || $t('watchlists.table.tickerAuto')}{:else}{$t('watchlists.table.tickerAuto')}{/if}
                        <Icon name="pencil" size={10} />
                      </button>
                    {/if}
                  </div>
                </td>
                <td class="num price" title={item.quoted_at ? new Date(item.quoted_at).toLocaleString() : ''}>
                  {#if item.quote?.error}
                    <span class="quoteerr" title={item.quote.error}><Icon name="alert-triangle" size={12} /></span>
                  {/if}
                  {fmtQuote(item.quote?.price_usd)}
                </td>
                {#each COLS as c (c.key)}
                  {@const v = item.quote?.[c.field]}
                  <td class="num" class:up={v > 0} class:down={v < 0}>{fmtSignedPct(v)}</td>
                {/each}
                <td class="sparkcell"><Sparkline values={item.quote?.spark ?? []} /></td>
                <td class="notecell">
                  {#if item.notes}
                    <button class="note" onclick={() => editNote(item)} title={item.notes}>{item.notes}</button>
                  {:else}
                    <button class="note addnote" onclick={() => editNote(item)}>
                      <Icon name="plus" size={11} /> {$t('watchlists.table.addNote')}
                    </button>
                  {/if}
                </td>
                <td class="actions">
                  <button class="iconbtn danger" onclick={() => ((pendingRemove = item), (confirmRemoveOpen = true))} aria-label={$t('watchlists.table.removeAria')}>
                    <Icon name="x" size={13} />
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

  <AddSymbolModal
    bind:open={showAdd}
    watchlistId={id}
    onadded={() => (load(true), onchanged())}
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
        placeholder: $t('watchlists.table.tickerAuto')
      }
    ]}
    onconfirm={saveTicker}
    oncancel={() => (tickerItem = null)}
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
  header {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .titlerow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  h2 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
    line-height: var(--lh-tight);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .desc {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .spacer {
    flex: 1;
  }
  .updated {
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .filter {
    width: 200px;
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
  }
  .filter::placeholder {
    color: var(--dim);
  }
  .sync {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
    white-space: nowrap;
  }
  .interval {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
  }
  .interval:disabled {
    opacity: 0.5;
  }
  .ratewarn {
    display: flex;
  }

  .tablewrap {
    overflow-x: auto;
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
  th.num .sort {
    justify-content: flex-end;
    width: 100%;
  }
  .symcell {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    padding: var(--space-1) 0;
  }
  .sym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
  }
  .subtle {
    font-size: var(--text-xs);
    color: var(--muted);
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .price {
    font-weight: var(--fw-medium);
  }
  .sparkth {
    width: 130px;
  }
  .sparkcell {
    padding-top: var(--space-1);
    padding-bottom: var(--space-1);
  }
  .notecell {
    max-width: 200px;
  }
  .note {
    background: none;
    border: none;
    padding: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    cursor: pointer;
    text-align: left;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .note:hover {
    color: var(--text);
  }
  .addnote {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  tr:hover .addnote {
    opacity: 1;
  }
  .actions {
    width: 32px;
    text-align: right;
  }
  .iconbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: 0;
  }
  .iconbtn:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .iconbtn.danger:hover {
    color: var(--red);
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .ticker {
    font-family: var(--mono);
    text-decoration: underline dotted;
    text-underline-offset: 3px;
  }
  .ticker :global(svg) {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  tr:hover .ticker :global(svg) {
    opacity: 1;
  }
  .quoteerr {
    color: var(--amber);
    display: inline-flex;
    vertical-align: middle;
    margin-right: 2px;
  }
</style>
