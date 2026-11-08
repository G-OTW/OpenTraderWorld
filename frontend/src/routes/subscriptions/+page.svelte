<script>
  // Subscription Tracker. Header block (this month, change, three tiles), one chip row
  // (view, search, filters), the chart card, then the list, the month calendar or the
  // "Worth it?" ranking. Money is shown in the module display currency (FX-converted).
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import { subsApi, fmtMoney, monthlyFactor, nextBillingDate, CURRENCIES, FREQUENCIES } from '$lib/modules/subscriptions/api.js';
  import { monthlyOf as myMonthlyOf, isEnded, switchSavings, trialsFirst, needsRating } from '$lib/modules/subscriptions/calc.js';
  import SubsHeadline from '$lib/modules/subscriptions/SubsHeadline.svelte';
  import SubsToolbar from '$lib/modules/subscriptions/SubsToolbar.svelte';
  import SpendPanel from '$lib/modules/subscriptions/SpendPanel.svelte';
  import SubsTable from '$lib/modules/subscriptions/SubsTable.svelte';
  import SubDetail from '$lib/modules/subscriptions/SubDetail.svelte';
  import CalendarView from '$lib/modules/subscriptions/CalendarView.svelte';
  import WorthView from '$lib/modules/subscriptions/WorthView.svelte';
  import RatePanel from '$lib/modules/subscriptions/RatePanel.svelte';
  import BudgetsPanel from '$lib/modules/subscriptions/BudgetsPanel.svelte';
  import SubscriptionForm from '$lib/modules/subscriptions/SubscriptionForm.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { dateKey } from '$lib/format.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import { createFxRates } from '$lib/fx.svelte.js';

  // Rates are loaded once; switching the display currency re-derives from the same map.
  const fx = createFxRates();

  let subs = $state([]);
  let bd = $state(null);
  let settings = $state({ display_currency: 'USD', category_budgets: {}, budget_channels: [] });
  let suggestions = $state({ platforms: [], categories: [], payment_methods: [] });
  // Bumped on every reload so the forecast tab re-reads after an edit.
  let version = $state(0);
  let loading = $state(true);

  // Filters.
  let fPlatform = $state('');
  let fCategory = $state('');

  let showForm = $state(false);
  let editing = $state(null);

  // Below the chart: the table, the month calendar, or the "Worth it?" ranking.
  let listView = $state('list');
  // Lifecycle filter: 'running' (not cancelled), 'ended' (cancelled) or 'all'.
  let fStatus = $state('running');
  let fMethod = $state('');

  // Price-band filter (table-only, client-side, compares raw price in each sub's own currency).
  // null = all. Bands: under 10, 10 to 50, 50 to 200, 200 and up.
  // Labels for these bands are plain numeric ranges (not natural-language), so they render as-is
  // across locales; only the "Any price" placeholder and column headers are keyed.
  const PRICE_BANDS = [
    { id: 'lt10', label: '< 10', test: (p) => p < 10 },
    { id: '10-50', label: '10 to 50', test: (p) => p >= 10 && p < 50 },
    { id: '50-200', label: '50 to 200', test: (p) => p >= 50 && p < 200 },
    { id: 'gte200', label: '200+', test: (p) => p >= 200 }
  ];
  let fBand = $state('');

  const filter = $derived({ platform: fPlatform, category: fCategory });
  const activeFilters = $derived([fPlatform, fCategory, fBand, fMethod].filter(Boolean).length);

  // Table-only search + sort (client-side; does not affect the chart/breakdown above).
  let tSearch = $state('');
  let tSort = $state('name'); // name | platform | category | price | frequency | monthly | next | rating | saves
  let tDir = $state('asc'); // asc | desc
  // List grouping by category (hairline group headers).
  let grouped = $state(false);

  onMount(async () => {
    [settings, suggestions] = await Promise.all([
      subsApi.getSettings(),
      subsApi.suggestions(),
      fx.load()
    ]);
    await reload();
    loading = false;
  });

  async function reload() {
    // Wide window (12 back, 24 fwd) so the chart's year view has full calendar-year buckets.
    [subs, bd] = await Promise.all([
      subsApi.list(filter),
      subsApi.breakdown({ ...filter, months_back: 12, months_fwd: 24 })
    ]);
    version++;
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
    fMethod = '';
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

  // A PATCH replaces the whole row: rebuild the form payload from a listed subscription.
  function payloadOf(sub, patch = {}) {
    return {
      name: sub.name,
      platform: sub.platform,
      url: sub.url,
      price: sub.price,
      currency: sub.currency,
      frequency: sub.frequency,
      category: sub.category,
      started_on: sub.started_on,
      active: sub.active,
      remind_days: sub.remind_days,
      remind_channels: sub.remind_channels ?? [],
      trial_end: sub.trial_end,
      ended_at: sub.ended_at,
      yearly_price: sub.yearly_price,
      payment_method: sub.payment_method,
      card_expiry: sub.card_expiry,
      my_share_pct: sub.my_share_pct ?? 100,
      ...patch
    };
  }

  // Cancel = an end date; the row and its history stay. Delete is kept for mistakes.
  let ending = $state(null);
  let endDate = $state('');
  function openEnd(s) {
    ending = s;
    endDate = dateKey(new Date());
  }
  async function confirmEnd() {
    const s = ending;
    if (!s || !endDate) return;
    try {
      await subsApi.update(s.id, payloadOf(s, { ended_at: endDate }));
      ending = null;
      await reload();
    } catch (e) {
      toast.err(e.message);
    }
  }
  async function resume(s) {
    try {
      await subsApi.update(s.id, payloadOf(s, { ended_at: null }));
      await reload();
    } catch (e) {
      toast.err(e.message);
    }
  }

  let detail = $state(null);
  let showRate = $state(false);
  let showBudgets = $state(false);

  async function exportIcs() {
    try {
      await subsApi.downloadIcs();
    } catch (e) {
      toast.err(e.message);
    }
  }

  // Rating prompt: shown while a running subscription has no rating from the last 90 days.
  const toRate = $derived(subs.filter((s) => needsRating(s)).length);

  // Sum of the category caps (display currency): the calendar tints the days past it.
  const totalBudget = $derived(
    Object.values(settings.category_budgets ?? {}).reduce((n, v) => n + (Number(v) > 0 ? Number(v) : 0), 0)
  );

  async function setCurrency(v) {
    settings = await subsApi.updateSettings({ display_currency: v });
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
  // Each sub's monthly-equivalent in its own currency, at the user's share of the plan.
  const monthlyOf = myMonthlyOf;

  // A billing reminder, seeded from the row: it fires on the next billing date and then
  // repeats on the sub's own cadence. `quarterly` has no reminder equivalent, so it falls
  // back to a one-shot rather than silently firing on the wrong months.
  const REMINDER_FREQ = { weekly: 'weekly', monthly: 'monthly', yearly: 'yearly' };
  function reminderDefaults(s) {
    const due = nextBillingDate(s);
    // The amount actually charged is the price in the sub's OWN currency, not the
    // display-currency conversion shown in the table.
    return {
      name: $t('subscriptions.page.reminderName', {
        name: s.name,
        amount: fmtMoney(s.price, s.currency)
      }),
      kind: 'custom',
      frequency: REMINDER_FREQ[s.frequency] ?? 'once',
      start_date: due ? dateKey(due) : ''
    };
  }

  // Display-currency views. The sub's own currency stays the reference; these are the
  // converted amounts shown in the table. null => no rate, so callers show the native value.
  const inDisplay = (amount, from) => fx.to(amount, from, cur);
  const toDisplay = (a, c) => (c === cur ? a : inDisplay(a, c));

  // Table view: client-side search + sort over the loaded subs.
  const tableSubs = $derived.by(() => {
    const q = tSearch.trim().toLowerCase();
    const band = PRICE_BANDS.find((b) => b.id === fBand);
    const rows = subs.filter((s) => {
      if (fStatus === 'running' && isEnded(s)) return false;
      if (fStatus === 'ended' && !isEnded(s)) return false;
      if (fMethod && (s.payment_method || '') !== fMethod) return false;
      if (band && !band.test(s.price)) return false;
      if (!q) return true;
      return [s.name, s.platform, s.category, s.payment_method].some((v) => (v || '').toLowerCase().includes(q));
    });
    const dir = tDir === 'desc' ? -1 : 1;
    const nextTs = (s) => {
      const d = nextBillingDate(s);
      return d ? d.getTime() : Infinity;
    };
    const savesOf = (s) => switchSavings(s) ?? -Infinity;
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
        case 'saves':
          return savesOf(a) - savesOf(b);
        case 'rating':
          return (a.last_score ?? 0) - (b.last_score ?? 0);
        default:
          return a.name.localeCompare(b.name);
      }
    };
    // Trials about to convert stay on top, soonest first, whatever the sort.
    return trialsFirst([...rows].sort((a, b) => cmp(a, b) * dir));
  });
</script>

<div class="subs">
  <PageHeader title={$t('subscriptions.page.title')} subtitle={$t('subscriptions.page.subtitle', { cur })}>
    {#snippet actions()}
      <span class="cur-select">
        <span class="lbl">{$t('subscriptions.page.display')}</span>
        <Dropdown
          value={cur}
          onpick={setCurrency}
          ariaLabel={$t('subscriptions.page.display')}
          options={CURRENCIES.map((c) => ({ value: c, label: c }))}
        />
      </span>
      <button class="ghost" onclick={() => (showBudgets = true)}><Icon name="target" size={14} /> {$t('subscriptions.budgets.title')}</button>
      <QuickReminderButton title={$t('subscriptions.page.addReminder')} />
      <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('subscriptions.page.addSubscription')}</button>
    {/snippet}
  </PageHeader>

  {#if loading}
    <!-- Header block, chart card, then rows: the skeleton keeps that shape so the page
         does not collapse to a line and reflow when the data lands. -->
    <div class="sk-page" aria-busy="true">
      <Skeleton height="96px" />
      <Skeleton height="300px" />
      <Skeleton rows={5} height="2.4rem" gap="var(--space-2)" />
    </div>
  {:else}
    <SubsHeadline {bd} {subs} {cur} budgets={settings.category_budgets ?? {}} {toDisplay} />

    <SubsToolbar
      bind:view={listView}
      bind:search={tSearch}
      bind:platform={fPlatform}
      bind:category={fCategory}
      bind:status={fStatus}
      bind:method={fMethod}
      bind:band={fBand}
      bands={PRICE_BANDS}
      {suggestions}
      shown={tableSubs.length}
      total={subs.length}
      onclear={clearFilters}
    />

    <SpendPanel {bd} {subs} {cur} {version} />

    {#if toRate > 0}
      <div class="banner">
        <Icon name="star" size={14} />
        <span>{$t('subscriptions.rate.banner', { count: toRate })}</span>
        <button class="btn sm" onclick={() => (showRate = true)}>{$t('subscriptions.rate.rateNow')}</button>
      </div>
    {/if}

    {#if listView === 'calendar'}
      <section class="panel">
        <CalendarView subs={tableSubs} {cur} {toDisplay} budget={totalBudget} onexport={exportIcs} />
      </section>
    {:else if listView === 'worth'}
      <WorthView subs={tableSubs} {cur} {toDisplay} onrate={() => (showRate = true)} />
    {:else if subs.length === 0}
      <section class="panel">
        {#if activeFilters}
          <EmptyState icon="filter" compact title={$t('subscriptions.page.emptyFiltered')} />
        {:else}
          <EmptyState icon="receipt" title={$t('subscriptions.page.emptyTitle')} description={$t('subscriptions.page.emptyBody')}>
            {#snippet action()}
              <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('subscriptions.page.addSubscription')}</button>
            {/snippet}
          </EmptyState>
        {/if}
      </section>
    {:else}
      <div class="list-head">
        <label class="check">
          <input type="checkbox" bind:checked={grouped} />
          <span>{$t('subscriptions.page.groupByCategory')}</span>
        </label>
        <button class="btn sm" onclick={exportIcs} title={$t('subscriptions.page.exportIcsTitle')}>
          <Icon name="download" size={13} />
          {$t('subscriptions.page.exportIcs')}
        </button>
      </div>
      <SubsTable
        rows={tableSubs}
        {cur}
        {toDisplay}
        bind:sort={tSort}
        bind:dir={tDir}
        {grouped}
        {freqLabel}
        {reminderDefaults}
        ondetail={(s) => (detail = s)}
        onedit={openEdit}
        onend={openEnd}
        onresume={resume}
        ondelete={del}
      />
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

<Modal open={!!ending} size="sm" title={$t('subscriptions.page.cancelSub')} onclose={() => (ending = null)}>
  {#if ending}
    <div class="end-form">
      <p class="hint">{$t('subscriptions.page.cancelHint', { name: ending.name })}</p>
      <label class="field">
        <span>{$t('subscriptions.form.endedAt')}</span>
        <input type="date" bind:value={endDate} />
      </label>
      <div class="end-actions">
        <button class="ghost" onclick={() => (ending = null)}>{$t('common.cancel')}</button>
        <button class="primary" disabled={!endDate} onclick={confirmEnd}>{$t('subscriptions.page.cancelSub')}</button>
      </div>
    </div>
  {/if}
</Modal>

<Modal open={!!detail} size="md" title={detail?.name ?? ''} onclose={() => (detail = null)}>
  {#if detail}
    <SubDetail sub={detail} />
  {/if}
</Modal>

<Modal bind:open={showRate} size="md" title={$t('subscriptions.rate.title')}>
  {#if showRate}
    <RatePanel {subs} onrated={reload} onclose={() => (showRate = false)} />
  {/if}
</Modal>

<Modal bind:open={showBudgets} size="md" title={$t('subscriptions.budgets.title')}>
  {#if showBudgets}
    <BudgetsPanel
      {settings}
      categories={suggestions.categories}
      totals={bd?.categories ?? []}
      onsaved={async (s) => {
        settings = s;
        showBudgets = false;
        await reload();
      }}
      oncancel={() => (showBudgets = false)}
    />
  {/if}
</Modal>

<Modal bind:open={showForm} size="md" title={editing ? $t('subscriptions.page.editSubscription') : $t('subscriptions.page.newSubscription')}>
  <SubscriptionForm
    initial={editing}
    {suggestions}
    defaultCurrency={cur}
    onsubmit={save}
    oncancel={() => (showForm = false)}
  />
</Modal>

<style>
  .subs {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
    overflow-y: auto;
  }
  /* A scrolling flex column must not squeeze its children to fit. */
  .subs > :global(*) {
    flex-shrink: 0;
  }
  .subs :global(.page-header) {
    margin-bottom: 0;
  }
  .sk-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .cur-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .lbl {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .panel {
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    font-size: 13px;
  }
  .banner :global(svg) {
    color: var(--accent);
  }
  .banner button {
    margin-left: auto;
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: calc(-1 * var(--space-2));
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
    color: var(--muted);
    cursor: pointer;
  }
  .check input {
    width: 14px;
  }
  .end-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .hint {
    color: var(--dim);
    font-size: 13px;
  }
  .end-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  @media (max-width: 767px) {
    .subs {
      padding: var(--space-4);
      padding-bottom: 80px;
    }
    .subs :global(.page-header .actions) {
      width: 100%;
      flex-wrap: wrap;
    }
    .subs :global(.page-header .actions button) {
      min-height: 44px;
    }
    .check {
      min-height: 44px;
    }
  }
</style>
