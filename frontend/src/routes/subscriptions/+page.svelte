<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Subscription Tracker module. Top: monthly-spend chart + totals rail. Below: filterable
  // list with a "+" to add. Money is shown in the module display currency (FX-converted).
  import { onMount } from 'svelte';
  import { subsApi, fmtMoney, monthlyFactor, nextBillingDate, fmtDate, CURRENCIES, FREQUENCIES } from '$lib/modules/subscriptions/api.js';
  import MonthlyChart from '$lib/modules/subscriptions/MonthlyChart.svelte';
  import CategoryChart from '$lib/modules/subscriptions/CategoryChart.svelte';
  import SubscriptionForm from '$lib/modules/subscriptions/SubscriptionForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let subs = $state([]);
  let bd = $state(null);
  let settings = $state({ display_currency: 'USD' });
  let suggestions = $state({ platforms: [], categories: [] });
  let loading = $state(true);

  // Filters.
  let fPlatform = $state('');
  let fCategory = $state('');

  let showForm = $state(false);
  let editing = $state(null);

  // Chart granularity toggle: 'month' (per-month bars) or 'year' (calendar-year totals).
  let chartMode = $state('month');
  // Chart coloring: 'single' (one accent color) or 'multi' (one color per subscription).
  let chartColor = $state('single');
  // Visualization toggle: 'spend' (monthly/yearly bars) or 'category' (per-category bars).
  let chartView = $state('spend');

  // Price-band filter (table-only, client-side, compares raw price in each sub's own currency).
  // null = all. Bands: <10, 10–50, 50–200, 200+.
  // Labels for these bands are plain numeric ranges (not natural-language), so they render as-is
  // across locales; only the "Any price" placeholder and column headers are keyed.
  const PRICE_BANDS = [
    { id: 'lt10', label: '< 10', test: (p) => p < 10 },
    { id: '10-50', label: '10–50', test: (p) => p >= 10 && p < 50 },
    { id: '50-200', label: '50–200', test: (p) => p >= 50 && p < 200 },
    { id: 'gte200', label: '200+', test: (p) => p >= 200 }
  ];
  let fBand = $state('');

  const filter = $derived({ platform: fPlatform, category: fCategory });
  const activeFilters = $derived([fPlatform, fCategory, fBand].filter(Boolean).length);

  // Table-only search + sort (client-side; does not affect the chart/breakdown above).
  let tSearch = $state('');
  let tSort = $state('name'); // name | platform | category | price | frequency | monthly | next
  let tDir = $state('asc'); // asc | desc

  onMount(async () => {
    [settings, suggestions] = await Promise.all([subsApi.getSettings(), subsApi.suggestions()]);
    await reload();
    loading = false;
  });

  async function reload() {
    // Wide window (12 back, 24 fwd) so the chart's year view has full calendar-year buckets.
    [subs, bd] = await Promise.all([
      subsApi.list(filter),
      subsApi.breakdown({ ...filter, months_back: 12, months_fwd: 24 })
    ]);
  }

  // Re-fetch when filters change.
  $effect(() => {
    void filter;
    if (!loading) reload();
  });

  function clearFilters() {
    fPlatform = '';
    fCategory = '';
    fBand = '';
  }

  function openAdd() {
    editing = null;
    showForm = true;
  }
  function openEdit(s) {
    editing = s;
    showForm = true;
  }

  async function save(payload) {
    if (editing) await subsApi.update(editing.id, payload);
    else await subsApi.add(payload);
    showForm = false;
    suggestions = await subsApi.suggestions();
    await reload();
  }

  // ConfirmModal rather than the browser's confirm(), which it exists to replace.
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function del(s) {
    pendingDelete = s;
    confirmOpen = true;
  }
  async function confirmDelete() {
    const s = pendingDelete;
    pendingDelete = null;
    if (!s) return;
    await subsApi.remove(s.id);
    await reload();
  }

  async function setCurrency(e) {
    settings = await subsApi.updateSettings({ display_currency: e.target.value });
    await reload();
  }

  const cur = $derived(settings.display_currency);
  // FREQUENCIES labels come from api.js (not localized there); map id -> key here for display.
  const FREQUENCY_KEYS = {
    weekly: 'subscriptions.form.freqWeekly',
    monthly: 'subscriptions.form.freqMonthly',
    quarterly: 'subscriptions.form.freqQuarterly',
    yearly: 'subscriptions.form.freqYearly'
  };
  const freqLabel = (id) => {
    const f = FREQUENCIES.find((x) => x.id === id);
    return $t(FREQUENCY_KEYS[id] ?? f?.label ?? id);
  };
  // Each sub's monthly-equivalent in its own currency (for the list display).
  const monthlyOf = (s) => s.price * monthlyFactor(s.frequency);

  // Table view: client-side search + sort over the loaded subs.
  const tableSubs = $derived.by(() => {
    const q = tSearch.trim().toLowerCase();
    const band = PRICE_BANDS.find((b) => b.id === fBand);
    const rows = subs.filter((s) => {
      if (band && !band.test(s.price)) return false;
      if (!q) return true;
      return [s.name, s.platform, s.category].some((v) => (v || '').toLowerCase().includes(q));
    });
    const dir = tDir === 'desc' ? -1 : 1;
    const nextTs = (s) => {
      const d = s.active ? nextBillingDate(s) : null;
      return d ? d.getTime() : Infinity;
    };
    const cmp = (a, b) => {
      switch (tSort) {
        case 'platform':
          return (a.platform || '').localeCompare(b.platform || '');
        case 'category':
          return (a.category || '').localeCompare(b.category || '');
        case 'price':
          return a.price - b.price;
        case 'frequency':
          return monthlyFactor(a.frequency) - monthlyFactor(b.frequency);
        case 'monthly':
          return monthlyOf(a) - monthlyOf(b);
        case 'next':
          return nextTs(a) - nextTs(b);
        default:
          return a.name.localeCompare(b.name);
      }
    };
    return [...rows].sort((a, b) => cmp(a, b) * dir);
  });

  function sortBy(col) {
    if (tSort === col) {
      tDir = tDir === 'asc' ? 'desc' : 'asc';
    } else {
      tSort = col;
      tDir = col === 'price' || col === 'monthly' ? 'desc' : 'asc';
    }
  }
  function sortArrow(col) {
    if (tSort !== col) return '';
    return tDir === 'asc' ? ' ▲' : ' ▼';
  }
  /** What a screen reader announces for a sortable column header. */
  function ariaSort(col) {
    if (tSort !== col) return 'none';
    return tDir === 'asc' ? 'ascending' : 'descending';
  }
</script>

<!-- A sortable column header. The clickable part is a real <button>, so the column is
     reachable by Tab and operable by Enter/Space; aria-sort announces the direction.
     A bare onclick on the <th> looked identical and did neither. -->
{#snippet sortHeader(col, label, numeric = false)}
  <th class:num={numeric} aria-sort={ariaSort(col)}>
    <button class="sortable" onclick={() => sortBy(col)}>{label}{sortArrow(col)}</button>
  </th>
{/snippet}

<div class="subs">
  <header class="head">
    <div class="title">
      <h1>{$t('subscriptions.page.title')}</h1>
      <label class="cur-select">
        <span>{$t('subscriptions.page.display')}</span>
        <select value={cur} onchange={setCurrency}>
          {#each CURRENCIES as c}<option value={c}>{c}</option>{/each}
        </select>
      </label>
    </div>
    <div class="head-actions">
      <QuickReminderButton title={$t('subscriptions.page.addReminder')} />
      <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('subscriptions.page.addSubscription')}</button>
    </div>
  </header>

  {#if loading}
    <!-- Chart card up top, then rows. The skeleton keeps that two-part shape so the page
         does not collapse to a line and reflow when the data lands. -->
    <div class="sk-page" aria-busy="true">
      <div class="sk-chart"><Skeleton height="180px" /></div>
      <Skeleton rows={5} height="2.2rem" gap="var(--space-2)" />
    </div>
  {:else}
    <section class="top">
      <div class="card chart-card">
        <div class="chart-head">
          <h3>
            {#if chartView === 'category'}{chartMode === 'year' ? $t('subscriptions.page.thisYearByCategory') : $t('subscriptions.page.thisMonthByCategory')}{:else}{chartMode === 'year' ? $t('subscriptions.page.yearlySpend') : $t('subscriptions.page.monthlySpend')}{/if}
            <span class="cur">{$t('subscriptions.page.inCurrency', { cur })}</span>
          </h3>
          <div class="chart-toggles">
            <div class="toggle" role="group" aria-label={$t('subscriptions.page.visualization')}>
              <button class:active={chartView === 'spend'} onclick={() => (chartView = 'spend')}>{$t('subscriptions.page.spend')}</button>
              <button class:active={chartView === 'category'} onclick={() => (chartView = 'category')}>{$t('subscriptions.page.categories')}</button>
            </div>
            <div class="toggle" role="group" aria-label={$t('subscriptions.page.chartGranularity')}>
              <button class:active={chartMode === 'month'} onclick={() => (chartMode = 'month')}>{$t('subscriptions.page.month')}</button>
              <button class:active={chartMode === 'year'} onclick={() => (chartMode = 'year')}>{$t('subscriptions.page.year')}</button>
            </div>
            {#if chartView === 'spend'}
              <div class="toggle" role="group" aria-label={$t('subscriptions.page.chartColoring')}>
                <button class:active={chartColor === 'single'} onclick={() => (chartColor = 'single')}>{$t('subscriptions.page.grouped')}</button>
                <button class:active={chartColor === 'multi'} onclick={() => (chartColor = 'multi')}>{$t('subscriptions.page.perSub')}</button>
              </div>
            {/if}
          </div>
        </div>
        {#if chartView === 'category'}
          <CategoryChart months={bd?.months ?? []} years={bd?.years ?? []} categories={bd?.categories ?? []} currency={cur} mode={chartMode} />
        {:else}
          <MonthlyChart
            months={bd?.months ?? []}
            years={bd?.years ?? []}
            subs={bd?.subs ?? []}
            currency={cur}
            mode={chartMode}
            color={chartColor}
          />
        {/if}
      </div>
      <div class="rail">
        <div class="stat big">
          <span class="lbl">{$t('subscriptions.page.monthlyTotal')}</span>
          <strong>{fmtMoney(bd?.monthly_total, cur)}</strong>
        </div>
        <div class="stat">
          <span class="lbl">{$t('subscriptions.page.yearly')}</span>
          <strong>{fmtMoney(bd?.yearly_total, cur)}</strong>
        </div>
        <div class="stat highlight">
          <span class="lbl">{$t('subscriptions.page.nextMonthBilling')}</span>
          <strong>{fmtMoney(bd?.next_month_total, cur)}</strong>
        </div>
        <div class="stat">
          <span class="lbl">{$t('subscriptions.page.activeSubs')}</span>
          <strong>{bd?.active_count ?? 0}</strong>
        </div>
        {#if bd?.unconverted > 0}
          <p class="warn"><Icon name="alert-triangle" size={13} /> {$t('subscriptions.page.unconvertedWarning', { count: bd.unconverted, cur })}</p>
        {/if}
      </div>
    </section>

    <section class="filters">
      <input class="search" placeholder={$t('subscriptions.page.searchPlaceholder')} autocomplete="off" bind:value={tSearch} />
      <input placeholder={$t('subscriptions.form.platform')} list="flt-platforms" autocomplete="off" bind:value={fPlatform} />
      <input placeholder={$t('subscriptions.form.category')} list="flt-categories" autocomplete="off" bind:value={fCategory} />
      <select class="band" bind:value={fBand} aria-label={$t('subscriptions.page.priceBand')}>
        <option value="">{$t('subscriptions.page.anyPrice')}</option>
        {#each PRICE_BANDS as b}<option value={b.id}>{b.label}</option>{/each}
      </select>
      {#if activeFilters > 0}
        <button class="clear" onclick={clearFilters}>{$t('subscriptions.page.clearCount', { count: activeFilters })}</button>
      {/if}
      <span class="count-tag">{$t('subscriptions.page.countOf', { shown: tableSubs.length, total: subs.length })}</span>
      <datalist id="flt-platforms">
        {#each suggestions.platforms as v}<option value={v}></option>{/each}
      </datalist>
      <datalist id="flt-categories">
        {#each suggestions.categories as v}<option value={v}></option>{/each}
      </datalist>
    </section>

    {#if subs.length === 0}
      {#if activeFilters}
        <EmptyState icon="filter" compact title={$t('subscriptions.page.emptyFiltered')} />
      {:else}
        <EmptyState icon="receipt" title={$t('subscriptions.page.emptyTitle')} description={$t('subscriptions.page.emptyBody')}>
          {#snippet action()}
            <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('subscriptions.page.addSubscription')}</button>
          {/snippet}
        </EmptyState>
      {/if}
    {:else}
      <div class="table-wrap">
        <table class="tbl">
          <thead>
            <tr>
              {@render sortHeader('name', $t('subscriptions.form.name'))}
              {@render sortHeader('platform', $t('subscriptions.form.platform'))}
              {@render sortHeader('category', $t('subscriptions.form.category'))}
              {@render sortHeader('price', $t('subscriptions.form.price'), true)}
              {@render sortHeader('frequency', $t('subscriptions.form.frequency'))}
              {@render sortHeader('monthly', $t('subscriptions.page.monthlyEq'), true)}
              {@render sortHeader('next', $t('subscriptions.page.nextBilling'))}
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each tableSubs as s (s.id)}
              <tr class:inactive={!s.active}>
                <td class="strong">
                  {#if s.url}<a href={s.url} target="_blank" rel="noreferrer">{s.name}</a>{:else}{s.name}{/if}
                  {#if !s.active}<span class="tag">{$t('subscriptions.page.paused')}</span>{/if}
                </td>
                <td>{s.platform || '—'}</td>
                <td>{s.category || '—'}</td>
                <td class="num">{fmtMoney(s.price, s.currency)}</td>
                <td>{freqLabel(s.frequency)}</td>
                <td class="num">{fmtMoney(monthlyOf(s), s.currency)}</td>
                <td>{s.active ? fmtDate(nextBillingDate(s)) : '—'}</td>
                <td class="row-actions">
                  <!-- Icon-only: a title is a tooltip, not an accessible name. -->
                  <button class="icon" aria-label={$t('subscriptions.page.edit')} title={$t('subscriptions.page.edit')} onclick={() => openEdit(s)}><Icon name="pencil" size={14} /></button>
                  <button class="icon danger-hover" aria-label={$t('subscriptions.page.delete')} title={$t('subscriptions.page.delete')} onclick={() => del(s)}><Icon name="trash" size={14} /></button>
                </td>
              </tr>
            {/each}
            {#if tableSubs.length === 0}
              <tr><td colspan="8" class="muted empty-row">{$t('subscriptions.page.emptySearch')}</td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('subscriptions.page.delete')}
  message={pendingDelete ? $t('subscriptions.list.confirmDelete', { name: pendingDelete.name }) : ''}
  confirmLabel={$t('subscriptions.page.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
/>

<Modal bind:open={showForm} size="md" title={editing ? $t('subscriptions.page.editSubscription') : $t('subscriptions.page.newSubscription')}>
  <SubscriptionForm
    initial={editing}
    {suggestions}
    onsubmit={save}
    oncancel={() => (showForm = false)}
  />
</Modal>

<style>
  .sk-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .sk-chart {
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
  }
  .subs {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    height: 100%;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
  }
  .title h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .cur-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .top {
    display: grid;
    grid-template-columns: 1fr 240px;
    gap: var(--space-4);
  }
  @media (max-width: 720px) {
    .top {
      grid-template-columns: 1fr;
    }
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .card h3 {
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
    margin-bottom: var(--space-3);
  }
  .chart-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .chart-head h3 {
    margin-bottom: 0;
  }
  .chart-toggles {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    justify-content: flex-end;
  }
  .toggle {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .toggle button {
    background: transparent;
    border: none;
    color: var(--muted);
    padding: 4px 10px;
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .toggle button.active {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .cur {
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-xs);
  }
  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .stat {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .stat .lbl {
    font-size: 0.7rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .stat strong {
    font-size: 1.05rem;
  }
  .stat.big strong {
    font-size: 1.5rem;
  }
  .stat.highlight {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .warn {
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
  }
  .filters input {
    width: 140px;
  }
  .filters input.search {
    flex: 1;
    min-width: 200px;
  }
  .filters .band {
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 8px;
    font-size: var(--text-sm);
  }
  .count-tag {
    font-size: var(--text-xs);
    color: var(--muted);
    margin-left: auto;
  }
  .clear {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 6px 10px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .table-wrap {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow-x: auto;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tr.inactive {
    opacity: 0.55;
  }
  .num {
    text-align: right;
  }
  .strong {
    font-weight: var(--fw-semibold);
  }
  .strong a {
    color: var(--text);
    text-decoration: none;
  }
  .strong a:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  .tag {
    font-size: 0.62rem;
    text-transform: uppercase;
    color: var(--amber);
    margin-left: 6px;
  }
  .row-actions {
    display: flex;
    gap: 4px;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-base);
  }
  /* The header's clickable part is a <button> (keyboard-operable), but it should look
     and hit like the whole cell: inherit the type, fill the box, no chrome. */
  .sortable {
    width: 100%;
    padding: 0;
    background: none;
    border: none;
    font: inherit;
    color: inherit;
    text-align: inherit;
    cursor: pointer;
    user-select: none;
  }
  .sortable:hover {
    color: var(--text);
  }
  .empty-row {
    text-align: center;
    padding: var(--space-6);
  }
</style>
