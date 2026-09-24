<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Trades list for the selected category. Log new trades from a template, edit/delete.
  import { journalApi, fmtSignedMoney, ASSET_CLASSES } from './api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import TradeForm from './TradeForm.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ScrollTop from '$lib/ui/ScrollTop.svelte';
  import { infiniteScroll } from '$lib/ui/infiniteScroll.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let {
    categoryId = '',
    categories = [],
    strategies = [],
    templates = [],
    feeSchedules = [],
    suggestions = { tickers: [], exchanges: [], signals: [] },
    // Discipline tags, offered as chips in the trade form.
    tags = [],
    // Optional 'YYYY-MM-DD' to pre-scope the list to a single day (calendar click-through).
    // Applied once on mount, overriding any persisted date filter.
    initialDay = '',
    // Set only when this list was opened from a calendar cell: drilling in from a grid
    // has to have a way out that lands back on the day you left.
    onback = null,
    onchanged = () => {}
  } = $props();

  // id → tag, so a trade row can show its tags without a second request.
  const tagById = $derived(new Map(tags.map((x) => [x.id, x])));
  const tagsOf = (trade) => (trade.tags ?? []).map((id) => tagById.get(id)).filter(Boolean);

  let trades = $state([]);
  let loading = $state(true);

  // ── Filters (persisted per browser). Ticker/side/date are sent to the API;
  // win/loss is derived client-side from net_pnl since PnL is computed on read. ──
  const FILTERS_KEY = 'otw.journal.trades.filters.v1';
  let fTicker = $state('');
  let fSide = $state(''); // '' | long | short
  let fResult = $state(''); // '' | win | loss
  let fSince = $state(''); // yyyy-mm-dd
  let fUntil = $state('');
  let filtersLoaded = $state(false);

  function loadFilters() {
    try {
      const p = JSON.parse(localStorage.getItem(FILTERS_KEY) || '{}');
      if (typeof p.fTicker === 'string') fTicker = p.fTicker;
      if (['', 'long', 'short'].includes(p.fSide)) fSide = p.fSide;
      if (['', 'win', 'loss'].includes(p.fResult)) fResult = p.fResult;
      if (typeof p.fSince === 'string') fSince = p.fSince;
      if (typeof p.fUntil === 'string') fUntil = p.fUntil;
    } catch {
      /* corrupt — ignore */
    }
    filtersLoaded = true;
  }

  $effect(() => {
    const snap = { fTicker, fSide, fResult, fSince, fUntil };
    if (!filtersLoaded) return;
    try {
      localStorage.setItem(FILTERS_KEY, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  const activeFilterCount = $derived(
    [fTicker, fSide, fResult, fSince, fUntil].filter(Boolean).length
  );
  function clearFilters() {
    fTicker = fSide = fResult = fSince = fUntil = '';
  }

  // View-only filters applied after fetch (so they refresh live as you type, without a
  // round-trip): ticker is a case-insensitive substring match; win/loss derives from the
  // computed net_pnl. Side/date stay server-side.
  const visibleTrades = $derived.by(() => {
    const tq = fTicker.trim().toLowerCase();
    return trades.filter((t) => {
      if (tq && !(t.ticker ?? '').toLowerCase().includes(tq)) return false;
      if (fResult === 'win') return t.net_pnl != null && t.net_pnl >= 0;
      if (fResult === 'loss') return t.net_pnl != null && t.net_pnl < 0;
      return true;
    });
  });

  // ── Infinite scroll ──
  // The whole filtered set is fetched (ticker/win-loss filter client-side, so the list
  // has to be complete to stay exact); only a slice of it is rendered, grown a page at
  // a time as the sentinel comes into view. A year of imported executions is thousands
  // of rows — that is a DOM problem, not a network one.
  const PAGE = 50;
  let visibleCount = $state(PAGE);
  const pageTrades = $derived(visibleTrades.slice(0, visibleCount));
  const hasMore = $derived(visibleCount < visibleTrades.length);

  // Any change of scope or filter starts the list over at the top of the first page.
  $effect(() => {
    void [trades, fTicker, fResult];
    visibleCount = PAGE;
  });

  let showForm = $state(false);
  let editing = $state(null); // existing trade or null
  let formTemplate = $state(null);

  // Autocomplete pools (tickers/exchanges/signals) come from the parent via the
  // `suggestions` prop; the parent refreshes them after each save (onchanged).

  // The API filter payload (date inputs become RFC3339 day bounds). Ticker is intentionally
  // not sent — it's matched client-side as a live substring (see visibleTrades).
  const apiFilter = $derived({
    category_id: categoryId || undefined,
    side: fSide,
    since: fSince ? new Date(fSince + 'T00:00:00').toISOString() : '',
    until: fUntil ? new Date(fUntil + 'T23:59:59').toISOString() : ''
  });

  // Load whenever the scope or any server-side filter changes.
  $effect(() => {
    const f = apiFilter;
    loading = true;
    journalApi
      .listTrades(f)
      .then((r) => {
        trades = r;
      })
      .finally(() => (loading = false));
  });

  async function reload() {
    trades = await journalApi.listTrades(apiFilter);
  }

  // Restore persisted filters on first mount (before the load effect settles).
  loadFilters();
  // A calendar click-through wins over the restored range: pin both ends to that day.
  if (initialDay) {
    fSince = initialDay;
    fUntil = initialDay;
  }

  function newTrade(template) {
    editing = null;
    formTemplate = template;
    showForm = true;
  }

  function editTrade(tr) {
    editing = tr;
    formTemplate = templates.find((t) => t.id === tr.template_id) ?? null;
    showForm = true;
  }

  async function save(payload) {
    if (editing) {
      await journalApi.updateTrade(editing.id, payload);
    } else {
      await journalApi.addTrade(payload);
    }
    showForm = false;
    await reload();
    onchanged();
  }

  let confirmOpen = $state(false);
  let pendingDelete = $state(null);
  function del(tr) {
    pendingDelete = tr;
    confirmOpen = true;
  }
  async function confirmDelete() {
    const tr = pendingDelete;
    pendingDelete = null;
    if (!tr) return;
    await journalApi.deleteTrade(tr.id);
    await reload();
    onchanged();
  }

  function catName(id) {
    return categories.find((c) => c.id === id)?.name ?? '';
  }
  function assetLabel(id) {
    return ASSET_CLASSES.find((a) => a.id === id)?.label ?? id;
  }
  // Trade times are stored to the minute (TIMESTAMPTZ, entered via datetime-local), so the
  // list shows them: two scalps on the same day are otherwise indistinguishable here. A
  // legacy row that carries an exact local midnight is a date-only entry, and stays one.
  function fmtDate(iso) {
    if (!iso) return '—';
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return '—';
    const day = d.toLocaleDateString();
    if (d.getHours() === 0 && d.getMinutes() === 0) return day;
    return `${day} ${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
  }

  // Decimal places in a number's string form (0 for integers).
  function decimals(n) {
    if (n == null) return 0;
    const s = String(n);
    const i = s.indexOf('.');
    return i < 0 ? 0 : s.length - i - 1;
  }

  // The QTY column shows a quantity. An advanced trade has no flat `quantity`, so it is the
  // sum of the entry legs; a partially-open one reads "closed/total" since the two differ.
  function fmtQty(tr) {
    if (!tr.advanced) return tr.quantity ?? '—';
    const total = (tr.entries ?? []).reduce((n, l) => n + (Number(l.qty) || 0), 0);
    if (!total) return '—';
    const open = Number(tr.open_qty) || 0;
    const trim = (n) => Number(n.toFixed(8)).toString();
    return open > 0 ? `${trim(total - open)}/${trim(total)}` : trim(total);
  }

  // Average entry display: weighted average can be a long repeating decimal, so show it
  // at the precision the user actually typed for the entry legs + 2 (e.g. prices entered
  // with 2 decimals show 4), capped at 6, with trailing zeros trimmed.
  function fmtAvgEntry(tr) {
    if (tr.avg_entry == null) return '—';
    const legPrec = (tr.entries ?? []).reduce((m, l) => Math.max(m, decimals(l.price)), 0);
    const dp = Math.min(legPrec + 2, 6);
    return Number(tr.avg_entry.toFixed(dp)).toString();
  }
</script>

<datalist id="tr-tickers">
  {#each suggestions.tickers as v}<option value={v}></option>{/each}
</datalist>

<div class="trades">
  <div class="toolbar">
    {#if onback}
      <Button icon="chevron-left" onclick={onback}>{$t('journal.trades.backToCalendar')}</Button>
    {/if}
    <div class="new">
      <span class="label">{$t('journal.trades.logTrade')}</span>
      <Button variant="primary" icon="plus" onclick={() => newTrade(null)}>
        {$t('journal.trades.quickAllFields')}
      </Button>
      {#each templates as tpl (tpl.id)}
        <button class="chip" onclick={() => newTrade(tpl)}>{tpl.name}</button>
      {/each}
    </div>
  </div>

  <section class="filter-bar">
    <input placeholder={$t('journal.trades.filter.ticker')} list="tr-tickers" autocomplete="off" bind:value={fTicker} />
    <Dropdown
      bind:value={fSide}
      title={$t('journal.breakdown.filter.side')}
      ariaLabel={$t('journal.breakdown.filter.side')}
      options={[
        { value: '', label: $t('journal.breakdown.filter.anySide') },
        { value: 'long', label: $t('journal.side.long') },
        { value: 'short', label: $t('journal.side.short') }
      ]}
    />
    <Dropdown
      bind:value={fResult}
      title={$t('journal.trades.filter.result')}
      ariaLabel={$t('journal.trades.filter.result')}
      options={[
        { value: '', label: $t('journal.trades.filter.anyResult') },
        { value: 'win', label: $t('journal.trades.filter.wins') },
        { value: 'loss', label: $t('journal.trades.filter.losses') }
      ]}
    />
    <label class="date">{$t('journal.breakdown.filter.from')} <input type="date" bind:value={fSince} /></label>
    <label class="date">{$t('journal.breakdown.filter.to')} <input type="date" bind:value={fUntil} /></label>
    {#if activeFilterCount > 0}
      <div class="filter-clear">
        <Button size="sm" icon="x" onclick={clearFilters}>
          {$t('journal.breakdown.filter.clear', { count: activeFilterCount })}
        </Button>
      </div>
    {/if}
  </section>

  {#if loading}
    <!-- Hold the table's shape: swapping it for a line of text makes the page
         jump when the rows land. -->
    <div class="table-wrap">
      <Skeleton rows={6} height="var(--row-h)" gap="1px" />
    </div>
  {:else if trades.length === 0}
    <!-- The existing i18n string is a full sentence ("No trades yet. Log one from a
         template above."), already translated into seven languages. It reads as the
         description; inventing a separate title key would mean inventing seven
         translations. The action below is what the sentence points at. -->
    <EmptyState icon="candlestick" description={$t('journal.trades.empty')}>
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={() => newTrade(null)}>
          {$t('journal.trades.quickAllFields')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if visibleTrades.length === 0}
    <!-- Nothing matched: the way out is clearing the filters, so offer it here
         rather than making the reader hunt for the toolbar. -->
    <EmptyState icon="filter" description={$t('journal.trades.noMatch')} compact>
      {#snippet action()}
        <Button icon="x" onclick={clearFilters}>
          {$t('journal.breakdown.filter.clear', { count: activeFilterCount })}
        </Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="table-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('journal.trades.col.ticker')}</th>
            <th>{$t('journal.trades.col.class')}</th>
            <th>{$t('journal.trades.col.side')}</th>
            <th>{$t('journal.trades.col.entry')}</th>
            <th>{$t('journal.trades.col.exit')}</th>
            <th class="num">{$t('journal.trades.col.qty')}</th>
            <th class="num">{$t('journal.trades.col.netPnl')}</th>
            {#if !categoryId}<th>{$t('journal.trades.col.category')}</th>{/if}
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each pageTrades as tr (tr.id)}
            <tr>
              <td class="strong">
                {tr.ticker || '—'}
                <!-- Tags stay a dot rail here: the list is scanned, not read. Hovering
                     names them; the analytics screen is where they are priced. -->
                {#if tagsOf(tr).length}
                  <span class="tagdots" title={tagsOf(tr).map((x) => x.name).join(', ')}>
                    {#each tagsOf(tr) as tag (tag.id)}
                      <span class="tagdot {tag.kind}"></span>
                    {/each}
                  </span>
                {/if}
              </td>
              <td>{assetLabel(tr.asset_class)}</td>
              <td><span class="side {tr.side}">{tr.side}</span></td>
              <td class="mono">{fmtDate(tr.entry_at)}</td>
              <td class="mono">{fmtDate(tr.exit_at)}</td>
              <td class="num" title={tr.advanced && tr.avg_entry != null ? `@${fmtAvgEntry(tr)}` : ''}>
                {fmtQty(tr)}
              </td>
              <td class="num">
                {#if tr.net_pnl == null}
                  <span class="muted">{$t('journal.trades.open')}</span>
                {:else}
                  <!-- fmtSignedMoney puts the +/− in the text: the color is a second
                       channel, never the only one. -->
                  <span class={tr.net_pnl >= 0 ? 'pos' : 'neg'}>
                    {fmtSignedMoney(tr.net_pnl, tr.currency)}
                  </span>
                  {#if tr.open_qty > 0}
                    <span class="partial" title={$t('journal.trades.partiallyOpen')} aria-label={$t('journal.trades.partiallyOpen')}>●</span>
                  {/if}
                {/if}
              </td>
              {#if !categoryId}<td>{catName(tr.category_id)}</td>{/if}
              <td class="row-actions">
                <button class="icon" title={$t('journal.trades.editTitle')} onclick={() => editTrade(tr)}><Icon name="pencil" size={14} /></button>
                <button class="icon" title={$t('journal.fees.schedules.delete')} onclick={() => del(tr)}><Icon name="trash" size={14} /></button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <!-- Sentinel below the table (not inside it: a row there would break the grid). -->
    {#if hasMore}
      <div
        class="sentinel"
        use:infiniteScroll={{ onLoadMore: () => (visibleCount += PAGE), disabled: !hasMore }}
      >
        {$t('common.loadingMore')}
      </div>
    {:else if visibleTrades.length > PAGE}
      <div class="sentinel">
        {$t('journal.trades.countOfTotal', { shown: pageTrades.length, total: visibleTrades.length })}
      </div>
    {/if}
    <ScrollTop />
  {/if}
</div>

<Modal
  bind:open={showForm}
  size="lg"
  title={editing ? $t('journal.trades.editModalTitle') : formTemplate ? $t('journal.trades.newModalTitleWithTemplate', { name: formTemplate.name }) : $t('journal.trades.newModalTitle')}
>
  <TradeForm
    template={formTemplate}
    {categories}
    {strategies}
    {feeSchedules}
    initial={editing}
    defaultCategoryId={categoryId}
    {suggestions}
    {tags}
    onsubmit={save}
    oncancel={() => (showForm = false)}
  />
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('journal.trades.deleteModalTitle')}
  message={pendingDelete ? $t('journal.trades.confirmDelete', { ticker: pendingDelete.ticker || '' }) : ''}
  confirmLabel={$t('journal.fees.deleteModal.confirm')}
  danger
  onconfirm={confirmDelete}
/>

<style>
  /* Tag rail on a trade row: kind is the only distinction, names live in the title. */
  .tagdots {
    display: inline-flex;
    gap: 3px;
    margin-left: var(--space-2);
    vertical-align: middle;
  }
  .tagdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--muted);
  }
  .tagdot.mistake {
    background: var(--red);
  }
  .tagdot.rule {
    background: var(--green);
  }

  .trades {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .toolbar .new {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  /* .filter-bar / .filter-clear come from theme/components.css (shared with Breakdown). */
  .filter-bar input[list] {
    width: 120px;
  }
  .label {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  /* Template chip: a control, not a pill — hairline outline, no radius. */
  .chip {
    background: transparent;
    border: 0.5px solid var(--border-control);
    color: var(--text);
    border-radius: 0;
    padding: 5px 10px;
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .chip:hover {
    background: var(--surface-2);
  }
  /* Level 1: the table rests on the page, so it takes a hairline filet, no shadow. */
  .table-wrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
    border-radius: 0;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  /* Foot of the list: "loading more…" while the slice grows, then the final count. */
  .sentinel {
    text-align: center;
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .strong {
    font-weight: var(--fw-medium);
  }

  /* Long/short is a direction, not a status. Institutional: no rounded pill —
     the uppercased word tinted green/red (conventional, not semantic) carries it. */
  .side {
    text-transform: uppercase;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
  }
  .side.long {
    color: var(--green);
  }
  .side.short {
    color: var(--red);
  }

  /* Color is the second channel; fmtSignedMoney already put the +/− in the text. */
  .pos {
    color: var(--green);
    font-weight: var(--fw-medium);
  }
  .neg {
    color: var(--red);
    font-weight: var(--fw-medium);
  }

  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .partial {
    color: var(--amber);
    font-size: 0.6rem; /* a dot, not text: sized to read as a marker */
    margin-left: 4px;
    vertical-align: middle;
  }
  .row-actions {
    display: flex;
    gap: 4px;
  }
</style>
