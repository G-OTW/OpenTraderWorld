<script>
  // Strategies modal body: the saved strategies, grouped by the month they were last updated.
  // The search matches names *and* tags (that is what tags are for here), so "btc scalp"
  // narrows to strategies tagged either; the tag strip narrows to one tag. Hovering a row swaps
  // its tags and date for its actions; the strategy currently loaded is marked.
  //
  // Only the list scrolls: the filters and the column heads stay put, and the modal body itself
  // never overflows, so a wheel gesture always lands on one single scroller.
  // Truncated text clips with `overflow: clip`, never `hidden`: a hidden box is a scroll
  // container, and with the modal's `overscroll-behavior: none` on every descendant a wheel
  // gesture starting over it is swallowed instead of reaching the list.
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtDay, fmtMonthName } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { strategies = [], activeId = null, onopen, onduplicate, onremove, onrobot } = $props();

  let query = $state('');
  let tagFilter = $state(''); // '' = all
  let newestFirst = $state(true);

  /** Tags offered in the strip: the most used first, capped so the strip stays one line. */
  const TOP_TAGS = 8;
  const topTags = $derived.by(() => {
    const n = new Map();
    for (const s of strategies) for (const tg of s.tags ?? []) n.set(tg, (n.get(tg) ?? 0) + 1);
    return [...n].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, TOP_TAGS).map(([tg]) => tg);
  });

  const filtered = $derived.by(() => {
    const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const list = strategies.filter((s) => {
      if (tagFilter && !(s.tags ?? []).includes(tagFilter)) return false;
      if (!terms.length) return true;
      // Every term must hit somewhere (name or one tag): narrowing, not widening.
      const hay = `${s.name} ${(s.tags ?? []).join(' ')}`.toLowerCase();
      return terms.every((w) => hay.includes(w));
    });
    const ts = (s) => Date.parse(s.updated_at ?? s.created_at ?? '') || 0;
    return list.sort((a, b) => (newestFirst ? ts(b) - ts(a) : ts(a) - ts(b)));
  });

  /** Month groups: [{ key, label, rows }]. */
  const groups = $derived.by(() => {
    const out = [];
    for (const s of filtered) {
      const iso = s.updated_at ?? s.created_at ?? '';
      const key = String(iso).slice(0, 7);
      let g = out.at(-1);
      if (!g || g.key !== key) out.push((g = { key, label: fmtMonthName(iso), rows: [] }));
      g.rows.push(s);
    }
    return out;
  });

  const SHOWN_TAGS = 3;
</script>

<div class="lib">
  <div class="filters">
    <label class="search">
      <Icon name="search" size={13} />
      <input bind:value={query} placeholder={$t('backtest.library.search')} />
    </label>
    {#if topTags.length}
      <div class="seg" role="tablist">
        <button role="tab" class="all" aria-selected={!tagFilter} onclick={() => (tagFilter = '')}>{$t('backtest.history.all')}</button>
        {#each topTags as tg (tg)}
          <button role="tab" aria-selected={tagFilter === tg} onclick={() => (tagFilter = tagFilter === tg ? '' : tg)}>{tg}</button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="cols row-grid">
    <span>{$t('backtest.library.colName')}</span>
    <span>{$t('backtest.library.colTags')}</span>
    <button
      class="sort"
      title={newestFirst ? $t('backtest.library.sortNewest') : $t('backtest.library.sortOldest')}
      onclick={() => (newestFirst = !newestFirst)}
    >
      {$t('backtest.library.colUpdated')} <Icon name={newestFirst ? 'arrow-down' : 'arrow-up'} size={12} />
    </button>
  </div>

  <div class="list">
    {#if !strategies.length}
      <p class="empty">{$t('backtest.dock.noStrategies')}</p>
    {:else if !groups.length}
      <p class="empty">{$t('backtest.dock.noMatch', { q: query || tagFilter })}</p>
    {:else}
      {#each groups as g (g.key)}
        <div class="month">{g.label}</div>
        {#each g.rows as s (s.id)}
          {@const tags = s.tags ?? []}
          <div class="row row-grid" class:on={s.id === activeId}>
            <button class="nm" onclick={() => onopen?.(s)} title={$t('backtest.dock.loadStrategy')}>{s.name}</button>
            <span class="tags">{[...tags.slice(0, SHOWN_TAGS), ...(tags.length > SHOWN_TAGS ? [`+${tags.length - SHOWN_TAGS}`] : [])].join(' · ')}</span>
            <span class="when">{fmtDay(s.updated_at ?? s.created_at)}</span>
            <div class="acts">
              <button class="open" onclick={() => onopen?.(s)}><Icon name="arrow-right" size={13} /> {$t('backtest.library.open')}</button>
              {#if onrobot}
                <button onclick={() => onrobot(s)}><Icon name="bot" size={13} /> {$t('backtest.library.automate')}</button>
              {/if}
              <button class="ico" title={$t('backtest.dock.duplicate')} aria-label={$t('backtest.dock.duplicate')} onclick={() => onduplicate?.(s)}><Icon name="copy" size={13} /></button>
              <button class="ico danger" title={$t('backtest.history.remove')} aria-label={$t('backtest.history.remove')} onclick={() => onremove?.(s)}><Icon name="trash" size={13} /></button>
            </div>
          </div>
        {/each}
      {/each}
    {/if}
  </div>
</div>

<style>
  /* Fills the modal body edge to edge (rows hover full width), and exactly: the body never
     overflows, only `.list` scrolls. */
  .lib {
    display: flex;
    flex-direction: column;
    height: calc(100% + 2 * var(--space-4));
    margin: calc(-1 * var(--space-4));
    min-height: 0;
  }
  .filters {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-3) var(--space-6) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 260px;
    max-width: 100%;
    color: var(--muted);
    border-bottom: var(--hairline) solid var(--border-control);
    margin-bottom: var(--space-2);
  }
  .search:focus-within {
    border-bottom-color: var(--accent);
  }
  /* The underline is the frame: the field inside has none, and no focus halo. */
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    outline: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-sm);
  }
  .seg {
    display: flex;
    gap: var(--space-4);
    min-width: 0;
    overflow: clip;
  }
  .seg button {
    background: transparent;
    border: none;
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-sm);
    padding: var(--space-2) 0 calc(var(--space-2) + 1px);
    cursor: pointer;
    white-space: nowrap;
  }
  .seg button.all {
    font-family: inherit;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button[aria-selected='true'] {
    color: var(--text);
    box-shadow: inset 0 -1px 0 var(--text);
  }

  .row-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 300px) 90px;
    align-items: center;
    gap: var(--space-4);
    padding: 0 var(--space-6);
  }
  .cols {
    flex: none;
    height: 34px;
    font-size: var(--text-xs);
    color: var(--muted);
    border-bottom: var(--hairline) solid var(--border);
  }
  .sort {
    justify-self: end;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-4) var(--space-6);
  }
  .month {
    font-size: var(--text-xs);
    color: var(--muted);
    padding: var(--space-4) var(--space-6) var(--space-2);
    text-transform: capitalize;
  }
  .row {
    height: 40px;
    border-bottom: var(--hairline) solid color-mix(in srgb, var(--border) 50%, transparent);
  }
  .row:hover,
  .row:focus-within {
    background: var(--surface-2);
  }
  .row.on {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .nm {
    min-width: 0;
    height: 100%;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    overflow: clip;
    text-overflow: ellipsis;
  }
  .tags,
  .when {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
    overflow: clip;
    text-overflow: ellipsis;
  }
  .when {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  /* Hover (or keyboard focus) trades the tags and the date for the actions, in their columns. */
  .acts {
    display: none;
    grid-column: 2 / -1;
    grid-row: 1;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-3);
  }
  .row:hover .acts,
  .row:focus-within .acts {
    display: flex;
  }
  .row:hover :is(.tags, .when),
  .row:focus-within :is(.tags, .when) {
    visibility: hidden;
  }
  .acts button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
  }
  .acts button:hover {
    color: var(--accent);
  }
  .acts .open {
    color: var(--green);
  }
  .acts .ico {
    color: var(--muted);
  }
  .acts .ico:hover {
    color: var(--text);
  }
  .acts .danger:hover {
    color: var(--red);
  }
  @media (max-width: 767px) {
    .lib {
      height: auto;
    }
    .filters {
      flex-wrap: wrap;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-4) 0;
    }
    .seg {
      flex-wrap: wrap;
    }
    .row-grid {
      grid-template-columns: minmax(0, 1fr) 70px;
      padding: 0 var(--space-4);
    }
    .cols span:nth-child(2),
    .tags {
      display: none;
    }
    .acts {
      grid-column: 1 / -1;
    }
    .row:hover .nm,
    .row:focus-within .nm {
      visibility: hidden;
    }
    .acts button:not(.ico) {
      font-size: var(--text-xs);
    }
  }
</style>
