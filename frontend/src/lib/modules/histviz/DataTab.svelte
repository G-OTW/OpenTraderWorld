<script>
  import Icon from '$lib/ui/Icon.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import { connectorsApi, allows, isReady, ALL_MODULES } from '$lib/connectors/api.js';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';
  // Data tab — one search box over the connectors the chart is allowed to use.
  //
  //   ▸ Type: the query goes to each ticked connector's own symbol lookup, so anything the
  //     provider serves is chartable, downloaded or not.
  //   ▸ Sources lists **every** connector you own; the tick is the grant itself — the
  //     server refuses a connector this module was never given, so a whitelist that only
  //     lived in the browser would have shown ticks that changed nothing.
  //
  // With no query the list is what you charted recently, or what is already stored —
  // both open with a single click and cost nothing.
  let {
    datasets = [],
    recents = [],
    selected = null,
    busy = false,
    onselect,
    ondownload
  } = $props();

  const PREFS = 'otw.histviz.data.v1';
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(PREFS) || '{}');
    } catch {
      return {};
    }
  })();

  /** Rows each shelf shows — a panel, not a catalogue: the module pages hold the full lists. */
  const SHELF_MAX = 10;

  let q = $state('');
  // Which shelf is open with no query. Recents first: the usual reason to come back here.
  let shelf = $state(saved.shelf === 'stored' ? 'stored' : 'recent');
  let connectors = $state([]); // every connector the user owns
  let dataModules = $state([]); // module ids a connector can be granted to
  let granting = $state(null); // connector id whose grant is being written
  let assetType = $state(saved.assetType ?? '');
  let sourcesOpen = $state(false);
  let results = $state([]);
  let notes = $state([]);
  let searching = $state(false);
  let searchError = $state('');
  let timer = null;

  /** Connectors that can actually answer: keyless, or with their credentials set. One that
   *  is still missing its key belongs in the connector manager, not in a source list. */
  const usable = $derived(connectors.filter(isReady));
  /** Of those, the ones ticked for this module — what a search really asks. */
  const active = $derived(usable.filter((c) => allows(c, 'histviz')));
  // Asset types offered = the union of what the connectors in play support.
  const assetTypes = $derived([...new Set(active.flatMap((c) => c.asset_types ?? []))].sort());

  async function loadConnectors() {
    try {
      const [all, mods] = await Promise.all([
        connectorsApi.list(),
        dataModules.length ? Promise.resolve(dataModules) : connectorsApi.modules()
      ]);
      connectors = all;
      dataModules = mods;
    } catch {
      /* stored datasets still work without the broker */
    }
  }
  $effect(() => {
    loadConnectors();
  });

  $effect(() => {
    const snap = { assetType, shelf };
    try {
      localStorage.setItem(PREFS, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  // Debounced lookup: the query, the ticked sources and the asset-type filter re-run it.
  $effect(() => {
    const needle = q.trim();
    const at = assetType;
    void active.length;
    if (timer) clearTimeout(timer);
    if (!needle) {
      results = [];
      notes = [];
      searching = false;
      searchError = '';
      return;
    }
    searching = true;
    timer = setTimeout(() => runSearch(needle, at), 250);
    return () => timer && clearTimeout(timer);
  });

  async function runSearch(needle, at) {
    try {
      const r = await histvizApi.symbols({ q: needle, asset_type: at });
      results = r.results ?? [];
      notes = r.notes ?? [];
      searchError = '';
    } catch (e) {
      results = [];
      notes = [];
      searchError = e.message;
    } finally {
      searching = false;
    }
  }

  /** Tick = grant this connector to the chart, untick = take it back. The grant lives on
   *  the server, so a wildcard has to be spelled out before one module can be removed. */
  async function toggleConnector(c) {
    if (granting) return;
    granting = c.id;
    const wildcard = c.modules?.includes(ALL_MODULES);
    const base = new Set(wildcard ? dataModules : (c.modules ?? []));
    if (allows(c, 'histviz')) base.delete('histviz');
    else base.add('histviz');
    // Everything back = the wildcard again, so future data modules stay covered.
    const next =
      dataModules.length && dataModules.every((m) => base.has(m)) ? [ALL_MODULES] : [...base];
    try {
      await connectorsApi.update(c.id, { modules: next });
      await loadConnectors();
    } catch (e) {
      searchError = e.message;
    } finally {
      granting = null;
    }
  }

  const sourceLabel = $derived(
    active.length === 0
      ? $t('histviz.data.noSources')
      : active.length === 1
        ? active[0].name
        : $t('histviz.data.nSources', { n: active.length })
  );

  // A search hit and a stored dataset both become the same chart coordinates.
  function pickHit(h) {
    onselect?.({
      connector_id: h.connector_id,
      provider: h.provider,
      asset_type: h.asset_type,
      ticker: h.symbol,
      name: h.name,
      timeframes: h.timeframes ?? [],
      stream: h.stream
    });
  }
  function pickStored(d) {
    const c = connectors.find((x) => x.provider === d.provider);
    onselect?.({
      connector_id: c?.id ?? null,
      provider: d.provider,
      asset_type: d.asset_type,
      ticker: d.ticker,
      timeframe: d.timeframe,
      timeframes: c?.timeframes ?? [],
      stream: c?.stream ?? false
    });
  }

  const isActive = (provider, ticker) =>
    selected?.provider === provider && selected?.ticker === ticker;

  // Stored datasets, one entry per instrument (timeframes are the toolbar's job).
  const storedInstruments = $derived.by(() => {
    const by = new Map();
    for (const d of datasets) {
      if (!d.bar_count) continue;
      const k = `${d.provider}|${d.asset_type}|${d.ticker}`;
      const cur = by.get(k);
      if (!cur) by.set(k, { ...d, bars: d.bar_count, tfs: [d.timeframe] });
      else {
        cur.bars += d.bar_count;
        if (!cur.tfs.includes(d.timeframe)) cur.tfs.push(d.timeframe);
      }
    }
    return [...by.values()].sort((a, b) => a.ticker.localeCompare(b.ticker));
  });

  const recentList = $derived(recents.slice(0, SHELF_MAX));
  const storedList = $derived(storedInstruments.slice(0, SHELF_MAX));
</script>

<div class="data">
  <!-- Search first and full width: finding an instrument is what this panel is for. The
       sources button sits under it, where it reads as a filter on the search. The input is
       a bare control — the theme gives it its border, nothing wraps it. -->
  <div class="qbox">
    <input
      class="q"
      type="search"
      bind:value={q}
      placeholder={$t('histviz.data.searchPlaceholder')}
      spellcheck="false"
      autocapitalize="characters"
    />
    {#if searching || busy}<span class="spin"><Icon name="refresh-cw" size={12} /></span>{/if}
  </div>

  <div class="filters">
    <div class="srcwrap">
      <button
        class="src"
        class:on={active.length > 0}
        title={$t('histviz.data.sourcesTitle')}
        onclick={() => (sourcesOpen = !sourcesOpen)}
      >
        <Icon name="database" size={12} />
        <span class="src-lbl">{sourceLabel}</span>
        <Icon name={sourcesOpen ? 'chevron-up' : 'chevron-down'} size={11} />
      </button>
      {#if sourcesOpen}
        <div class="srcmenu">
          <div class="srchead">
            <span>{$t('histviz.data.sources')}</span>
            <ConnectorButton module="histviz" size="sm" onchanged={loadConnectors} />
          </div>
          {#if !usable.length}
            <p class="muted">{$t('histviz.data.noConnectors')}</p>
          {:else}
            {#each usable as c (c.id)}
              <label class="srcrow">
                <input
                  type="checkbox"
                  checked={allows(c, 'histviz')}
                  disabled={granting === c.id}
                  onchange={() => toggleConnector(c)}
                />
                <span class="nm">{c.name}</span>
                <!-- The provider only when the account isn't already named after it. -->
                {#if c.label !== c.name}<span class="pl">{c.label}</span>{/if}
                {#if !c.searchable}<span class="tag" title={$t('histviz.data.noSearchHint')}
                    >{$t('histviz.data.noSearch')}</span
                  >{/if}
              </label>
            {/each}
          {/if}
        </div>
      {/if}
    </div>

    {#if assetTypes.length > 1}
      <select class="atype" bind:value={assetType} title={$t('histviz.data.assetType')}>
        <option value="">{$t('histviz.data.allTypes')}</option>
        {#each assetTypes as a (a)}<option value={a}>{a}</option>{/each}
      </select>
    {/if}
  </div>

  <!-- Two shelves, one at a time: what you were just looking at, and what you hold. -->
  {#if !q.trim()}
    <div class="shelf" role="tablist">
      <button
        role="tab"
        aria-selected={shelf === 'recent'}
        class:active={shelf === 'recent'}
        onclick={() => (shelf = 'recent')}>{$t('histviz.data.recent')}</button
      >
      <button
        role="tab"
        aria-selected={shelf === 'stored'}
        class:active={shelf === 'stored'}
        onclick={() => (shelf = 'stored')}>{$t('histviz.data.stored')}</button
      >
      <button class="dl" onclick={() => ondownload?.()} title={$t('histviz.picker.downloadTitle')}>
        <Icon name="download" size={12} />
      </button>
    </div>
  {/if}

  <div class="results">
    {#if q.trim()}
      {#if searchError}
        <p class="warn"><Icon name="alert-triangle" size={12} /> {searchError}</p>
      {/if}
      {#each notes as n (n.connector_id + n.code)}
        <p class="warn" title={n.message}>
          <Icon name="alert-triangle" size={12} />
          <span>{n.connector}: {n.message}</span>
        </p>
      {/each}
      {#if !results.length && !searching}
        <div class="placeholder">
          <Icon name="search" size={18} strokeWidth={1.7} />
          <span>{$t('histviz.data.noMatches')}</span>
        </div>
      {:else}
        {#each results as h (h.connector_id + h.symbol)}
          <button
            class="row"
            class:active={isActive(h.provider, h.symbol)}
            title={h.name || h.symbol}
            onclick={() => pickHit(h)}
          >
            <span class="line1">
              <span class="sym">{h.symbol}</span>
              {#if h.stored}<span class="has-data" title={$t('histviz.data.storedHint')}
                  ><Icon name="database" size={10} /></span
                >{/if}
              {#if h.stream}<span class="live-dot" title={$t('histviz.data.liveHint')}></span>{/if}
              <span class="prov">{h.provider}</span>
            </span>
          </button>
        {/each}
      {/if}
    {:else if shelf === 'recent'}
      {#if !recentList.length}
        <p class="muted">{$t('histviz.data.noRecent')}</p>
      {:else}
        {#each recentList as r (r.provider + r.asset_type + r.ticker)}
          <button
            class="row"
            class:active={isActive(r.provider, r.ticker)}
            onclick={() => onselect?.(r)}
          >
            <span class="line1">
              <span class="sym">{r.ticker}</span>
              <span class="prov">{r.provider}</span>
            </span>
          </button>
        {/each}
      {/if}
    {:else}
      {#if !storedList.length}
        <p class="muted">{$t('histviz.data.noStored')}</p>
      {:else}
        {#each storedList as d (d.provider + d.asset_type + d.ticker)}
          <button
            class="row"
            class:active={isActive(d.provider, d.ticker)}
            onclick={() => pickStored(d)}
          >
            <span class="line1">
              <span class="sym">{d.ticker}</span>
              <span class="prov">{d.provider}</span>
            </span>
            <span class="line2">
              {d.tfs.join(' ')}
              <span class="dot">·</span>
              {$t('histviz.data.barsN', { bars: fmtNum(d.bars, 0) })}
            </span>
          </button>
        {/each}
      {/if}
    {/if}
  </div>
</div>

<style>
  .data {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
    flex: 1;
  }
  /* Only the result list scrolls; everything above it keeps its natural height. */
  .filters {
    display: flex;
    align-items: stretch;
    gap: var(--space-1);
    flex: none;
  }
  .srcwrap {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  .src {
    display: flex;
    width: 100%;
    align-items: center;
    gap: var(--space-2);
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .src:hover,
  .src.on {
    color: var(--text);
  }
  .src.on {
    border-color: var(--accent);
  }
  .src-lbl {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Anchored under the button; the aside clips nothing because the popover is absolute. */
  .srcmenu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: var(--z-dropdown);
    right: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2);
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
  }
  .srchead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .srcrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--text);
    cursor: pointer;
  }
  .srcrow .pl,
  .srcrow .tag {
    color: var(--muted);
  }
  .srcrow .nm {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .srcrow .tag {
    margin-left: auto;
    font-size: var(--text-xs);
    opacity: 0.8;
  }
  /* The spinner rides *inside* the input's own box — the box is the input, nothing is
     drawn around it. */
  .qbox {
    position: relative;
    flex: none;
  }
  .q {
    width: 100%;
  }
  .spin {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    color: var(--muted);
    pointer-events: none;
    right: var(--space-3);
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: translateY(-50%) rotate(360deg);
    }
  }
  .atype {
    flex: none;
    max-width: 40%;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  /* Shelf toggle: the same underlined tab language as the aside's own tabs, one rung
     quieter, so it reads as a filter inside the panel rather than a second navigation. */
  .shelf {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .shelf button {
    padding: 0 0 var(--space-1);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
    color: var(--muted);
    cursor: pointer;
  }
  .shelf button:hover {
    color: var(--text);
  }
  .shelf button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .shelf .dl {
    margin-left: auto;
    display: inline-flex;
    padding-bottom: var(--space-1);
  }
  .results {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .dl {
    background: none;
    border: none;
    padding: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .dl:hover {
    color: var(--text);
  }
  .row {
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    cursor: pointer;
    transition: border-color 0.12s ease;
  }
  .row:hover {
    border-color: var(--border-control);
  }
  .row.active {
    border-color: var(--accent);
  }
  .line1 {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .sym {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
    color: var(--text);
  }
  /* Ticker then provider: the two facts that identify a series. */
  .prov {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .line2 {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    opacity: 0.5;
  }
  /* Deliberately not called `.chip`: the theme owns that class globally (padded pill),
     and a scoped rule of the same name inherits its padding. */
  .has-data {
    display: inline-flex;
    flex: none;
    color: var(--muted);
  }
  .live-dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--red);
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--amber);
    line-height: 1.4;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-xs);
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
    font-size: var(--text-xs);
    text-align: center;
  }
  .placeholder :global(.icon-svg) {
    opacity: 0.55;
  }
</style>
