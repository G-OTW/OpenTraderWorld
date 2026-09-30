<script>
  // The instrument rail: one list at a time, from two sources that never get confused with
  // each other.
  //
  //  - **Chart lists** are the rail's own (`/api/histviz/lists`), built with the + button,
  //    which opens the same instrument picker the chart uses. They hold chart coordinates,
  //    so a click charts them with no lookup. Recents is the same thing without a name.
  //  - **The Watchlists module's lists** hold *quote* symbols, so charting one is a lookup.
  //    When the list (or the row) quotes through a data connector, that connector is the only
  //    one asked: a VT quoted through IBKR charts on IBKR or not at all.
  //
  // **The module's lists are never written to from here.** Editing one copies it into a chart
  // list first and edits the copy; the original watchlist stays exactly as it was. Turning a
  // chart list into a real watchlist is the Promote button and nothing else.
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import DataModal from './DataModal.svelte';
  import { t } from '$lib/i18n';
  import { histvizApi } from './api.js';
  import { loadRecents, rememberRecent } from './pane.js';
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';

  let {
    current = null,
    /** Set by the page's drag handle. The rail owns its look, the page owns the split. */
    width = 198,
    datasets = [],
    connectors = [],
    onpick,
    /** A row that cannot be charted: the pane says why, in place of the chart. */
    onfail,
    ondownload
  } = $props();

  const PICK_KEY = 'otw.histviz.rail.v1';

  let chartLists = $state([]);
  let moduleLists = $state([]);
  let recents = $state(loadRecents());
  let selected = $state(localStorage.getItem(PICK_KEY) || 'recents');
  let error = $state('');
  /** A Watchlists list being copied into a chart list: one copy at a time, and no second
   *  edit while it runs. */
  let copying = $state(false);
  /** The inline name field: 'new' creates a list, 'rename' renames the one on screen. One
   *  field in one place, so the rail never shows two of them. */
  let formMode = $state(null);
  let draftName = $state('');
  let pickOpen = $state(false);
  // Resolution of a module row to chart coordinates: 'pending' while it is being looked up,
  // an object once found, null when nothing serves it.
  let resolved = $state({});
  /** Why a row could not be resolved, keyed like `resolved`. What the pane shows. */
  let reason = $state({});
  /** Set when a Watchlists list has just been copied into a chart list. */
  let copied = $state(null);

  export function remember(coords) {
    recents = rememberRecent(coords);
  }

  /** The picker's rows, with the two sources as group headers. A themed dropdown rather
   *  than a native select: the OS popup ignores the app palette entirely. */
  const listOpts = $derived([
    { value: 'recents', label: $t('histviz.rail.recents') },
    ...(moduleLists.length
      ? [
          { value: '__mod', label: $t('histviz.rail.fromWatchlists'), header: true },
          ...moduleLists.map((l) => ({ value: `module:${l.id}`, label: l.name }))
        ]
      : []),
    ...(chartLists.length
      ? [
          { value: '__chart', label: $t('histviz.rail.chartLists'), header: true },
          ...chartLists.map((l) => ({ value: `chart:${l.id}`, label: l.name }))
        ]
      : [])
  ]);

  const sel = $derived.by(() => {
    if (selected === 'recents') return { kind: 'recents', name: $t('histviz.rail.recents') };
    const chart = chartLists.find((l) => `chart:${l.id}` === selected);
    if (chart) return { kind: 'chart', name: chart.name, row: chart };
    const mod = moduleLists.find((l) => `module:${l.id}` === selected);
    if (mod) return { kind: 'module', name: mod.name, row: mod };
    return { kind: 'recents', name: $t('histviz.rail.recents') };
  });

  $effect(() => {
    load();
  });

  async function load() {
    try {
      chartLists = await histvizApi.lists();
    } catch (e) {
      error = e.message;
    }
    try {
      moduleLists = await watchlistsApi.list();
    } catch {
      /* the module may be disabled: the rail still has its own lists */
    }
  }

  // Whatever list is selected, drawn from what the rail already holds. Recents and a chart
  // list are coordinates already, so the rows are derived, not fetched: an edit repaints them
  // in the same frame. Only a Watchlists list costs a round trip, and it is fetched once per
  // list rather than on every write that happens to touch `chartLists`.
  let modRows = $state([]);
  let modBusy = $state(false);
  let modLoaded = null; // plain, not $state: the effect writes it and must not re-run on it

  $effect(() => {
    const s = sel;
    if (s.kind !== 'module' || modLoaded === s.row.id) return;
    const id = s.row.id;
    modLoaded = id;
    modRows = [];
    modBusy = true;
    watchlistsApi
      .detail(id)
      .then((r) => {
        if (modLoaded !== id) return; // the picker moved on while this was in flight
        modRows = (r.items ?? []).map((it) => ({
          key: it.id,
          label: it.symbol,
          sub: it.name || it.asset_class,
          item: it,
          coords: null
        }));
      })
      .catch((e) => (error = e.message))
      .finally(() => {
        if (modLoaded === id) modBusy = false;
      });
  });

  const items = $derived.by(() => {
    const s = sel;
    if (s.kind === 'chart')
      return (s.row.items ?? []).map((c) => ({
        key: keyOf(c),
        label: c.ticker,
        sub: subOf(c),
        coords: c
      }));
    if (s.kind === 'module') return modRows;
    return recents.map((c) => ({ key: keyOf(c), label: c.ticker, sub: subOf(c), coords: c }));
  });
  const busy = $derived((sel.kind === 'module' && modBusy) || copying);

  const keyOf = (c) => `${c.provider}|${c.asset_type}|${c.ticker}`;
  /** The second line of a row: what the instrument is called, else what it is. */
  const subOf = (c) => c.name || c.asset_type || '';

  /** The row the active pane is showing, so the rail says where the chart is. */
  const currentKey = $derived(current?.ticker ? keyOf(current) : '');

  /** The chart's asset type for a watchlist item's asset class. The two vocabularies meet
   *  here and nowhere else. */
  function assetTypeOf(item) {
    switch (item.asset_class) {
      case 'crypto':
        return 'crypto';
      case 'forex':
        return 'fx';
      case 'etf':
        return 'etf';
      default:
        return 'equity';
    }
  }

  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

  /** The data connector a watchlist row is bound to. The item's own quote source wins, else
   *  the list's; 'auto' (and a list with none) means the default quote providers, CoinGecko
   *  and Yahoo, which are not chart connectors and so leave the symbol to be searched. */
  function boundConnector(row) {
    const src = (row.item?.quote_source ?? '').trim();
    if (src.toLowerCase() === 'auto') return null;
    const id = UUID.test(src) ? src : (sel.row?.connector_id ?? null);
    if (!id) return null;
    const c = connectors.find((x) => x.id === id) ?? null;
    // An empty list means the grants have not come back, not that nothing is granted: the
    // search is the authority either way, since the server only ever reads granted connectors.
    return { id, name: c?.name ?? '', granted: !connectors.length || !!c };
  }

  const coordsOf = (hit) => ({
    connector_id: hit.connector_id,
    provider: hit.provider,
    asset_type: hit.asset_type,
    ticker: hit.symbol,
    name: hit.name,
    timeframes: hit.timeframes ?? [],
    stream: hit.stream,
    stream_timeframes: hit.stream_timeframes ?? [],
    stream_note: hit.stream_note ?? ''
  });

  /** Find a module item on the connectors the chart is granted.
   *
   *  A watchlist row names a *quote* symbol, so this is a search rather than a translation.
   *  When the list (or the row) quotes through a real data connector, the search is scoped to
   *  that one and nothing else: a VT priced through IBKR must not silently chart on whatever
   *  other connector happens to answer first. A failure is kept with its reason, which is what
   *  the pane shows in place of the chart. */
  async function resolve(row) {
    const cached = resolved[row.key];
    if (cached && cached !== 'pending') return cached;
    if (cached === 'pending') return null;
    const bound = boundConnector(row);
    resolved = { ...resolved, [row.key]: 'pending' };
    const symbol = (row.item.quote_ticker || row.item.symbol || '').trim();
    try {
      const r = await histvizApi.symbols({
        q: symbol,
        asset_type: assetTypeOf(row.item),
        limit: 5,
        ...(bound ? { connectors: [bound.id] } : {})
      });
      const hit = (r.results ?? [])[0];
      if (hit) {
        resolved = { ...resolved, [row.key]: coordsOf(hit) };
        reason = { ...reason, [row.key]: null };
        return resolved[row.key];
      }
      // The server says per connector why it had nothing: a missing credential, a provider
      // that refused, a connector with no symbol search at all.
      const note = (r.notes ?? [])[0];
      if (note) fail(row, { code: note.code, connector: note.connector, message: note.message });
      else if (bound && !bound.granted) fail(row, { code: 'grant' });
      else if (bound) fail(row, { code: 'symbol', connector: bound.name });
      else fail(row, { code: 'none' });
      return null;
    } catch (e) {
      fail(row, { code: 'transport', connector: bound?.name ?? '', message: e.message });
      return null;
    }
  }

  function fail(row, info) {
    resolved = { ...resolved, [row.key]: null };
    reason = { ...reason, [row.key]: { ticker: row.label, name: row.sub ?? '', ...info } };
  }

  async function pick(row) {
    if (resolved[row.key] === 'pending') return;
    const coords = row.coords ?? (await resolve(row));
    if (coords) {
      remember(coords);
      onpick?.(coords);
      return;
    }
    // Clicking always answers: the pane explains what is missing and how to fix it.
    onfail?.(reason[row.key] ?? { ticker: row.label, code: 'none' });
  }

  function dragStart(e, row) {
    const coords = row.coords ?? resolved[row.key];
    if (!coords || coords === 'pending') {
      // A module row still has to be looked up; do it now so the next drag works.
      if (row.item) resolve(row);
      e.preventDefault();
      return;
    }
    e.dataTransfer.setData('application/x-otw-instrument', JSON.stringify(coords));
    e.dataTransfer.effectAllowed = 'copy';
  }

  function selectList(value) {
    if (value !== selected) copied = null;
    selected = value;
    try {
      localStorage.setItem(PICK_KEY, value);
    } catch {
      /* non-fatal */
    }
  }

  async function newList(name = null) {
    const label = (name ?? draftName ?? '').trim() || $t('histviz.rail.newListName');
    const row = await histvizApi.createList({ name: label, items: [] });
    chartLists = [...chartLists, row];
    selectList(`chart:${row.id}`);
    formMode = null;
    draftName = '';
    return row;
  }

  function openForm(mode) {
    if (mode === 'rename' && sel.kind !== 'chart') return;
    formMode = formMode === mode ? null : mode;
    draftName = formMode === 'rename' ? sel.name : '';
  }

  /** A rename keeps the list, its id and its rows: only the label the picker shows changes.
   *  Watchlists lists are not renamed from here, that is the other module's own screen. */
  async function submitForm() {
    const label = draftName.trim();
    try {
      if (formMode !== 'rename') {
        await newList();
        return;
      }
      if (label && sel.kind === 'chart' && label !== sel.row.name)
        writeList(sel.row, { name: label });
      formMode = null;
      draftName = '';
    } catch (e) {
      error = e.message;
    }
  }

  const entryOf = (c) => ({
    provider: c.provider,
    asset_type: c.asset_type,
    ticker: c.ticker,
    timeframe: c.timeframe ?? null,
    connector_id: c.connector_id ?? null,
    name: c.name ?? ''
  });

  /** The list an edit writes to.
   *
   *  A chart list is edited in place. Recents opens a new list. A **Watchlists list is copied**
   *  into a chart list and the copy is what gets edited: this module never writes to the other
   *  one's data. The copy holds coordinates, so every row is resolved on the way in and a row
   *  no connector serves is left out and counted rather than copied as a symbol nobody can
   *  chart. */
  /** The copy's name: the watchlist's own, then "(1)", "(2)" once that one is taken. Copying
   *  a list twice must not leave two chart lists the picker shows under the same label. */
  function copyName(base) {
    const taken = new Set(chartLists.map((l) => l.name));
    if (!taken.has(base)) return base;
    for (let i = 1; ; i++) if (!taken.has(`${base} (${i})`)) return `${base} (${i})`;
  }

  async function writableList({ skip = null, add = null } = {}) {
    if (sel.kind === 'chart') return sel.row;
    if (sel.kind !== 'module') return newList();
    const src = sel.row;
    // The copy carries the edit: the row being deleted is left out of it and a row being
    // added goes in with the rest. One write, and no moment where the copy holds a symbol the
    // user has just removed.
    const rows = items.filter((r) => r.key !== skip);
    const found = new Array(rows.length);
    // Four at a time: a fifty-row watchlist resolved one by one is a visibly frozen rail, and
    // fifty searches fired at once is a connector rate limit.
    const next = (() => {
      let i = 0;
      return () => (i < rows.length ? i++ : -1);
    })();
    const worker = async () => {
      for (let i = next(); i >= 0; i = next()) {
        const row = rows[i];
        const cached = row.coords ?? resolved[row.key];
        const c = cached && cached !== 'pending' ? cached : await resolve(row);
        found[i] = c && c !== 'pending' ? entryOf(c) : null;
      }
    };
    copying = true;
    try {
      await Promise.all([worker(), worker(), worker(), worker()]);
      const out = found.filter(Boolean);
      if (add && !out.some((x) => keyOf(x) === keyOf(add))) out.push(add);
      const created = await histvizApi.createList({ name: copyName(src.name), items: out });
      chartLists = [...chartLists, created];
      selectList(`chart:${created.id}`);
      copied = { name: created.name, skipped: found.filter((x) => !x).length };
      return created;
    } finally {
      copying = false;
    }
  }

  /** Every list write goes through here: the rail repaints from the new items at once and the
   *  server catches up behind it. Writes are chained, so two quick edits cannot race and put a
   *  deleted row back; a failed one puts the server's list back on screen. */
  let writes = Promise.resolve();
  function writeList(list, patch) {
    const next = { name: patch.name ?? list.name, items: patch.items ?? list.items ?? [] };
    chartLists = chartLists.map((l) => (l.id === list.id ? { ...l, ...next } : l));
    writes = writes.then(async () => {
      try {
        const saved = await histvizApi.saveList(list.id, next);
        chartLists = chartLists.map((l) => (l.id === saved.id ? saved : l));
      } catch (e) {
        error = e.message;
        try {
          chartLists = await histvizApi.lists();
        } catch {
          /* offline: leave the optimistic list, the error says why it is not saved */
        }
      }
    });
    return writes;
  }

  /** Add an instrument the picker returned, to the writable list. */
  async function addPicked(next) {
    if (!next?.ticker || copying) return;
    const entry = entryOf(next);
    try {
      // A Watchlists list is copied with the new symbol already in it, not copied and then
      // written to.
      if (sel.kind === 'module') {
        await writableList({ add: entry });
        return;
      }
      const row = await writableList();
      const cur = chartLists.find((l) => l.id === row.id) ?? row;
      if ((cur.items ?? []).some((x) => keyOf(x) === keyOf(entry))) return;
      writeList(cur, { items: [...(cur.items ?? []), entry] });
    } catch (e) {
      error = e.message;
    }
  }

  /** Delete a row. Nothing is charted and nothing is looked up: on a chart list the row is
   *  gone in the same frame, and on a Watchlists list the copy is made without it. */
  async function removeItem(row) {
    if (sel.kind === 'recents' || copying) return;
    if (sel.kind === 'module') {
      try {
        await writableList({ skip: row.key });
      } catch (e) {
        error = e.message;
      }
      return;
    }
    const list = sel.row;
    const key = keyOf(row.coords);
    writeList(list, { items: (list.items ?? []).filter((x) => keyOf(x) !== key) });
  }

  async function promote() {
    if (sel.kind !== 'chart') return;
    try {
      await histvizApi.promoteList(sel.row.id, sel.row.name);
      moduleLists = await watchlistsApi.list();
    } catch (e) {
      error = e.message;
    }
  }

  async function deleteList() {
    if (sel.kind !== 'chart') return;
    try {
      await histvizApi.deleteList(sel.row.id);
      chartLists = chartLists.filter((l) => l.id !== sel.row.id);
      selectList('recents');
    } catch (e) {
      error = e.message;
    }
  }

  const rowState = (row) => (row.coords ? 'ok' : (resolved[row.key] ?? 'unknown'));
</script>

<aside class="rail" style="width:{width}px">
  <div class="head">
    <div class="picker">
      <Dropdown
        value={selected}
        options={listOpts}
        ariaLabel={$t('histviz.rail.list')}
        title={$t('histviz.rail.list')}
        float
        onpick={selectList}
      />
    </div>
  </div>

  <!-- The same four actions whatever list is showing: one that does not apply to a
       Watchlists list is disabled, not removed, so the row never reflows. -->
  <div class="tools" role="toolbar" aria-label={$t('histviz.rail.list')}>
    <button
      class="icon"
      title={$t('histviz.rail.addSymbol')}
      aria-label={$t('histviz.rail.addSymbol')}
      onclick={() => (pickOpen = true)}
    >
      <Icon name="plus" size={12} />
    </button>
    <button
      class="icon"
      class:on={formMode === 'new'}
      title={$t('histviz.rail.newList')}
      aria-label={$t('histviz.rail.newList')}
      onclick={() => openForm('new')}
    >
      <Icon name="folder-plus" size={12} />
    </button>
    <button
      class="icon"
      class:on={formMode === 'rename'}
      disabled={sel.kind !== 'chart'}
      title={$t('histviz.rail.renameList')}
      aria-label={$t('histviz.rail.renameList')}
      onclick={() => openForm('rename')}
    >
      <Icon name="pencil" size={12} />
    </button>
    <span class="count" aria-hidden="true">{items.length || ''}</span>
    <button
      class="icon"
      disabled={sel.kind !== 'chart'}
      title={$t('histviz.rail.promote')}
      aria-label={$t('histviz.rail.promote')}
      onclick={promote}
    >
      <Icon name="upload" size={12} />
    </button>
    <button
      class="icon danger"
      disabled={sel.kind !== 'chart'}
      title={$t('histviz.rail.deleteList')}
      aria-label={$t('histviz.rail.deleteList')}
      onclick={deleteList}
    >
      <Icon name="trash-2" size={12} />
    </button>
  </div>

  {#if formMode}
    <form
      class="new"
      onsubmit={(e) => {
        e.preventDefault();
        submitForm();
      }}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input
        bind:value={draftName}
        autofocus
        placeholder={formMode === 'rename' ? sel.name : $t('histviz.rail.newListName')}
        onkeydown={(e) => {
          if (e.key === 'Escape') formMode = null;
        }}
      />
      <button class="btn sm" type="submit">
        {formMode === 'rename' ? $t('common.save') : $t('common.add')}
      </button>
    </form>
  {/if}

  {#if sel.kind === 'module'}
    <p class="src">{$t('histviz.rail.moduleHint')}</p>
  {/if}

  <!-- Said once, where the edit happened: the module's list is untouched. -->
  {#if copied}
    <p class="src ok">
      <Icon name="check" size={11} />
      {$t('histviz.rail.copied', { name: copied.name })}
      {#if copied.skipped}{$t('histviz.rail.copiedSkipped', { n: copied.skipped })}{/if}
    </p>
  {/if}

  <ErrorText {error} compact />

  <ul class="rows">
    {#each items as row (row.key)}
      <li class:unresolved={rowState(row) === null} class:cur={row.key === currentKey}>
        <button
          class="row"
          draggable="true"
          ondragstart={(e) => dragStart(e, row)}
          onclick={() => pick(row)}
          title={row.sub || row.label}
          aria-current={row.key === currentKey ? 'true' : undefined}
        >
          <span class="line">
            <span class="tick">{row.label}</span>
            {#if rowState(row) === null}
              <span class="warn" title={$t('histviz.rail.noConnector')}>
                <Icon name="alert-triangle" size={10} />
              </span>
            {/if}
          </span>
          {#if row.sub}<span class="sub">{row.sub}</span>{/if}
        </button>
        {#if sel.kind !== 'recents'}
          <button
            class="x"
            type="button"
            disabled={copying}
            title={sel.kind === 'module' ? $t('histviz.rail.removeCopies') : $t('common.remove')}
            aria-label={$t('common.remove')}
            onpointerdown={(e) => e.stopPropagation()}
            onclick={(e) => {
              e.stopPropagation();
              removeItem(row);
            }}
          >
            <Icon name="x" size={11} />
          </button>
        {/if}
      </li>
    {:else}
      <li class="empty">{busy ? $t('common.loading') : $t('histviz.rail.empty')}</li>
    {/each}
  </ul>
</aside>

<!-- The same picker the chart opens: one way to find an instrument in this module. -->
<DataModal
  bind:open={pickOpen}
  {datasets}
  {recents}
  selected={null}
  busy={false}
  onselect={addPicked}
  ondownload={() => ondownload?.()}
/>

<style>
  .rail {
    --control-h: 26px;
    display: flex;
    flex-direction: column;
    flex: none;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .head {
    display: flex;
    gap: var(--space-1);
    align-items: center;
    flex: none;
  }
  .picker {
    flex: 1;
    min-width: 0;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
    padding-bottom: var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
  }
  /* How many instruments the list holds, in the gap between the two action groups: the
     number belongs to the list, and it keeps the spacer from being empty. */
  .count {
    flex: 1;
    text-align: center;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.04em;
    color: var(--faint);
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 22px;
    flex: none;
    padding: 0;
    background: none;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease), background-color var(--dur-fast) var(--ease);
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon.on {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: var(--surface-2);
  }
  .icon:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .icon.danger:hover:not(:disabled) {
    color: var(--red);
  }
  .new {
    display: flex;
    gap: var(--space-1);
  }
  .new input {
    flex: 1;
    min-width: 0;
  }
  .src,
  .empty {
    margin: 0;
    padding: 0 var(--space-1);
    font-size: var(--fs-desc);
    color: var(--dim);
    line-height: 1.35;
  }
  .src.ok {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    color: var(--green);
  }
  .rows {
    list-style: none;
    margin: 0 calc(-1 * var(--space-1));
    padding: 0;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }
  /* The delete button rides over the row instead of sitting beside it: a row that shortens
     itself the moment the pointer touches it re-flows its own text. */
  .rows li {
    position: relative;
  }
  .row {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 1px;
    padding: 5px var(--space-2);
    background: none;
    border: 0;
    border-left: var(--active-rule) solid transparent;
    border-radius: 0;
    color: var(--text);
    cursor: grab;
    text-align: left;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .row:hover {
    background: var(--surface-2);
    border-left-color: color-mix(in srgb, var(--accent) 55%, transparent);
  }
  /* What the active pane is showing. The rail is a navigation list, so it says where you
     are rather than leaving every row looking the same. */
  li.cur .row {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-left-color: var(--accent);
  }
  li.cur .tick {
    color: var(--accent);
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .tick {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
  }
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-desc);
    color: var(--faint);
  }
  .warn {
    flex: none;
    color: var(--amber);
  }
  li.unresolved .tick {
    color: var(--faint);
  }
  .x {
    position: absolute;
    top: 50%;
    right: 2px;
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .rows li:hover .x,
  .rows li:focus-within .x {
    opacity: 1;
  }
  .x:hover {
    color: var(--red);
  }
</style>
