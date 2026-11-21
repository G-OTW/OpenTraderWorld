<script>
  // The balance sheet: every asset, grouped by category under a hairline header that carries
  // the category total, 36px rows. A name opens the asset; Update, Edit and Delete sit on
  // the row as icon buttons. Type, category and owner filters are the page's (they scope the
  // headline too); search, sort and collapsed groups are this table's own.
  import Icon from '$lib/ui/Icon.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import { ASSET_TYPES, assetTypeLabel } from './api.js';
  import { money, isLinked } from './view.js';
  import { t } from '$lib/i18n';

  let {
    /** Assets for the page filter (server side). */
    assets = [],
    currency = 'USD',
    /** (amount, from) => amount in `currency`, or null without a rate. */
    toDisplay = (v) => v,
    /** The breakdown's aged valuations, `[{ id, age_days }]`. */
    stale = [],
    categories = [],
    owners = [],
    /** Category name -> identity colour (the composition bar's), or undefined. */
    colorFor = () => undefined,
    tType = $bindable([]),
    tCategory = $bindable([]),
    tOwner = $bindable([]),
    onopen = () => {},
    onedit = () => {},
    ondelete = () => {}
  } = $props();

  let tSearch = $state('');
  let tSort = $state('name'); // name | type | value | date
  let tDir = $state('asc');
  let collapsed = $state(new Set());

  const staleById = $derived(new Map((stale ?? []).map((s) => [s.id, s])));

  // An asset's signed value in the display currency: a debt counts against its group.
  function shownOf(a) {
    const v = a.estimated_value ?? a.latest_value;
    return v == null ? null : v * (a.sign < 0 ? -1 : 1);
  }
  function displayOf(a) {
    const v = shownOf(a);
    if (v == null) return null;
    return toDisplay(v, a.currency) ?? v;
  }

  const tableAssets = $derived.by(() => {
    const q = tSearch.trim().toLowerCase();
    const rows = assets.filter((a) => {
      if (tType.length && !tType.includes(a.asset_type)) return false;
      if (tCategory.length && !tCategory.includes(a.category || '')) return false;
      if (tOwner.length && !tOwner.includes(a.owner || '')) return false;
      if (q && !a.name.toLowerCase().includes(q) && !(a.category || '').toLowerCase().includes(q)) return false;
      return true;
    });
    const dir = tDir === 'desc' ? -1 : 1;
    const cmp = (a, b) => {
      switch (tSort) {
        case 'type':
          return assetTypeLabel(a.asset_type).localeCompare(assetTypeLabel(b.asset_type));
        case 'value':
          return (displayOf(a) ?? -Infinity) - (displayOf(b) ?? -Infinity);
        case 'date':
          return String(a.latest_at || '').localeCompare(String(b.latest_at || ''));
        default:
          return a.name.localeCompare(b.name);
      }
    };
    return [...rows].sort((a, b) => cmp(a, b) * dir || a.name.localeCompare(b.name));
  });

  const groupKeys = $derived([...new Set(tableAssets.map((a) => a.category || ''))]);
  // When every group is collapsed, the header sorts the categories themselves: the header
  // acts on whatever level is on screen.
  const allCollapsed = $derived(groupKeys.length > 0 && groupKeys.every((k) => collapsed.has(k)));

  const grouped = $derived.by(() => {
    const map = new Map();
    for (const a of tableAssets) {
      const key = a.category || '';
      if (!map.has(key)) map.set(key, { key, label: a.category || $t('wealth.page.uncategorized'), rows: [], total: 0 });
      const g = map.get(key);
      g.rows.push(a);
      // In the display currency, signed: a mortgage filed under Real estate lowers it.
      g.total += displayOf(a) ?? 0;
    }
    const groups = [...map.values()];
    if (allCollapsed) {
      const dir = tDir === 'desc' ? -1 : 1;
      const latestOf = (g) => g.rows.reduce((m, r) => (String(r.latest_at || '') > m ? String(r.latest_at || '') : m), '');
      const cmp = (a, b) => {
        if (tSort === 'value') return a.total - b.total;
        if (tSort === 'date') return latestOf(a).localeCompare(latestOf(b));
        return a.label.localeCompare(b.label);
      };
      return groups.sort((a, b) => cmp(a, b) * dir || a.label.localeCompare(b.label));
    }
    return groups.sort((a, b) => b.total - a.total || a.label.localeCompare(b.label));
  });

  function toggleGroup(key) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsed = next;
  }
  function toggleAll() {
    collapsed = allCollapsed ? new Set() : new Set(groupKeys);
  }
  function sortBy(col) {
    if (tSort === col) tDir = tDir === 'asc' ? 'desc' : 'asc';
    else {
      tSort = col;
      tDir = col === 'value' || col === 'date' ? 'desc' : 'asc';
    }
  }
  const ariaSort = (col) => (tSort !== col ? 'none' : tDir === 'asc' ? 'ascending' : 'descending');
  function clearFilters() {
    tSearch = '';
    tType = [];
    tCategory = [];
    tOwner = [];
  }
  function toggleOwner(o) {
    tOwner = tOwner.includes(o) ? tOwner.filter((x) => x !== o) : [...tOwner, o];
  }
  const filtered = $derived(!!(tSearch || tType.length || tCategory.length || tOwner.length));

  // What a linked row says about its last reading: a word, never a red surface.
  const liveWord = (a) => (a.live_error ? $t('wealth.live.failed') : a.latest_value == null ? $t('wealth.live.unread') : $t('wealth.live.ok'));
</script>

{#snippet sortBtn(col, label)}
  <button class="sortable" class:on={tSort === col} onclick={() => sortBy(col)}>
    {label}{#if tSort === col}<Icon name={tDir === 'asc' ? 'chevron-up' : 'chevron-down'} size={11} />{/if}
  </button>
{/snippet}

<section class="box assets">
  <div class="toolbar">
    <div class="t-title">
      <h3>{$t('wealth.page.assetsTab')}</h3>
      <span class="count">{$t('wealth.page.countOfTotal', { count: tableAssets.length, total: assets.length })}</span>
    </div>
    <div class="t-controls">
      <label class="search">
        <Icon name="search" size={13} />
        <input placeholder={$t('wealth.page.searchPlaceholder')} bind:value={tSearch} aria-label={$t('common.search')} />
      </label>
      <MultiSelect bind:value={tType} options={ASSET_TYPES.map((at) => ({ value: at.id, label: at.label }))} allLabel={$t('wealth.page.allTypes')} width="150px" />
      <MultiSelect bind:value={tCategory} options={categories} allLabel={$t('wealth.page.allCategories')} width="150px" />
      {#if filtered}
        <button class="btn sm ghost" onclick={clearFilters}>{$t('common.clear')}</button>
      {/if}
      <button class="btn sm" onclick={toggleAll}>{allCollapsed ? $t('wealth.page.expandAll') : $t('wealth.page.collapseAll')}</button>
    </div>
    {#if owners.length}
      <div class="owner-chips" role="group" aria-label={$t('wealth.owner.filter')}>
        <span class="eyebrow">{$t('wealth.owner.filter')}</span>
        {#each owners as o (o)}
          <button class="chip" class:active={tOwner.includes(o)} aria-pressed={tOwner.includes(o)} onclick={() => toggleOwner(o)}>{o}</button>
        {/each}
        <button class="chip" class:active={tOwner.includes('')} aria-pressed={tOwner.includes('')} onclick={() => toggleOwner('')}>{$t('wealth.owner.none')}</button>
      </div>
    {/if}
  </div>

  <table class="tbl">
    <thead>
      <tr>
        <th aria-sort={tSort === 'type' ? ariaSort('type') : ariaSort('name')}>
          <span class="th-pair">{@render sortBtn('name', $t('wealth.page.colAsset'))}<span class="sep">/</span>{@render sortBtn('type', $t('wealth.page.colType'))}</span>
        </th>
        <th class="c-date" aria-sort={ariaSort('date')}>{@render sortBtn('date', $t('wealth.page.colAsOf'))}</th>
        <th class="num c-val" aria-sort={ariaSort('value')}>{@render sortBtn('value', $t('wealth.page.colCurrentValue'))}</th>
        <th class="act-th c-act"><span class="sr">{$t('wealth.page.colActions')}</span></th>
      </tr>
    </thead>
    <tbody>
      {#each grouped as g (g.key)}
        {@const dot = colorFor(g.key)}
        <tr class="group-row" onclick={() => toggleGroup(g.key)}>
          <td class="group-head">
            <button class="caret" aria-expanded={!collapsed.has(g.key)} aria-label={g.label} onclick={(e) => { e.stopPropagation(); toggleGroup(g.key); }}>
              <Icon name={collapsed.has(g.key) ? 'chevron-right' : 'chevron-down'} size={12} />
            </button>
            {#if dot}<span class="swatch" style:background={dot} aria-hidden="true"></span>{/if}
            <span class="group-name">{g.label}</span>
            <span class="group-count">{g.rows.length}</span>
          </td>
          <td class="c-date"></td>
          <td class="num group-total" class:neg={g.total < 0}>{money(g.total, currency)}</td>
          <td></td>
        </tr>
        {#if !collapsed.has(g.key)}
          {#each g.rows as a (a.id)}
            {@const shown = shownOf(a)}
            {@const conv = shown == null ? null : toDisplay(shown, a.currency)}
            {@const est = a.estimated_value != null}
            {@const aged = staleById.get(a.id)}
            {@const linked = isLinked(a)}
            <tr class="asset-row">
              <td class="name-cell">
                <div class="name-line">
                  {#if linked}<span class="link-ic" title={$t('wealth.page.linkedTitle')}><Icon name="link" size={11} /></span>{/if}
                  <button class="name" title={a.name} onclick={() => onopen(a)}>{a.name}</button>
                  {#if a.owner}<span class="owner-tag">{a.owner}</span>{/if}
                </div>
                <div class="sub-line">
                  <span>{assetTypeLabel(a.asset_type)}</span>
                  {#if linked}
                    <span class="live" class:failed={!!a.live_error} title={a.live_error ? $t('wealth.live.error', { error: a.live_error }) : ''}>· {liveWord(a)}</span>
                  {/if}
                </div>
              </td>
              <td class="c-date date-cell">
                <span class="date">{a.latest_at || $t('wealth.page.notValued')}</span>
                {#if aged}
                  <span class="aged" title={$t('wealth.page.agedTitle', { days: a.review_days })}>
                    <span class="dot" aria-hidden="true"></span>{aged.age_days == null ? $t('wealth.page.neverValued') : $t('wealth.page.aged', { n: aged.age_days })}
                  </span>
                {/if}
              </td>
              <td class="num val-cell">
                {#if shown == null}
                  <span class="word">{a.live_error ? $t('wealth.live.unread') : $t('wealth.page.na')}</span>
                {:else}
                  <span class="val-line" title={est ? $t('wealth.drift.rowNote', { value: money(a.latest_value, a.currency) }) : ''}>
                    {#if est}<span class="est-tag">{$t('wealth.drift.est')}</span>{/if}
                    <span class="val" class:est class:neg={shown < 0}>{a.currency !== currency && conv != null ? money(conv, currency) : money(shown, a.currency)}</span>
                  </span>
                  {#if a.currency !== currency && conv != null}<span class="native">{money(shown, a.currency)}</span>{/if}
                {/if}
              </td>
              <td class="row-actions">
                <button class="icon" title={$t('wealth.page.updateBtn')} aria-label="{$t('wealth.page.updateBtn')}: {a.name}" onclick={() => onopen(a)}><Icon name="refresh-cw" size={14} /></button>
                <button class="icon" title={$t('wealth.page.editTitle')} aria-label="{$t('wealth.page.editTitle')}: {a.name}" onclick={() => onedit(a)}><Icon name="pencil" size={14} /></button>
                <button class="icon danger-hover" title={$t('wealth.page.deleteTitle')} aria-label="{$t('wealth.page.deleteTitle')}: {a.name}" onclick={() => ondelete(a)}><Icon name="trash" size={14} /></button>
              </td>
            </tr>
          {/each}
        {/if}
      {/each}
      {#if tableAssets.length === 0}
        <tr><td colspan="4" class="empty-row">{$t('wealth.page.noAssetsMatch')}</td></tr>
      {/if}
    </tbody>
  </table>
</section>

<style>
  .box {
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    min-width: 0;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .t-title {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .count {
    color: var(--dim);
    font-family: var(--mono);
    font-size: 11px;
  }
  .t-controls {
    display: flex;
    flex-wrap: wrap;
    align-items: stretch;
    gap: var(--space-2);
  }
  /* One frame for the search: the box owns the hairline, the input inside is bare. */
  .search {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-2);
    min-width: 200px;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    color: var(--dim);
    transition: border-color 160ms ease;
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 12px;
    outline: none;
  }
  .t-controls :global(.ms-toggle) {
    height: 28px;
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: transparent;
    font-size: 12px;
  }
  .t-controls :global(.ms-toggle.on) {
    border-color: var(--accent);
  }
  .owner-chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    width: 100%;
  }
  .eyebrow {
    margin-right: var(--space-1);
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  table {
    width: 100%;
    table-layout: fixed;
    border-collapse: separate;
    border-spacing: 0;
  }
  .c-date {
    width: 150px;
  }
  .c-val {
    width: 190px;
  }
  .c-act {
    width: 108px;
  }
  /* Sticky header: the page column scrolls, the header stays. */
  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--bg);
  }
  thead th:first-child,
  .group-head {
    padding-left: var(--space-4);
  }
  thead th:last-child,
  tbody td:last-child {
    padding-right: var(--space-3);
  }
  .th-pair {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .sep {
    color: var(--faint);
  }
  .sortable {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0;
    background: none;
    border: none;
    font: inherit;
    color: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }
  .sortable:hover,
  .sortable.on {
    color: var(--text);
  }
  th.num .sortable {
    justify-content: flex-end;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  /* Group header: a hairline row, the category total right aligned under the value column. */
  .group-row {
    cursor: pointer;
  }
  .group-row td {
    height: 32px;
    border-top: var(--hairline) solid var(--border-control);
    background: var(--surface-2);
  }
  tbody tr.group-row:first-child td {
    border-top: none;
  }
  table.tbl tbody tr.group-row:hover {
    background: transparent;
  }
  table.tbl tbody tr.group-row:hover td {
    background: var(--surface-hover);
  }
  .group-head {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .caret {
    display: inline-flex;
    vertical-align: middle;
    margin-right: var(--space-1);
    padding: 0;
    background: none;
    border: none;
    color: var(--dim);
    cursor: pointer;
  }
  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: var(--space-2);
    vertical-align: middle;
  }
  .group-name {
    color: var(--text);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .group-count {
    margin-left: var(--space-2);
    color: var(--dim);
    font-family: var(--mono);
    font-size: 11px;
  }
  .group-total {
    font-weight: 600;
  }

  .asset-row td {
    height: 36px;
  }
  .name-cell {
    min-width: 0;
    padding-left: var(--space-6);
  }
  .name-line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .name {
    min-width: 0;
    padding: 0;
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    line-height: 16px;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .link-ic {
    display: inline-flex;
    color: var(--dim);
    flex: none;
  }
  .owner-tag {
    flex: none;
    padding: 0 6px;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: 10px;
    line-height: 15px;
  }
  .sub-line {
    display: flex;
    gap: var(--space-1);
    color: var(--dim);
    font-size: 11px;
    line-height: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .live.failed {
    color: var(--amber-ink);
  }
  .date-cell {
    white-space: nowrap;
  }
  .date {
    display: block;
    color: var(--muted);
    font-family: var(--mono);
    font-size: 12px;
    line-height: 16px;
  }
  .aged {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--amber-ink);
    font-size: 10px;
    line-height: 14px;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
  }
  .val-cell {
    white-space: nowrap;
  }
  .val-line {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-1);
  }
  .val {
    font-size: 13px;
    line-height: 16px;
  }
  /* An estimate is not a valuation: dimmer, a dashed underline, an "est." eyebrow. */
  .val.est {
    color: var(--dim);
    text-decoration: underline dashed var(--faint);
    text-underline-offset: 3px;
  }
  .est-tag {
    color: var(--dim);
    font-family: var(--font);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .native {
    display: block;
    color: var(--dim);
    font-size: 11px;
    line-height: 14px;
  }
  .word {
    color: var(--dim);
    font-family: var(--font);
    font-size: 12px;
  }
  .row-actions {
    text-align: right;
    white-space: nowrap;
  }
  /* Row actions appear on hover or keyboard focus; on touch they are always there. */
  .row-actions button {
    opacity: 0;
    transition: opacity 160ms ease;
  }
  .asset-row:hover .row-actions button,
  .asset-row:focus-within .row-actions button {
    opacity: 1;
  }
  @media (hover: none) {
    .row-actions button {
      opacity: 1;
    }
  }
  .empty-row {
    padding: var(--space-6);
    text-align: center;
    color: var(--muted);
  }

  @media (max-width: 767px) {
    .toolbar {
      padding: var(--space-3);
    }
    .t-controls {
      width: 100%;
    }
    .search {
      flex: 1 1 100%;
      min-width: 0;
      height: 44px;
    }
    .t-controls :global(.ms-toggle),
    .t-controls .btn,
    .chip {
      min-height: 44px;
    }
    /* Phone: the three columns that matter, name, value, actions. */
    .c-date {
      display: none;
    }
    .c-val {
      width: 120px;
    }
    .c-act {
      width: 96px;
    }
    .name-cell,
    .group-head,
    thead th:first-child {
      padding-left: var(--space-3);
    }
    .row-actions button {
      width: 32px;
      min-height: 44px;
    }
    .asset-row td {
      height: 48px;
    }
  }
</style>
