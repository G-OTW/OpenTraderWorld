<script>
  // MyWealth module. Assets tab: the headline block (net worth, change over the period, the
  // curve, the tiles), then the composition and the analysis panels (movers, then versus
  // now, liquidity, targets), then the balance sheet. Goals, Settings and Templates are
  // their own tabs. Money converts into the display currency.
  //
  // One filter row scopes the page: the period chips drive the curve and the movers and
  // then-versus-now panels; type, category and owner drive the breakdown and the table, so
  // the headline always describes the rows under it.
  //
  // Deep links: ?asset=<id> opens an asset, ?new=1 the new-asset form, ?view=goals |
  // settings | templates a tab, ?insight=compare | liquidity | targets a panel.
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import { wealthApi, CURRENCIES, assetTypeLabel } from '$lib/modules/wealth/api.js';
  import HeadlinePanel from '$lib/modules/wealth/HeadlinePanel.svelte';
  import CompositionPanel from '$lib/modules/wealth/CompositionPanel.svelte';
  import AssetsTable from '$lib/modules/wealth/AssetsTable.svelte';
  import TemplatesView from '$lib/modules/wealth/TemplatesView.svelte';
  import AssetForm from '$lib/modules/wealth/AssetForm.svelte';
  import TemplateBuilder from '$lib/modules/wealth/TemplateBuilder.svelte';
  import LinkPortfolioModal from '$lib/modules/wealth/LinkPortfolioModal.svelte';
  import AssetDetail from '$lib/modules/wealth/AssetDetail.svelte';
  import MoversPanel from '$lib/modules/wealth/MoversPanel.svelte';
  import ComparePanel from '$lib/modules/wealth/ComparePanel.svelte';
  import LiquidityPanel from '$lib/modules/wealth/LiquidityPanel.svelte';
  import TargetsPanel from '$lib/modules/wealth/TargetsPanel.svelte';
  import GoalsPanel from '$lib/modules/wealth/GoalsPanel.svelte';
  import SettingsPanel from '$lib/modules/wealth/SettingsPanel.svelte';
  import TaxSendModal from '$lib/modules/wealth/TaxSendModal.svelte';
  import { periodQuery, periodPoints, monthsBack } from '$lib/modules/wealth/engine.js';
  import { identityColor, otherColor } from '$lib/ui/charts/palette.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { ensureInstalled } from '$lib/modules/installed.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { createFxRates } from '$lib/fx.svelte.js';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';

  const VIEWS = ['assets', 'goals', 'settings', 'templates'];
  const INSIGHTS = ['movers', 'compare', 'liquidity', 'targets'];

  let view = $state('assets');
  let assets = $state([]);
  let templates = $state([]);
  let bd = $state(null);
  let settings = $state({ display_currency: 'USD' });
  let loading = $state(true);
  // Every category and owner in use, filter-independent, for the pickers.
  let allCategories = $state([]);
  let allOwners = $state([]);
  // Every asset, unfiltered, for the pair picker and the paired debt lookup.
  let everyAsset = $state([]);
  // Category names ranked by value over the whole book: the colour each one keeps.
  let catRank = $state([]);

  let period = $state('1Y');
  let insight = $state('movers');
  // Bumped after any write so the panels reload what they show.
  let reloadKey = $state(0);
  let menuOpen = $state(false);
  let exportError = $state('');
  let showTax = $state(false);

  let tType = $state([]);
  let tCategory = $state([]);
  let tOwner = $state([]);

  let showAsset = $state(false);
  let editingAsset = $state(null);
  let showImport = $state(false);
  let showUpdate = $state(false);
  let updateAsset = $state(null);
  let updateRevs = $state([]);
  let showTemplate = $state(false);
  let editingTemplate = $state(null);

  const filter = $derived({ asset_type: tType.join(','), category: tCategory.join(','), owner: tOwner.join(',') });
  const unfiltered = $derived(tType.length === 0 && tCategory.length === 0 && tOwner.length === 0);
  const categories = $derived([...new Set(allCategories)].sort((a, b) => a.localeCompare(b)));
  const owners = $derived([...new Set(allOwners)].sort((a, b) => a.localeCompare(b)));
  const cur = $derived(settings.display_currency);
  const fx = createFxRates();
  // Display-currency view of an amount. null => no rate, the caller shows the native amount.
  const inDisplay = (amount, from) => fx.to(amount, from, cur);

  // Colour follows the category, not its rank under the current filter: six named slots
  // from the whole book's ranking, everything past them in the fold colour.
  const c = $derived(chartColors());
  const colorFor = $derived.by(() => {
    const order = new Map(catRank.map((k, i) => [k, i]));
    return (cat) => {
      const i = order.get(cat ?? '');
      return i != null && i < 6 ? identityColor(i, c, 6) : otherColor(c);
    };
  });

  onMount(async () => {
    ensureInstalled();
    const q = page.url.searchParams;
    if (VIEWS.includes(q.get('view'))) view = q.get('view');
    if (INSIGHTS.includes(q.get('insight'))) insight = q.get('insight');
    [settings, templates] = await Promise.all([wealthApi.getSettings(), wealthApi.listTemplates(), fx.load()]);
    await reload();
    loading = false;
    const deep = q.get('asset');
    if (deep) {
      const a = everyAsset.find((x) => x.id === deep);
      if (a) openUpdate(a);
    } else if (q.get('new') === '1') openAddAsset();
  });

  async function fetchBreakdown() {
    const pq = periodQuery(period, dateKey());
    const got = await wealthApi.breakdown({ ...filter, ...pq });
    // Five years of month ends did not reach the start: read the whole history by year.
    if (period === 'ALL' && got.points?.length >= pq.points_back && got.points[0]?.net_worth) {
      return wealthApi.breakdown({ ...filter, granularity: 'year', points_back: 60 });
    }
    return got;
  }

  async function reload() {
    [assets, bd] = await Promise.all([wealthApi.listAssets(filter), fetchBreakdown()]);
    // Learn the picker vocabulary and the colour ranking only from the unfiltered book,
    // never from a subset.
    if (unfiltered) {
      allCategories = assets.map((a) => a.category).filter(Boolean);
      allOwners = assets.map((a) => a.owner).filter(Boolean);
      everyAsset = assets;
      catRank = Object.entries(bd?.by_category ?? {})
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
        .map(([k]) => k);
    }
    reloadKey += 1;
  }
  $effect(() => {
    void filter;
    void period;
    if (!loading) reload();
  });

  // The curve the period shows, and where it starts: the movers and then-versus-now panels
  // read from that same date, so their totals agree with the headline change.
  const points = $derived(periodPoints(bd?.points ?? [], period));
  const lastValued = $derived(assets.reduce((m, a) => (a.latest_at && a.latest_at > m ? a.latest_at : m), '') || null);
  const from = $derived(points[0]?.at ?? monthsBack(dateKey(), 12));

  function tplFor(id) {
    return templates.find((x) => x.id === id) ?? null;
  }

  function openAddAsset() {
    editingAsset = null;
    showAsset = true;
  }
  function openEditAsset(a) {
    editingAsset = a;
    showAsset = true;
  }
  async function saveAsset(payload) {
    if (editingAsset) await wealthApi.updateAsset(editingAsset.id, payload);
    else await wealthApi.addAsset(payload);
    showAsset = false;
    // A category typed here may not be in the vocabulary yet, and `reload` won't learn it
    // while a filter is on (it would be reading a subset).
    if (payload.category) allCategories = [...allCategories, payload.category];
    if (payload.owner) allOwners = [...allOwners, payload.owner];
    await reload();
  }
  function openImport() {
    showAsset = false;
    showImport = true;
  }

  // One dialog, three callers: ConfirmModal instead of the browser's confirm().
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }
  function delAsset(a) {
    askConfirm($t('wealth.page.confirmDeleteAsset', { name: a.name }), async () => {
      await wealthApi.deleteAsset(a.id);
      await reload();
    });
  }

  // Revisions.
  async function refreshRevs() {
    updateRevs = await wealthApi.listRevisions(updateAsset.id);
  }
  // After a write the asset row itself changed (latest value, date): keep the open one fresh.
  function refreshOpenAsset() {
    if (!updateAsset) return;
    updateAsset = assets.find((a) => a.id === updateAsset.id) ?? everyAsset.find((a) => a.id === updateAsset.id) ?? updateAsset;
  }
  async function openUpdate(a) {
    updateAsset = a;
    await refreshRevs();
    showUpdate = true;
  }
  async function saveUpdate(payload) {
    await wealthApi.addRevision(updateAsset.id, payload);
    await refreshRevs();
    await reload();
    refreshOpenAsset();
  }
  async function editRevision(id, payload) {
    await wealthApi.updateRevision(id, payload);
    await refreshRevs();
    await reload();
    refreshOpenAsset();
  }
  function deleteRevision(rev) {
    askConfirm($t('wealth.page.confirmDeleteRevision', { date: rev.valued_at }), async () => {
      await wealthApi.deleteRevision(rev.id);
      await refreshRevs();
      await reload();
      refreshOpenAsset();
    });
  }

  // Templates.
  function openAddTemplate() {
    editingTemplate = null;
    showTemplate = true;
  }
  function openEditTemplate(tpl) {
    if (tpl.is_builtin) return;
    editingTemplate = tpl;
    showTemplate = true;
  }
  async function saveTemplate(payload) {
    if (editingTemplate) await wealthApi.updateTemplate(editingTemplate.id, payload);
    else await wealthApi.addTemplate(payload);
    showTemplate = false;
    templates = await wealthApi.listTemplates();
  }
  function delTemplate(tpl) {
    askConfirm($t('wealth.page.confirmDeleteTemplate', { name: tpl.name }), async () => {
      await wealthApi.deleteTemplate(tpl.id);
      templates = await wealthApi.listTemplates();
    });
  }

  async function setCurrency(v) {
    settings = await wealthApi.updateSettings({ display_currency: v });
    await reload();
  }

  async function exportCsv() {
    menuOpen = false;
    exportError = '';
    try {
      await wealthApi.download('/wealth/export.csv', 'mywealth.csv');
    } catch (e) {
      exportError = e.message;
    }
  }

  const emptyBook = $derived(assets.length === 0 && unfiltered);
  const viewTabs = $derived([
    { id: 'assets', label: $t('wealth.page.assetsTab') },
    { id: 'goals', label: $t('wealth.goals.tab') },
    { id: 'settings', label: $t('wealth.settings.tab') },
    { id: 'templates', label: $t('wealth.page.templatesTab') }
  ]);
  const insightTabs = $derived([
    { id: 'movers', label: $t('wealth.movers.tab') },
    { id: 'compare', label: $t('wealth.compare.tab') },
    { id: 'liquidity', label: $t('wealth.liquidity.tab') },
    { id: 'targets', label: $t('wealth.targets.tab') }
  ]);
</script>

<div class="wealth">
  <PageHeader
    title="MyWealth"
    subtitle={bd ? $t('wealth.page.subtitle', { count: bd.asset_count ?? 0, currency: cur }) : ''}
  >
    {#snippet actions()}
      <div class="cur-select">
        <span>{$t('wealth.page.display')}</span>
        <Dropdown value={cur} onpick={setCurrency} ariaLabel={$t('wealth.page.display')} options={CURRENCIES.map((x) => ({ value: x, label: x }))} />
      </div>
      <QuickReminderButton title={$t('wealth.page.addReminder')} />
      <button class="btn" onclick={() => (showTax = true)} disabled={!bd}><Icon name="receipt" size={14} /> {$t('wealth.tax.button')}</button>
      <div class="menu" use:clickOutside={() => (menuOpen = false)}>
        <button class="btn icon-btn" aria-haspopup="menu" aria-expanded={menuOpen} aria-label={$t('wealth.page.more')} title={$t('wealth.page.more')} onclick={() => (menuOpen = !menuOpen)}><Icon name="more-horizontal" size={15} /></button>
        {#if menuOpen}
          <div class="menu-pop" role="menu">
            <button role="menuitem" onclick={exportCsv}><Icon name="download" size={14} /> {$t('wealth.export.csv')}</button>
          </div>
        {/if}
      </div>
      {#if view === 'templates'}
        <button class="primary" onclick={openAddTemplate}><Icon name="plus" size={15} /> {$t('wealth.page.newTemplate')}</button>
      {:else if view === 'assets'}
        <button class="primary" onclick={openAddAsset}><Icon name="plus" size={15} /> {$t('wealth.page.addAsset')}</button>
      {/if}
    {/snippet}
    <nav class="view-tabs" aria-label="MyWealth">
      {#each viewTabs as v (v.id)}
        <button class:active={view === v.id} aria-current={view === v.id ? 'page' : undefined} onclick={() => (view = v.id)}>{v.label}</button>
      {/each}
    </nav>
  </PageHeader>

  <ErrorText error={exportError} />

  {#if loading}
    <div aria-busy="true">
      <Skeleton rows={6} height="2.2rem" gap="var(--space-2)" />
    </div>
  {:else if view === 'assets'}
    {#if emptyBook}
      <!-- Only the genuinely empty book gets the empty state. A filter that matches nothing
           still renders the controls, otherwise it hides the very buttons that clear it. -->
      <EmptyState icon="wallet" title={$t('wealth.page.noAssetsTitle')} description={$t('wealth.page.noAssetsBody')}>
        {#snippet action()}
          <button class="primary" onclick={openAddAsset}><Icon name="plus" size={15} /> {$t('wealth.page.addAsset')}</button>
        {/snippet}
      </EmptyState>
    {:else}
      <HeadlinePanel {bd} {points} {lastValued} currency={cur} bind:period {settings} toDisplay={inDisplay} {reloadKey} />

      <div class="mid">
        <CompositionPanel byCategory={bd?.by_category ?? {}} byOwner={bd?.by_owner ?? {}} currency={cur} {colorFor} />
        <section class="box insights">
          <div class="itabs" role="tablist" aria-label={$t('wealth.page.insights')}>
            {#each insightTabs as it (it.id)}
              <button role="tab" aria-selected={insight === it.id} class:active={insight === it.id} onclick={() => (insight = it.id)}>{it.label}</button>
            {/each}
            {#if insight === 'movers' || insight === 'compare'}<span class="i-since">{$t('wealth.headline.since', { date: from })}</span>{/if}
          </div>
          <div class="insight-body">
            {#if insight === 'movers'}
              <MoversPanel currency={cur} {reloadKey} {from} />
            {:else if insight === 'compare'}
              <ComparePanel currency={cur} {reloadKey} {from} />
            {:else if insight === 'liquidity'}
              <LiquidityPanel currency={cur} monthlyExpenses={settings.monthly_expenses} toDisplay={inDisplay} {reloadKey} />
            {:else}
              <TargetsPanel {reloadKey} categories={[...new Set(everyAsset.map((a) => a.category || ''))]} />
            {/if}
          </div>
        </section>
      </div>

      <AssetsTable
        {assets}
        currency={cur}
        toDisplay={inDisplay}
        stale={bd?.stale ?? []}
        {categories}
        {owners}
        {colorFor}
        bind:tType
        bind:tCategory
        bind:tOwner
        onopen={openUpdate}
        onedit={openEditAsset}
        ondelete={delAsset}
      />
    {/if}
  {:else if view === 'goals'}
    <GoalsPanel bind:settings {reloadKey} />
  {:else if view === 'settings'}
    <SettingsPanel bind:settings />
  {:else}
    <TemplatesView {templates} onadd={openAddTemplate} onedit={openEditTemplate} ondelete={delTemplate} />
  {/if}
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('wealth.page.deleteTitle')}
  message={confirmMessage}
  confirmLabel={$t('wealth.page.deleteTitle')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={onConfirmYes}
/>

<Modal bind:open={showAsset} size="lg" title={editingAsset ? $t('wealth.page.editAssetTitle') : $t('wealth.page.newAssetTitle')}>
  {#if !editingAsset}
    <!-- Linking a portfolio needs the Portfolio Tracker, linking a broker account does not:
         the dialog is always offered and its portfolio tab says when there is nothing. -->
    <button type="button" class="import-pf" onclick={openImport}>
      <Icon name="link" size={14} /> {$t('wealth.link.open')}
      <Icon name="chevron-right" size={13} />
    </button>
  {/if}
  <AssetForm initial={editingAsset} {templates} {categories} {owners} assets={everyAsset} defaultCurrency={cur} onsubmit={saveAsset} oncancel={() => (showAsset = false)} />
</Modal>

<LinkPortfolioModal bind:open={showImport} existing={everyAsset} defaultCurrency={cur} onimported={reload} />

<TaxSendModal bind:open={showTax} byCategory={bd?.by_category ?? {}} currency={cur} />

<Modal bind:open={showUpdate} size="lg" title={updateAsset ? $t('wealth.page.updateAssetTitle', { name: updateAsset.name }) : $t('wealth.page.updateTitle')}>
  {#snippet head()}
    {#if updateAsset}
      <div class="d-head">
        <span class="d-eyebrow">{[assetTypeLabel(updateAsset.asset_type), updateAsset.category, updateAsset.owner].filter((x, i, all) => x && all.findIndex((y) => y?.toLowerCase() === x.toLowerCase()) === i).join(' · ')}</span>
        <h3 class="d-name">{updateAsset.name}</h3>
      </div>
    {/if}
  {/snippet}
  {#if updateAsset}
    <AssetDetail
      asset={updateAsset}
      assets={everyAsset}
      revisions={updateRevs}
      template={tplFor(updateAsset.template_id)}
      toDisplay={inDisplay}
      currency={cur}
      onsubmit={saveUpdate}
      onupdate={editRevision}
      ondelete={deleteRevision}
      oncancel={() => (showUpdate = false)}
    />
  {/if}
</Modal>

<Modal bind:open={showTemplate} size="md" title={editingTemplate ? $t('wealth.page.editTemplateTitle') : $t('wealth.page.newTemplateTitle')}>
  <TemplateBuilder initial={editingTemplate} onsubmit={saveTemplate} oncancel={() => (showTemplate = false)} />
</Modal>

<style>
  .wealth {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
    overflow-y: auto;
  }
  .wealth > :global(*) {
    flex-shrink: 0;
  }
  .wealth :global(.page-header) {
    margin-bottom: 0;
  }
  .view-tabs {
    display: flex;
    gap: var(--space-1);
    margin-top: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  .view-tabs button,
  .itabs button {
    background: transparent;
    border: none;
    border-bottom: 1.5px solid transparent;
    border-radius: 0;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    margin-bottom: -1px;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
    transition: color 160ms ease;
  }
  .view-tabs button:hover,
  .itabs button:hover {
    color: var(--text);
  }
  .view-tabs button.active,
  .itabs button.active {
    color: var(--text);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }
  .cur-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .cur-select :global(.dd) {
    text-transform: none;
    letter-spacing: 0;
    min-width: 84px;
  }
  .menu {
    position: relative;
  }
  .icon-btn {
    padding: 0 var(--space-2);
  }
  .menu-pop {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: var(--z-dropdown);
    min-width: 180px;
    display: flex;
    flex-direction: column;
    padding: var(--space-1);
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
  }
  .menu-pop button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
  }
  .menu-pop button:hover {
    background: var(--surface-hover);
  }

  .box {
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    padding: var(--space-4);
    min-width: 0;
  }
  .mid {
    display: grid;
    grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
    gap: var(--space-4);
    align-items: start;
  }
  .insights {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .itabs {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: calc(-1 * var(--space-2)) 0 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .i-since {
    margin-left: auto;
    color: var(--dim);
    font-size: 11px;
    font-family: var(--mono);
  }

  /* "Link a portfolio or a broker account" at the top of the new-asset modal. */
  .import-pf {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    margin-bottom: var(--space-4);
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 160ms ease, color 160ms ease;
  }
  .import-pf :global(svg:last-child) {
    margin-left: auto;
  }
  .import-pf:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .d-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .d-eyebrow {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .d-name {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 1100px) {
    .mid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 767px) {
    .wealth {
      padding: var(--space-4) var(--space-4) 80px;
    }
    .view-tabs,
    .itabs {
      overflow-x: auto;
    }
    .view-tabs button,
    .itabs button,
    .menu-pop button {
      min-height: 44px;
    }
    .i-since {
      display: none;
    }
  }
</style>
