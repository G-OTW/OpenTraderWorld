<script module>
  /** The connector list outlives the dialog: reopening it shows the side list at once
      instead of popping it in when the fetch lands. */
  let connectorCache = null;
</script>

<script>
  // The chart's instrument picker. One search box over the connectors the chart may use; a
  // side list with the shelves shown under an empty query (recent, favorites, stored), the
  // asset classes that filter the search and the sources it asks; results grouped by asset
  // class. Arrows move through the list, Enter opens the row, the star keeps an instrument
  // in Favorites.
  import Icon from '$lib/ui/Icon.svelte';
  import SymbolHelp from '$lib/ui/SymbolHelp.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import { connectorsApi, allows, isReady } from '$lib/connectors/api.js';
  import { histvizApi } from './api.js';
  import { t } from '$lib/i18n';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { fmtNum } from '$lib/format.js';
  import { getPref, setPref } from '$lib/prefs.svelte.js';
  import { instrumentKeyOf, loadFavorites, toggleFavorite, onFavorites } from './pane.js';

  let {
    datasets = [],
    recents = [],
    selected = null,
    busy = false,
    onselect,
    ondownload = null,
    onimport = null
  } = $props();

  const PREFS = 'otw.histviz.data.v1';
  const saved = (() => {
    const v = getPref(PREFS, {});
    return v && typeof v === 'object' ? v : {};
  })();
  const SHELF_MAX = 12;
  const CLASS_ORDER = ['crypto', 'future', 'equity', 'etf', 'index', 'fx', 'option'];

  let q = $state('');
  let shelf = $state(['recent', 'favorites', 'stored'].includes(saved.shelf) ? saved.shelf : 'recent');
  let connectors = $state(connectorCache ?? []);
  let picked = $state(Array.isArray(saved.sources) ? saved.sources : null);
  let assetType = $state(saved.assetType ?? '');
  let sourcesOpen = $state(false);
  let results = $state([]);
  let notes = $state([]);
  let searching = $state(false);
  let searchError = $state('');
  let favorites = $state(loadFavorites());
  /** The row the arrows point at, in the flat list. */
  let cur = $state(0);
  let listEl = $state(null);
  let timer = null;

  $effect(() => onFavorites((l) => (favorites = l)));

  const usable = $derived(connectors.filter((c) => isReady(c) && allows(c, 'histviz')));
  const active = $derived(usable.filter((c) => picked === null || picked.includes(c.id)));
  const assetTypes = $derived(
    [...new Set([...active.flatMap((c) => c.asset_types ?? []), ...(assetType ? [assetType] : [])])].sort(
      (a, b) => rank(a) - rank(b)
    )
  );
  const rank = (a) => {
    const i = CLASS_ORDER.indexOf(a);
    return i < 0 ? 99 : i;
  };

  async function loadConnectors() {
    try {
      connectors = connectorCache = await connectorsApi.list();
    } catch {
      /* stored datasets still work without the broker */
    }
  }
  $effect(() => {
    loadConnectors();
  });

  let snapRead = false;
  $effect(() => {
    const snap = { assetType, shelf, sources: $state.snapshot(picked) };
    if (snapRead) setPref(PREFS, snap);
    snapRead = true;
  });

  $effect(() => {
    const needle = q.trim();
    const at = assetType;
    const ids = active.map((c) => c.id);
    if (timer) clearTimeout(timer);
    if (!needle || !ids.length) {
      results = [];
      notes = [];
      searching = false;
      searchError = '';
      return;
    }
    searching = true;
    timer = setTimeout(() => runSearch(needle, at, ids), 250);
    return () => timer && clearTimeout(timer);
  });

  async function runSearch(needle, at, ids) {
    try {
      const r = await histvizApi.symbols({ q: needle, asset_type: at, connectors: ids, module: 'histviz' });
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

  function toggleConnector(c) {
    const on = new Set(picked ?? usable.map((u) => u.id));
    if (on.has(c.id)) on.delete(c.id);
    else on.add(c.id);
    picked = usable.every((u) => on.has(u.id)) ? null : usable.filter((u) => on.has(u.id)).map((u) => u.id);
  }
  const isOn = (c) => picked === null || picked.includes(c.id);

  const sourceLabel = $derived(
    active.length === 0
      ? $t('histviz.data.noSources')
      : active.length === usable.length
        ? $t('histviz.data.allSources')
        : active.length === 1
          ? active[0].name
          : $t('histviz.data.nSources', { n: active.length })
  );

  function sourceFor(provider, named) {
    const ticked = active.filter((c) => c.provider === provider);
    if (named && ticked.some((c) => c.id === named)) return named;
    return ticked[0]?.id ?? usable.find((c) => c.provider === provider)?.id ?? named ?? null;
  }
  const connectorOf = (id, provider) =>
    connectors.find((c) => c.id === id) ?? connectors.find((c) => c.provider === provider) ?? null;

  const storedInstruments = $derived.by(() => {
    const by = new Map();
    for (const d of datasets) {
      if (!d.bar_count) continue;
      const k = `${d.provider}|${d.asset_type}|${d.ticker}`;
      const c = by.get(k);
      if (!c) by.set(k, { ...d, bars: d.bar_count, tfs: [d.timeframe] });
      else {
        c.bars += d.bar_count;
        if (!c.tfs.includes(d.timeframe)) c.tfs.push(d.timeframe);
      }
    }
    return [...by.values()].sort((a, b) => a.ticker.localeCompare(b.ticker));
  });

  /** Every row, whatever it came from, as one shape: what to show and what to open. */
  function fromHit(h) {
    return {
      key: `${h.connector_id}|${h.asset_type}|${h.symbol}`,
      ticker: h.symbol,
      name: h.name,
      asset_type: h.asset_type,
      exchange: h.exchange,
      provider: h.provider,
      source: h.connector || h.label || h.provider,
      sourceOn: true,
      stored: h.stored,
      coords: {
        connector_id: h.connector_id,
        provider: h.provider,
        asset_type: h.asset_type,
        ticker: h.symbol,
        name: h.name,
        timeframes: h.timeframes ?? [],
        stream: h.stream,
        stream_timeframes: h.stream_timeframes ?? [],
        stream_note: h.stream_note ?? ''
      }
    };
  }
  function fromCoords(r) {
    const id = sourceFor(r.provider, r.connector_id);
    const c = connectorOf(id, r.provider);
    return {
      key: instrumentKeyOf(r),
      ticker: r.ticker,
      name: r.name ?? '',
      asset_type: r.asset_type,
      exchange: r.exchange ?? '',
      provider: r.provider,
      source: c?.name ?? r.provider,
      sourceOn: !!c && active.some((a) => a.id === c.id),
      coords: { ...r, connector_id: id }
    };
  }
  function fromStored(d) {
    const id = sourceFor(d.provider, null);
    const c = connectorOf(id, d.provider);
    return {
      key: instrumentKeyOf(d),
      ticker: d.ticker,
      name: d.name ?? '',
      asset_type: d.asset_type,
      exchange: '',
      provider: d.provider,
      source: c?.name ?? d.provider,
      sourceOn: !!c && active.some((a) => a.id === c.id),
      detail: `${d.tfs.join(' ')} · ${$t('histviz.data.barsN', { bars: fmtNum(d.bars, 0) })}`,
      coords: {
        connector_id: id,
        provider: d.provider,
        asset_type: d.asset_type,
        ticker: d.ticker,
        timeframe: d.timeframe,
        timeframes: c?.timeframes ?? [],
        stream: (c?.stream_asset_types ?? []).includes(d.asset_type),
        stream_timeframes: c?.stream_timeframes ?? [],
        stream_note: c?.stream_note ?? ''
      }
    };
  }

  const rows = $derived.by(() => {
    if (q.trim()) return results.map(fromHit);
    const byClass = (r) => !assetType || r.asset_type === assetType;
    if (shelf === 'favorites') return favorites.map(fromCoords).filter(byClass);
    if (shelf === 'stored') return storedInstruments.slice(0, 200).map(fromStored).filter(byClass);
    return recents.slice(0, SHELF_MAX).map(fromCoords).filter(byClass);
  });
  /** Rows by asset class, in a fixed order; the flat order is the arrows' order. */
  const groups = $derived.by(() => {
    const by = new Map();
    for (const r of rows) {
      const k = r.asset_type || 'other';
      if (!by.has(k)) by.set(k, []);
      by.get(k).push(r);
    }
    return [...by.entries()].sort((a, b) => rank(a[0]) - rank(b[0]));
  });
  const flat = $derived(groups.flatMap(([, list]) => list));
  const indexOf = $derived(new Map(flat.map((r, i) => [r.key, i])));

  $effect(() => {
    void flat.length;
    void q;
    cur = 0;
  });

  const favKeys = $derived(new Set(favorites.map(instrumentKeyOf)));
  const isFav = (r) => favKeys.has(instrumentKeyOf(r.coords));
  const isSelected = (r) => selected?.provider === r.provider && selected?.ticker === r.ticker;

  function open(r) {
    if (r) onselect?.(r.coords);
  }
  function star(r, e) {
    e?.stopPropagation();
    favorites = toggleFavorite(r.coords);
  }
  /** Hover follows the pointer only when it really moves: a keyboard scroll slides rows under
      a still pointer, and taking that as a hover sent the highlight back and forth. */
  let lastPt = '';
  function hover(e, i) {
    const pt = `${e.clientX},${e.clientY}`;
    if (pt === lastPt) return;
    lastPt = pt;
    if (cur !== i) cur = i;
  }
  function move(d) {
    if (!flat.length) return;
    cur = (cur + d + flat.length) % flat.length;
    queueMicrotask(() => listEl?.querySelector(`[data-i="${cur}"]`)?.scrollIntoView({ block: 'nearest' }));
  }
  function onkey(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      move(-1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      sourcesOpen = false;
      open(flat[cur]);
    } else if (e.key === 'Escape') {
      sourcesOpen = false;
    }
  }

  /** Avatar letters: the base of a pair (BTCUSDT → BTC), else the ticker's head. */
  function initials(r) {
    const s = String(r.ticker ?? '').toUpperCase().replace(/[^A-Z0-9]/g, '');
    const base = r.asset_type === 'crypto' ? s.replace(/(USDT|USDC|BUSD|FDUSD|USD|EUR|BTC|ETH)$/, '') || s : s;
    return base.slice(0, 3);
  }
  const TONES = ['accent', 'amber', 'blue', 'violet', 'teal'];
  function tone(r) {
    let h = 0;
    for (const ch of initials(r)) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return TONES[h % TONES.length];
  }
  const classLabel = (a) => (a === 'other' ? $t('histviz.data.class.other') : $t(`histviz.data.class.${a}`));

  const shelves = $derived([
    { key: 'recent', icon: 'history', n: Math.min(recents.length, SHELF_MAX) },
    { key: 'favorites', icon: 'star', n: favorites.length },
    { key: 'stored', icon: 'database', n: storedInstruments.length }
  ]);
</script>

<div class="ip">
  <!-- One frame: the sources, the field, the tools. -->
  <div class="sbar">
    <Icon name="search" size={16} />
    <div class="srcwrap" use:clickOutside={() => (sourcesOpen = false)}>
      <button type="button" class="chip" class:open={sourcesOpen} title={$t('histviz.data.sourcesTitle')} onclick={() => (sourcesOpen = !sourcesOpen)}>
        <Icon name="database" size={13} />
        <span>{sourceLabel}</span>
        <Icon name={sourcesOpen ? 'chevron-up' : 'chevron-down'} size={12} />
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
                <input type="checkbox" checked={isOn(c)} onchange={() => toggleConnector(c)} />
                <span class="nm">{c.name}</span>
                {#if c.label !== c.name}<span class="pl">{c.label}</span>{/if}
                {#if !c.searchable}<span class="tag" title={$t('histviz.data.noSearchHint')}>{$t('histviz.data.noSearch')}</span>{/if}
              </label>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
    <span class="vsep" aria-hidden="true"></span>
    <input
      class="q"
      type="search"
      bind:value={q}
      onkeydown={onkey}
      placeholder={$t('histviz.data.searchPlaceholder')}
      aria-label={$t('histviz.data.searchPlaceholder')}
      spellcheck="false"
      autocapitalize="characters"
    />
    {#if searching || busy}<span class="spin"><Icon name="refresh-cw" size={13} /></span>{/if}
    <SymbolHelp provider={active.length === 1 ? active[0].provider : ''} />
    {#if onimport}
      <button type="button" class="tool" onclick={() => onimport()} title={$t('histviz.picker.importTitle')} aria-label={$t('histviz.picker.importTitle')}>
        <Icon name="upload" size={14} />
      </button>
    {/if}
    {#if ondownload}
      <button type="button" class="tool" onclick={() => ondownload()} title={$t('histviz.picker.downloadTitle')} aria-label={$t('histviz.picker.downloadTitle')}>
        <Icon name="download" size={14} />
      </button>
    {/if}
  </div>

  <div class="main">
    <nav class="side">
      {#each shelves as s (s.key)}
        <button
          type="button"
          class="nav"
          class:on={!q.trim() && shelf === s.key}
          onclick={() => {
            shelf = s.key;
            q = '';
          }}
        >
          <Icon name={s.icon} size={14} />
          <span class="nl">{$t(`histviz.data.${s.key}`)}</span>
          <span class="nn">{s.n}</span>
        </button>
      {/each}

      {#if assetTypes.length}
        <p class="sh">{$t('histviz.data.assetClass')}</p>
        {#each assetTypes as a (a)}
          <button type="button" class="nav" class:on={assetType === a} aria-pressed={assetType === a} onclick={() => (assetType = assetType === a ? '' : a)}>
            <span class="box" class:ticked={assetType === a}></span>
            <span class="nl">{classLabel(a)}</span>
          </button>
        {/each}
      {/if}

      {#if usable.length}
        <p class="sh">{$t('histviz.data.sources')}</p>
        {#each usable as c (c.id)}
          <button
            type="button"
            class="nav src"
            aria-pressed={isOn(c)}
            title={c.searchable ? c.label : $t('histviz.data.noSearchHint')}
            onclick={() => toggleConnector(c)}
          >
            <span class="sdot" class:on={isOn(c)} class:warn={isOn(c) && !c.searchable}></span>
            <span class="nl" class:off={!isOn(c)}>{c.name}</span>
          </button>
        {/each}
      {/if}
    </nav>

    <div class="list" bind:this={listEl}>
      {#if q.trim()}
        {#if searchError}
          <p class="warn"><Icon name="alert-triangle" size={12} /> {searchError}</p>
        {/if}
        {#each notes as n (n.connector_id + n.code)}
          <p class="warn" title={n.message}><Icon name="alert-triangle" size={12} /> <span>{n.connector}: {n.message}</span></p>
        {/each}
      {/if}

      {#each groups as [cls, list] (cls)}
        <div class="gh"><span>{classLabel(cls)}</span><span class="gn">{list.length}</span><span class="gl"></span></div>
        {#each list as r (r.key)}
          {@const i = indexOf.get(r.key)}
          <div
            class="row"
            class:cur={i === cur}
            class:sel={isSelected(r)}
            data-i={i}
            role="button"
            tabindex="-1"
            onpointermove={(e) => hover(e, i)}
            onclick={() => open(r)}
            onkeydown={(e) => e.key === 'Enter' && open(r)}
          >
            <span class="av t-{tone(r)}">{initials(r)}</span>
            <span class="txt">
              <span class="l1">
                <span class="sym">{r.ticker}</span>
                {#if r.exchange}<span class="tag">{r.exchange}</span>{/if}
                {#if r.stored}<span class="has" title={$t('histviz.data.storedHint')}><Icon name="database" size={11} /></span>{/if}
                {#if isFav(r)}<span class="fav"><Icon name="star" size={12} /></span>{/if}
              </span>
              {#if r.name || r.detail}<span class="l2">{r.detail ?? r.name}</span>{/if}
            </span>
            <!-- Both faces stay laid out in one cell, so the row keeps its width on hover. -->
            <span class="end" class:hot={i === cur}>
              <span class="face idle">
                <span class="sdot" class:on={r.sourceOn}></span>
                <span class="srcn">{r.source}</span>
              </span>
              <span class="face act-face">
                <button type="button" class="act" class:faved={isFav(r)} tabindex="-1" title={$t('histviz.data.favToggle')} aria-label={$t('histviz.data.favToggle')} onclick={(e) => star(r, e)}>
                  <Icon name="star" size={14} />
                </button>
                <span class="openhint"><span class="kbd">↵</span>{$t('histviz.data.open')}</span>
              </span>
            </span>
          </div>
        {/each}
      {:else}
        {#if !(q.trim() && searching)}
          <div class="empty">
            <Icon name={q.trim() ? 'search' : shelf === 'favorites' ? 'star' : 'history'} size={18} strokeWidth={1.7} />
            <span>
              {q.trim()
                ? $t('histviz.data.noMatches')
                : shelf === 'favorites'
                  ? $t('histviz.data.noFavorites')
                  : shelf === 'stored'
                    ? $t('histviz.data.noStored')
                    : $t('histviz.data.noRecent')}
            </span>
          </div>
        {/if}
      {/each}
    </div>
  </div>

  <div class="foot">
    <span><span class="kbd">↑↓</span>{$t('histviz.data.navigate')}</span>
    <span><span class="kbd">↵</span>{$t('histviz.data.open')}</span>
    <span class="cnt">{$t('histviz.data.nResults', { n: flat.length })}</span>
  </div>
</div>

<style>
  .ip {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    flex: 1;
    min-height: 0;
  }
  .sbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    height: 48px;
    padding: 0 var(--space-2) 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg, var(--surface));
    color: var(--muted);
  }
  .sbar:focus-within {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .srcwrap {
    position: relative;
    flex: none;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .chip :global(svg) {
    color: var(--muted);
  }
  .chip:hover,
  .chip.open {
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
  }
  .srcmenu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 5;
    min-width: 260px;
    padding: var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.32);
  }
  .srchead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .srcrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-2);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .srcrow:hover {
    background: var(--surface-2);
  }
  .srcrow .pl {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .srcrow .tag {
    margin-left: auto;
  }
  .vsep {
    flex: none;
    width: 1px;
    height: 20px;
    background: var(--border);
  }
  .q {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    color: var(--text);
    font-size: var(--text-base);
  }
  .q:focus {
    border: none;
    box-shadow: none;
  }
  .spin {
    display: inline-flex;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .tool {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .tool:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .main {
    display: grid;
    grid-template-columns: 180px 1fr;
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--border);
    margin: 0 calc(-1 * var(--space-4));
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-3) var(--space-2) var(--space-3) var(--space-3);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 32px;
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .nav :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .nav:hover {
    background: var(--surface-2);
  }
  .nav.on {
    background: var(--surface-2);
  }
  .nav.on :global(svg) {
    color: var(--accent);
  }
  .nl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .nl.off {
    color: var(--muted);
  }
  .nn {
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .sh {
    margin: var(--space-3) var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .box {
    flex: none;
    width: 12px;
    height: 12px;
    border: 1.5px solid var(--muted);
    border-radius: 3px;
  }
  .box.ticked {
    border-color: var(--accent);
    background: var(--accent);
    box-shadow: inset 0 0 0 2px var(--surface);
  }
  .sdot {
    flex: none;
    width: 7px;
    height: 7px;
    margin: 0 2px;
    border-radius: 50%;
    background: var(--faint, var(--border));
  }
  .sdot.on {
    background: var(--green);
  }
  .sdot.warn {
    background: var(--amber);
  }

  .list {
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2) var(--space-3) var(--space-3);
  }
  .gh {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .gh:first-child {
    margin-top: var(--space-1);
  }
  .gn {
    font-family: var(--mono);
  }
  .gl {
    flex: 1;
    height: 1px;
    background: var(--border);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 52px;
    padding: 6px var(--space-3) 6px var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-lg);
    cursor: pointer;
    outline: none;
  }
  .row.cur {
    background: var(--surface-2);
  }
  .row.sel {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .av {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: var(--radius);
    font-family: var(--mono);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .t-accent {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .t-amber {
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    color: var(--amber);
  }
  .t-blue {
    background: color-mix(in srgb, #6ea8fe 14%, transparent);
    color: #8fb8f5;
  }
  .t-violet {
    background: color-mix(in srgb, #a78bfa 14%, transparent);
    color: #b9a5f5;
  }
  .t-teal {
    background: color-mix(in srgb, #4fd1c5 14%, transparent);
    color: #6fd6cc;
  }
  .txt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .l1 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .sym {
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }
  .tag {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
    font-family: var(--mono);
    font-size: 10px;
    white-space: nowrap;
  }
  .row.cur .tag {
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
  }
  .has {
    display: inline-flex;
    color: var(--green);
  }
  .fav {
    display: inline-flex;
    color: var(--amber);
  }
  .l2 {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .end {
    display: inline-grid;
    flex: none;
    font-size: var(--text-sm);
  }
  .face {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    grid-area: 1 / 1;
  }
  .end .act-face,
  .end.hot .idle {
    visibility: hidden;
  }
  .end.hot .act-face {
    visibility: visible;
  }
  .srcn {
    color: var(--muted);
  }
  .act {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .act:hover {
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
    color: var(--text);
  }
  .act.faved {
    color: var(--amber);
  }
  .openhint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }
  .kbd {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 18px;
    margin-right: 6px;
    padding: 0 4px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
    color: var(--muted);
    font-family: var(--mono);
    font-size: 10px;
  }
  .openhint .kbd {
    margin-right: 0;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-8) var(--space-4);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .warn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: var(--space-1) var(--space-2);
    color: var(--amber);
    font-size: var(--text-xs);
  }
  .muted {
    margin: var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex: none;
    margin: 0 calc(-1 * var(--space-4)) calc(-1 * var(--space-4));
    padding: var(--space-2) var(--space-4);
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .cnt {
    margin-left: auto;
  }
  @media (max-width: 640px) {
    .main {
      grid-template-columns: 1fr;
    }
    .side {
      display: none;
    }
  }
</style>
