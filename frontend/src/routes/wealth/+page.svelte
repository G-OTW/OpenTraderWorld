<script>
  import Icon from '$lib/ui/Icon.svelte';
  // MyWealth module. Top: net-worth chart + total (filterable by type/category). Below:
  // asset list with each asset's current value and an Update button (new revision). A
  // Templates tab manages asset templates. Money converts into the display currency.
  import { onMount } from 'svelte';
  import {
    wealthApi,
    fmtMoney,
    ASSET_TYPES,
    assetTypeLabel,
    assetTypeIcon,
    CURRENCIES
  } from '$lib/modules/wealth/api.js';
  import NetWorthChart from '$lib/modules/wealth/NetWorthChart.svelte';
  import CategoryBars from '$lib/modules/wealth/CategoryBars.svelte';
  import AssetForm from '$lib/modules/wealth/AssetForm.svelte';
  import UpdateForm from '$lib/modules/wealth/UpdateForm.svelte';
  import TemplateBuilder from '$lib/modules/wealth/TemplateBuilder.svelte';
  import ImportPortfolioModal from '$lib/modules/wealth/ImportPortfolioModal.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { installedIds, ensureInstalled } from '$lib/modules/installed.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { createFxRates } from '$lib/fx.svelte.js';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let view = $state('assets'); // assets | templates
  let assets = $state([]);
  let templates = $state([]);
  let bd = $state(null);
  let settings = $state({ display_currency: 'USD' });
  let loading = $state(true);

  let fType = $state('');
  let fCategory = $state('');

  // Breakdown chart granularity ('month' | 'year') — affects only the chart above.
  let granularity = $state('month');
  // Visual mode for the chart above: 'curve' (net-worth time series) | 'categories' (bars).
  let chartView = $state('curve');
  // Collapsed category groups in the asset list (Set of category keys).
  let collapsed = $state(new Set());

  // Table-only controls (do NOT affect the breakdown above).
  let tSearch = $state('');
  let tType = $state('');
  let tCategory = $state('');
  let tSort = $state('name'); // name | type | category | value | date
  let tDir = $state('asc'); // asc | desc

  let showAsset = $state(false);
  let editingAsset = $state(null);
  let showImport = $state(false);
  let showUpdate = $state(false);
  let updateAsset = $state(null);
  let updateRevs = $state([]);
  let showTemplate = $state(false);
  let editingTemplate = $state(null);

  const filter = $derived({ asset_type: fType, category: fCategory });
  const categories = $derived([...new Set(assets.map((a) => a.category).filter(Boolean))]);
  const cur = $derived(settings.display_currency);
  // Rates load once; switching the display currency re-derives from the same map.
  const fx = createFxRates();
  // Display-currency view of an asset's latest value. null => no rate, show the native amount.
  const inDisplay = (amount, from) => fx.to(amount, from, cur);
  // Portfolio-tracker import is offered only when that module is installed.
  const canImport = $derived($installedIds?.has('portfolios') ?? false);

  onMount(async () => {
    ensureInstalled();
    [settings, templates] = await Promise.all([
      wealthApi.getSettings(),
      wealthApi.listTemplates(),
      fx.load()
    ]);
    await reload();
    loading = false;
  });

  // Breakdown query: top filter + granularity. Yearly samples ~8 year-ends; monthly ~12.
  const bdQuery = $derived({
    ...filter,
    granularity,
    points_back: granularity === 'year' ? 8 : 12
  });

  async function reload() {
    [assets, bd] = await Promise.all([
      wealthApi.listAssets(filter),
      wealthApi.breakdown(bdQuery)
    ]);
  }
  // Re-fetch when the top filter or chart granularity changes. (The table below is derived
  // client-side and unaffected by these.)
  $effect(() => {
    void filter;
    void granularity;
    if (!loading) reload();
  });

  // Table view: client-side filter + sort over the loaded assets (independent of breakdown).
  const tableAssets = $derived.by(() => {
    const q = tSearch.trim().toLowerCase();
    let rows = assets.filter((a) => {
      if (tType && a.asset_type !== tType) return false;
      if (tCategory && (a.category || '') !== tCategory) return false;
      if (q && !a.name.toLowerCase().includes(q) && !(a.category || '').toLowerCase().includes(q))
        return false;
      return true;
    });
    const dir = tDir === 'desc' ? -1 : 1;
    const cmp = (a, b) => {
      switch (tSort) {
        case 'type':
          return assetTypeLabel(a.asset_type).localeCompare(assetTypeLabel(b.asset_type));
        case 'category':
          return (a.category || '').localeCompare(b.category || '');
        case 'value':
          return (a.latest_value ?? -Infinity) - (b.latest_value ?? -Infinity);
        case 'date':
          return String(a.latest_at || '').localeCompare(String(b.latest_at || ''));
        default:
          return a.name.localeCompare(b.name);
      }
    };
    return [...rows].sort((a, b) => cmp(a, b) * dir);
  });

  // Group the (filtered, sorted) table rows by category. By default ranked high→low by group
  // total; but when every group is collapsed, a column-header click reorders the *categories*
  // themselves (top level) instead of the rows inside them — so the header acts on whatever
  // level is visible.
  const grouped = $derived.by(() => {
    const map = new Map();
    for (const a of tableAssets) {
      const key = a.category || '';
      if (!map.has(key)) map.set(key, { key, label: a.category || $t('wealth.page.uncategorized'), rows: [], total: 0 });
      const g = map.get(key);
      g.rows.push(a);
      // Sum in the display currency — the raw values are in each asset's own currency, so
      // adding them unconverted would mislabel a native total as the display currency.
      // No rate => fall back to the native amount (same policy as the row cells).
      const v = a.latest_value ?? 0;
      g.total += inDisplay(v, a.currency) ?? v;
    }
    const groups = [...map.values()];

    // Only reorder groups by the header when the group level is what's on screen (all collapsed).
    if (groupsAllCollapsed) {
      const dir = tDir === 'desc' ? -1 : 1;
      const latestOf = (g) => g.rows.reduce((m, r) => (String(r.latest_at || '') > m ? String(r.latest_at || '') : m), '');
      const cmp = (a, b) => {
        switch (tSort) {
          case 'value':
            return a.total - b.total;
          case 'date':
            return latestOf(a).localeCompare(latestOf(b));
          // 'type'/'category'/'name' all fall back to the category label (the group's identity).
          default:
            return a.label.localeCompare(b.label);
        }
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

  // Distinct category keys currently in the filtered table. Derived independently of `grouped`
  // so `grouped` can consult "are all groups collapsed?" without a reactive cycle.
  const groupKeys = $derived([...new Set(tableAssets.map((a) => a.category || ''))]);
  // True when every visible group is collapsed (drives the expand/collapse-all button and
  // switches the header sort to reorder categories instead of rows).
  const groupsAllCollapsed = $derived(groupKeys.length > 0 && groupKeys.every((k) => collapsed.has(k)));
  const allCollapsed = $derived(groupsAllCollapsed);
  function toggleAll() {
    collapsed = allCollapsed ? new Set() : new Set(groupKeys);
  }

  function sortBy(col) {
    if (tSort === col) {
      tDir = tDir === 'asc' ? 'desc' : 'asc';
    } else {
      tSort = col;
      tDir = col === 'value' || col === 'date' ? 'desc' : 'asc';
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
  function clearTableFilters() {
    tSearch = '';
    tType = '';
    tCategory = '';
  }

  function tplFor(id) {
    return templates.find((t) => t.id === id) ?? null;
  }

  // Assets.
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
    await reload();
  }
  function openImport() {
    showAsset = false;
    showImport = true;
  }
  // One dialog, three callers. ConfirmModal exists to replace the browser's confirm(),
  // which can't be themed and blocks the event loop while it's up.
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

  // Update (revisions).
  async function refreshRevs() {
    updateRevs = await wealthApi.listRevisions(updateAsset.id);
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
  }
  async function editRevision(id, payload) {
    await wealthApi.updateRevision(id, payload);
    await refreshRevs();
    await reload();
  }
  function deleteRevision(rev) {
    askConfirm($t('wealth.page.confirmDeleteRevision', { date: rev.valued_at }), async () => {
      await wealthApi.deleteRevision(rev.id);
      await refreshRevs();
      await reload();
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

  async function setCurrency(e) {
    settings = await wealthApi.updateSettings({ display_currency: e.target.value });
    await reload();
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

<div class="wealth">
  <header class="head">
    <div class="title">
      <h1>MyWealth</h1>
      <nav class="tabs">
        <button class:active={view === 'assets'} onclick={() => (view = 'assets')}>{$t('wealth.page.assetsTab')}</button>
        <button class:active={view === 'templates'} onclick={() => (view = 'templates')}>{$t('wealth.page.templatesTab')}</button>
      </nav>
    </div>
    <div class="head-right">
      <label class="cur-select">
        <span>{$t('wealth.page.display')}</span>
        <select value={cur} onchange={setCurrency}>
          {#each CURRENCIES as c}<option value={c}>{c}</option>{/each}
        </select>
      </label>
      <QuickReminderButton title={$t('wealth.page.addReminder')} />
      {#if view === 'assets'}
        <button class="primary" onclick={openAddAsset}><Icon name="plus" size={15} /> {$t('wealth.page.addAsset')}</button>
      {:else}
        <button class="primary" onclick={openAddTemplate}><Icon name="plus" size={15} /> {$t('wealth.page.newTemplate')}</button>
      {/if}
    </div>
  </header>

  {#if loading}
    <!-- Assets and templates both render as tables; the skeleton is a row stack. -->
    <div class="sk-page" aria-busy="true">
      <Skeleton rows={6} height="2.2rem" gap="var(--space-2)" />
    </div>
  {:else if view === 'assets'}
    <section class="filters">
      <select bind:value={fType}>
        <option value="">{$t('wealth.page.allTypes')}</option>
        {#each ASSET_TYPES as at}<option value={at.id}>{at.icon} {at.label}</option>{/each}
      </select>
      <select bind:value={fCategory}>
        <option value="">{$t('wealth.page.allCategories')}</option>
        {#each categories as c}<option value={c}>{c}</option>{/each}
      </select>
    </section>

    <section class="top">
      <div class="card chart-card">
        <div class="chart-head">
          <h3>{$t('wealth.page.netWorth')} <span class="cur">{$t('wealth.page.inCurrency', { currency: cur })}</span></h3>
          <div class="chart-toggles">
            <div class="toggle" role="group" aria-label={$t('wealth.page.chartViewGroup')}>
              <button class:active={chartView === 'curve'} onclick={() => (chartView = 'curve')}>{$t('wealth.page.curve')}</button>
              <button class:active={chartView === 'categories'} onclick={() => (chartView = 'categories')}>{$t('wealth.page.categories')}</button>
            </div>
            {#if chartView === 'curve'}
              <div class="toggle" role="group" aria-label={$t('wealth.page.chartGranularityGroup')}>
                <button class:active={granularity === 'month'} onclick={() => (granularity = 'month')}>{$t('wealth.page.month')}</button>
                <button class:active={granularity === 'year'} onclick={() => (granularity = 'year')}>{$t('wealth.page.year')}</button>
              </div>
            {/if}
          </div>
        </div>
        {#if chartView === 'curve'}
          <NetWorthChart points={bd?.points ?? []} />
        {:else}
          <CategoryBars byCategory={bd?.by_category ?? {}} currency={cur} />
        {/if}
      </div>
      <div class="rail">
        <div class="stat big">
          <span class="lbl">{$t('wealth.page.netWorth')}</span>
          <strong>{fmtMoney(bd?.net_worth, cur)}</strong>
        </div>
        <div class="stat">
          <span class="lbl">{$t('wealth.page.assets')}</span>
          <strong>{bd?.asset_count ?? 0}</strong>
        </div>
        {#if bd?.unconverted > 0}
          <p class="warn"><Icon name="alert-triangle" size={13} /> {$t('wealth.page.unconvertedWarning', { count: bd.unconverted, currency: cur })}</p>
        {/if}
      </div>
    </section>

    {#if assets.length === 0}
      <EmptyState icon="wallet" title={$t('wealth.page.noAssetsTitle')} description={$t('wealth.page.noAssetsBody')}>
        {#snippet action()}
          <button class="primary" onclick={openAddAsset}><Icon name="plus" size={15} /> {$t('wealth.page.addAsset')}</button>
        {/snippet}
      </EmptyState>
    {:else}
      <section class="table-controls">
        <input class="search" placeholder={$t('wealth.page.searchPlaceholder')} bind:value={tSearch} />
        <select bind:value={tType}>
          <option value="">{$t('wealth.page.allTypes')}</option>
          {#each ASSET_TYPES as at}<option value={at.id}>{at.icon} {at.label}</option>{/each}
        </select>
        <select bind:value={tCategory}>
          <option value="">{$t('wealth.page.allCategories')}</option>
          {#each categories as c}<option value={c}>{c}</option>{/each}
        </select>
        {#if tSearch || tType || tCategory}
          <button class="clear" onclick={clearTableFilters}>{$t('common.clear')}</button>
        {/if}
        <button class="clear" onclick={toggleAll}>{allCollapsed ? $t('wealth.page.expandAll') : $t('wealth.page.collapseAll')}</button>
        <span class="count-tag">{$t('wealth.page.countOfTotal', { count: tableAssets.length, total: assets.length })}</span>
      </section>

      <div class="table-wrap">
        <table class="tbl">
          <colgroup>
            <col class="c-name" />
            <col class="c-type" />
            <col class="c-cat" />
            <col class="c-val" />
            <col class="c-date" />
            <col class="c-act" />
          </colgroup>
          <thead>
            <tr>
              {@render sortHeader('name', $t('wealth.page.colAsset'))}
              {@render sortHeader('type', $t('wealth.page.colType'))}
              {@render sortHeader('category', $t('wealth.page.colCategory'))}
              {@render sortHeader('value', $t('wealth.page.colCurrentValue'), true)}
              {@render sortHeader('date', $t('wealth.page.colAsOf'))}
              <th class="act-th">{$t('wealth.page.colActions')}</th>
            </tr>
          </thead>
          <tbody>
            {#each grouped as g (g.key)}
              <tr class="group-row" onclick={() => toggleGroup(g.key)}>
                <td colspan="3" class="group-head">
                  <span class="caret"><Icon name={collapsed.has(g.key) ? 'chevron-right' : 'chevron-down'} size={13} /></span>
                  <span class="group-name">{g.label}</span>
                  <span class="group-count">{g.rows.length}</span>
                </td>
                <td class="num group-total">{fmtMoney(g.total, cur)}</td>
                <td colspan="2"></td>
              </tr>
              {#if !collapsed.has(g.key)}
                {#each g.rows as a (a.id)}
                  <tr>
                    <td class="strong indent">{a.name}</td>
                    <td>{assetTypeIcon(a.asset_type)} {assetTypeLabel(a.asset_type)}</td>
                    <td>{a.category || '—'}</td>
                    <td class="num">
                      {#if a.latest_value != null}
                        {@const conv = inDisplay(a.latest_value, a.currency)}
                        {#if a.currency !== cur && conv != null}
                          {fmtMoney(conv, cur)}<span class="native">{fmtMoney(a.latest_value, a.currency)}</span>
                        {:else}
                          {fmtMoney(a.latest_value, a.currency)}
                        {/if}
                      {:else}<span class="muted">—</span>{/if}
                    </td>
                    <td class="num date-cell">{a.latest_at || '—'}</td>
                    <td class="row-actions">
                      <button class="upd" onclick={() => openUpdate(a)}>{$t('wealth.page.updateBtn')}</button>
                      <button class="icon" title={$t('wealth.page.editTitle')} onclick={() => openEditAsset(a)}><Icon name="pencil" size={14} /></button>
                      <button class="icon" title={$t('wealth.page.deleteTitle')} onclick={() => delAsset(a)}><Icon name="trash" size={14} /></button>
                    </td>
                  </tr>
                {/each}
              {/if}
            {/each}
            {#if tableAssets.length === 0}
              <tr><td colspan="6" class="muted empty-row">{$t('wealth.page.noAssetsMatch')}</td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    {/if}
  {:else}
    {#if templates.length === 0}
      <EmptyState icon="layers" title={$t('wealth.page.noTemplatesTitle')} description={$t('wealth.page.noTemplatesBody')}>
        {#snippet action()}
          <button class="primary" onclick={openAddTemplate}><Icon name="plus" size={15} /> {$t('wealth.page.newTemplate')}</button>
        {/snippet}
      </EmptyState>
    {:else}
      <div class="tpl-grid">
        {#each templates as tpl (tpl.id)}
          <div class="tcard">
            <div class="tcard-head">
              <span class="tname">{assetTypeIcon(tpl.asset_type)} {tpl.name}</span>
              {#if tpl.is_builtin}<span class="badge">{$t('wealth.page.builtIn')}</span>{/if}
            </div>
            {#if tpl.description}<p class="tdesc">{tpl.description}</p>{/if}
            <p class="count">{$t('wealth.page.fieldsCount', { count: tpl.fields?.length ?? 0, type: assetTypeLabel(tpl.asset_type) })}</p>
            {#if !tpl.is_builtin}
              <div class="tcard-actions">
                <button class="link" onclick={() => openEditTemplate(tpl)}>{$t('wealth.page.editLink')}</button>
                <button class="link danger" onclick={() => delTemplate(tpl)}>{$t('wealth.page.deleteLink')}</button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
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

<Modal bind:open={showAsset} size="md" title={editingAsset ? $t('wealth.page.editAssetTitle') : $t('wealth.page.newAssetTitle')}>
  {#if !editingAsset && canImport}
    <button type="button" class="import-pf" onclick={openImport}>
      <Icon name="download" size={14} /> {$t('wealth.import.fromPortfolio')}
    </button>
  {/if}
  <AssetForm initial={editingAsset} {templates} {categories} defaultCurrency={cur} onsubmit={saveAsset} oncancel={() => (showAsset = false)} />
</Modal>

<ImportPortfolioModal bind:open={showImport} existing={assets} onimported={reload} />

<Modal bind:open={showUpdate} size="md" title={updateAsset ? $t('wealth.page.updateAssetTitle', { name: updateAsset.name }) : $t('wealth.page.updateTitle')}>
  {#if updateAsset}
    <UpdateForm
      asset={updateAsset}
      template={tplFor(updateAsset.template_id)}
      revisions={updateRevs}
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
  .sk-page {
    margin-top: var(--space-4);
  }
  .wealth {
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
    align-items: center;
    gap: var(--space-6);
  }
  .title h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 0.5px solid var(--border);
  }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 1.5px solid transparent;
    color: var(--muted);
    padding: 6px 12px;
    border-radius: 0;
    cursor: pointer;
    font-size: var(--text-base);
    margin-bottom: -1px;
  }
  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
    font-weight: var(--fw-medium);
  }
  .head-right {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .cur-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
  }
  .filters {
    display: flex;
    gap: var(--space-2);
  }
  .top {
    display: grid;
    grid-template-columns: 1fr 220px;
    gap: var(--space-4);
  }
  @media (max-width: 720px) {
    .top {
      grid-template-columns: 1fr;
    }
  }
  .card {
    background: var(--bg);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-4);
  }
  .card h3 {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    margin-bottom: var(--space-3);
  }
  .cur {
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-xs);
    text-transform: none;
  }
  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .stat {
    background: var(--bg);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .stat .lbl {
    font-size: 10px;
    color: var(--dim);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .stat strong {
    font-size: 15px;
    font-weight: var(--fw-normal);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  /* Net worth is a magnitude, not a result — no green/red. */
  .stat.big strong {
    font-size: var(--text-xl);
    color: var(--text);
  }
  .warn {
    font-size: var(--text-xs);
    color: var(--amber-ink);
  }
  .table-wrap {
    border: 0.5px solid var(--border);
    border-radius: 0;
    overflow-x: auto;
  }
  table {
    width: 100%;
    min-width: 720px;
    table-layout: fixed;
    border-collapse: collapse;
    font-size: var(--text-base);
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  .num {
    text-align: right;
  }
  /* Native (stored) amount under the converted one — the FX reference stays visible. */
  .native {
    display: block;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .date-cell {
    color: var(--muted);
  }
  /* Fixed column widths so the actions column never shifts; name flexes. */
  .c-name { width: auto; }
  .c-type { width: 130px; }
  .c-cat { width: 130px; }
  .c-val { width: 130px; }
  .c-date { width: 110px; }
  .c-act { width: 150px; }
  /* Text columns truncate rather than widen the table. */
  td.strong,
  td:nth-child(2),
  td:nth-child(3) {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .act-th {
    text-align: right;
  }
  .row-actions {
    display: flex;
    gap: 6px;
    align-items: center;
    justify-content: flex-end;
  }
  .upd {
    background: transparent;
    color: var(--text);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: 4px 10px;
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .upd:hover {
    background: var(--surface-2);
  }
  .tpl-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .tcard {
    background: var(--bg);
    border: none;
    border-radius: 0;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: background-color var(--dur-fast) var(--ease);
  }
  .tcard:hover {
    background: var(--surface-2);
  }
  .tcard-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .tname {
    font-weight: var(--fw-medium);
  }
  .badge {
    font-size: 0.65rem;
    text-transform: uppercase;
    color: var(--muted);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: 1px 6px;
  }
  .tdesc {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .tcard-actions {
    display: flex;
    gap: var(--space-3);
    margin-top: auto;
  }
  .link {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
  }
  .link:hover {
    color: var(--text);
  }
  .link.danger {
    color: var(--red);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-base);
  }

  .chart-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .chart-head h3 {
    margin-bottom: 0;
  }
  .chart-toggles {
    display: flex;
    gap: var(--space-2);
  }
  .toggle {
    display: inline-flex;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    overflow: hidden;
  }
  .toggle button {
    background: transparent;
    border: none;
    border-left: 0.5px solid var(--border-control);
    color: var(--muted);
    padding: 4px 10px;
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .toggle button:first-child {
    border-left: none;
  }
  .toggle button.active {
    background: var(--surface-2);
    color: var(--text);
  }

  .table-controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--bg);
    border: 0.5px solid var(--border);
  }
  .table-controls .search {
    flex: 1;
    min-width: 180px;
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    padding: 6px 9px;
    font-size: var(--text-sm);
  }
  .clear {
    background: transparent;
    border: 0.5px solid var(--border-control);
    color: var(--muted);
    border-radius: 0;
    padding: 6px 10px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .clear:hover {
    background: var(--surface-2);
  }
  .count-tag {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
    margin-left: auto;
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
  .group-row {
    cursor: pointer;
    background: var(--surface-2);
  }
  .group-row:hover {
    background: var(--surface-2);
  }
  .group-head {
    font-weight: var(--fw-medium);
  }
  .caret {
    color: var(--muted);
    margin-right: var(--space-2);
    font-size: var(--text-xs);
  }
  .group-count {
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-xs);
    font-family: var(--mono);
    margin-left: var(--space-2);
  }
  .group-total {
    font-weight: var(--fw-medium);
  }
  .indent {
    padding-left: var(--space-6);
  }
  /* "Import a whole portfolio" shortcut at the top of the new-asset modal. */
  .import-pf {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    width: 100%;
    background: transparent;
    border: 0.5px dashed var(--border-control);
    border-radius: 0;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    margin-bottom: var(--space-3);
  }
  .import-pf:hover {
    border-color: var(--border-control);
    color: var(--text);
    background: var(--surface-2);
  }
</style>
