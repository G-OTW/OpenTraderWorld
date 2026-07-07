<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Trades list for the selected category. Log new trades from a template, edit/delete.
  import { journalApi, fmtMoney, ASSET_CLASSES } from './api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import TradeForm from './TradeForm.svelte';
  import { t } from '$lib/i18n';

  let {
    categoryId = '',
    categories = [],
    strategies = [],
    templates = [],
    feeSchedules = [],
    suggestions = { tickers: [], exchanges: [], signals: [] },
    onchanged = () => {}
  } = $props();

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
  function fmtDate(iso) {
    return iso ? new Date(iso).toLocaleDateString() : '—';
  }

  // Decimal places in a number's string form (0 for integers).
  function decimals(n) {
    if (n == null) return 0;
    const s = String(n);
    const i = s.indexOf('.');
    return i < 0 ? 0 : s.length - i - 1;
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
    <div class="new">
      <span class="label">{$t('journal.trades.logTrade')}</span>
      <button class="primary" onclick={() => newTrade(null)}>{$t('journal.trades.quickAllFields')}</button>
      {#each templates as tpl}
        <button class="chip" onclick={() => newTrade(tpl)}>{tpl.name}</button>
      {/each}
    </div>
  </div>

  <section class="filters">
    <input placeholder={$t('journal.trades.filter.ticker')} list="tr-tickers" autocomplete="off" bind:value={fTicker} />
    <select bind:value={fSide} title={$t('journal.breakdown.filter.side')}>
      <option value="">{$t('journal.breakdown.filter.anySide')}</option>
      <option value="long">{$t('journal.side.long')}</option>
      <option value="short">{$t('journal.side.short')}</option>
    </select>
    <select bind:value={fResult} title={$t('journal.trades.filter.result')}>
      <option value="">{$t('journal.trades.filter.anyResult')}</option>
      <option value="win">{$t('journal.trades.filter.wins')}</option>
      <option value="loss">{$t('journal.trades.filter.losses')}</option>
    </select>
    <label class="date">{$t('journal.breakdown.filter.from')} <input type="date" bind:value={fSince} /></label>
    <label class="date">{$t('journal.breakdown.filter.to')} <input type="date" bind:value={fUntil} /></label>
    {#if activeFilterCount > 0}
      <button class="clear" onclick={clearFilters}>{$t('journal.breakdown.filter.clear', { count: activeFilterCount })}</button>
    {/if}
  </section>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if trades.length === 0}
    <div class="empty">{$t('journal.trades.empty')}</div>
  {:else if visibleTrades.length === 0}
    <div class="empty">{$t('journal.trades.noMatch')}</div>
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
          {#each visibleTrades as tr (tr.id)}
            <tr>
              <td class="strong">{tr.ticker || '—'}</td>
              <td>{assetLabel(tr.asset_class)}</td>
              <td><span class="side {tr.side}">{tr.side}</span></td>
              <td>{fmtDate(tr.entry_at)}</td>
              <td>{fmtDate(tr.exit_at)}</td>
              <td class="num">{tr.advanced ? (tr.avg_entry != null ? `@${fmtAvgEntry(tr)}` : '—') : (tr.quantity ?? '—')}</td>
              <td class="num">
                {#if tr.net_pnl == null}
                  <span class="muted">{$t('journal.trades.open')}</span>
                {:else}
                  <span class={tr.net_pnl >= 0 ? 'pos' : 'neg'}>{fmtMoney(tr.net_pnl, tr.currency)}</span>
                  {#if tr.open_qty > 0}<span class="partial" title={$t('journal.trades.partiallyOpen')}>●</span>{/if}
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
  .trades {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .toolbar .new {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  .filters input[list] {
    width: 120px;
  }
  .filters .date {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .clear {
    margin-left: auto;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 6px 10px;
    cursor: pointer;
    font-size: 0.8rem;
  }
  .clear:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .label {
    font-size: 0.8rem;
    color: var(--muted);
  }
  .table-wrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  .num {
    text-align: right;
  }
  .strong {
    font-weight: 600;
  }
  .side {
    text-transform: uppercase;
    font-size: 0.68rem;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 999px;
  }
  .side.long {
    background: color-mix(in srgb, var(--green) 18%, transparent);
    color: var(--green);
  }
  .side.short {
    background: color-mix(in srgb, var(--red) 18%, transparent);
    color: var(--red);
  }
  .pos {
    color: var(--green);
    font-weight: 600;
  }
  .neg {
    color: var(--red);
    font-weight: 600;
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .partial {
    color: var(--amber);
    font-size: 0.6rem;
    margin-left: 4px;
    vertical-align: middle;
  }
  .row-actions {
    display: flex;
    gap: 4px;
  }
  .empty {
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-8);
    text-align: center;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
