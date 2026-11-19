<script>
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  // One portfolio's full-page detail: a header with grouped actions (back, currency dropdown,
  // auto-update, add asset, refresh), the headline block (value, period change, chips, the
  // value chart) beside the allocation, a row of figure tiles, then a single tabbed panel
  // switching between the Assets table, the Operations ledger, income and analysis, each
  // with its own search box and per-column sorting. Prices come from the server in the
  // portfolio's display currency; changing the currency dropdown re-prices via a PATCH + reload.
  import Modal from '$lib/ui/Modal.svelte';
  import AddAssetModal from './AddAssetModal.svelte';
  import ImportModal from './ImportModal.svelte';
  import BrokerHoldingsModal from './BrokerHoldingsModal.svelte';
  import ReconcileModal from './ReconcileModal.svelte';
  import CashOpModal from './CashOpModal.svelte';
  import TargetsModal from './TargetsModal.svelte';
  import MeasureModal from './MeasureModal.svelte';
  import AssetDetailModal from './AssetDetailModal.svelte';
  import IncomePanel from './IncomePanel.svelte';
  import PortfolioSettingsModal from './PortfolioSettingsModal.svelte';
  import AnalyticsView from './analytics/AnalyticsView.svelte';
  import HeadlineBlock from './HeadlineBlock.svelte';
  import AllocationPanel from './AllocationPanel.svelte';
  import {
    portfoliosApi,
    fmtMoney,
    fmtSignedMoney,
    fmtNum,
    unrealizedPct,
    na,
    opKind,
    CURRENCIES
  } from './api.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { fmtDate } from '$lib/format.js';

  let {
    id,
    openBroker = $bindable(false),
    onback = () => {},
    onchanged = () => {}
  } = $props();

  let data = $state(null);
  let loading = $state(false);
  let error = $state('');
  let refreshing = $state(false);
  let showAddAsset = $state(false);
  let showImport = $state(false);
  let showBroker = $state(false);
  // Open the broker import on arrival once the portfolio is loaded (a background read's
  // notification links here).
  $effect(() => {
    if (!openBroker || !data) return;
    showBroker = true;
    openBroker = false;
  });
  let showReconcile = $state(false);
  let showCash = $state(false);
  let showTargets = $state(false);
  let showMeasure = $state(false);
  let showSettings = $state(false);
  // The holding open in the detail modal (thesis, lots, plans, history, transfer).
  let assetFor = $state(null);
  const assetOpen = $derived(data?.assets?.find((a) => a.id === assetFor) ?? null);
  const overLimit = (assetId) => (data?.concentration ?? []).includes(assetId);
  // Bumped after any write, so the analysis tab drops its cached blocks: the ledger is
  // upstream of every one of them.
  let analyticsKey = $state(0);

  /** Route an "unavailable" block's fix button to the screen that fixes it. */
  function fixFrom(what) {
    if (what === 'targets') showTargets = true;
    else showMeasure = true;
  }
  // Info popover next to "Refresh prices": where the quotes come from + why not to hammer it.
  let showSourcesInfo = $state(false);
  let infoRoot = $state(null);

  function onInfoDocClick(e) {
    if (infoRoot && !infoRoot.contains(e.target)) showSourcesInfo = false;
  }

  // Inline operation form state, keyed by asset id when open.
  let opFor = $state(null);
  let opForm = $state({ side: 'buy', quantity: '', price: '', fee: '', op_date: '', note: '' });
  // Operations are entered in the asset's own currency, not the portfolio's display one.
  // AssetView flattens the asset, so its fields sit at the top level of each row.
  const opCurrency = $derived(data?.assets?.find((a) => a.id === opFor)?.currency ?? 'USD');

  // Which table tab is showing.
  let tab = $state('assets'); // 'assets' | 'operations' | 'income' | 'analysis'

  // Independent search boxes per tab.
  let assetSearch = $state('');
  let opSearch = $state('');

  // Sort state per tab: { col, dir }. dir: 1 asc, -1 desc.
  let assetSortState = $state({ col: 'symbol', dir: 1 });
  let opSortState = $state({ col: 'op_date', dir: -1 });
  // Passed to the sortHeader snippet, which is shared by both tables.
  const setAssetSort = (s) => (assetSortState = s);
  const setOpSort = (s) => (opSortState = s);

  let ccy = $derived(data?.portfolio?.currency ?? 'USD');

  $effect(() => {
    if (!id) return;
    load();
  });

  function load() {
    loading = true;
    error = '';
    portfoliosApi
      .detail(id)
      .then((r) => (data = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  }

  async function refresh() {
    refreshing = true;
    error = '';
    try {
      await portfoliosApi.refresh(id);
      load();
      onchanged();
      toast.ok($t('portfolios.detail.refreshDone'));
    } catch (e) {
      // The 1-min cooldown is an expected nudge, not a failure: warn and leave the page as-is.
      if (e.status === 429) {
        toast.warn($t('portfolios.detail.refreshCooldown'));
      } else {
        error = e.message;
      }
    } finally {
      refreshing = false;
    }
  }

  // Turning auto-refresh OFF is immediate. Turning it ON first reconciles every asset against its
  // price source: if all resolve we enable directly; if any is unresolved we open the modal so the
  // user can fix or opt-out those assets before enabling.
  let autoBusy = $state(false);
  async function toggleAuto(e) {
    const wantOn = e.target.checked;
    if (!wantOn) {
      try {
        await portfoliosApi.update(id, { auto_refresh: false });
        load();
        onchanged();
      } catch (err) {
        error = err.message;
      }
      return;
    }
    // Keep the checkbox reflecting server truth until we actually enable.
    e.target.checked = false;
    if (!data?.assets?.length) {
      showReconcile = true;
      return;
    }
    autoBusy = true;
    error = '';
    try {
      const r = await portfoliosApi.reconcile(id);
      if ((r.unresolved ?? 0) === 0) {
        await portfoliosApi.update(id, { auto_refresh: true });
        load();
        onchanged();
      } else {
        showReconcile = true;
      }
    } catch (err) {
      error = err.message;
    } finally {
      autoBusy = false;
    }
  }

  async function changeCurrency(currency) {
    try {
      await portfoliosApi.update(id, { currency });
      load(); // re-priced in the new currency server-side
      onchanged();
    } catch (err) {
      error = err.message;
    }
  }

  // Inline rename of the portfolio (click the pencil, Enter/blur saves, Esc cancels).
  let renaming = $state(false);
  let renameVal = $state('');
  function startRename() {
    renameVal = data?.portfolio?.name ?? '';
    renaming = true;
  }
  async function commitRename() {
    renaming = false;
    const name = renameVal.trim();
    if (!name || name === data?.portfolio?.name) return;
    try {
      await portfoliosApi.update(id, { name });
      load();
      onchanged();
    } catch (err) {
      error = err.message;
    }
  }

  // Same gesture for the one-line description: click it, Enter/blur saves, Esc cancels.
  // An empty description is a valid state, so the placeholder line stays clickable.
  let editingDesc = $state(false);
  let descVal = $state('');
  function startDesc() {
    descVal = data?.portfolio?.description ?? '';
    editingDesc = true;
  }
  async function commitDesc() {
    editingDesc = false;
    const description = descVal.trim();
    if (description === (data?.portfolio?.description ?? '')) return;
    try {
      await portfoliosApi.update(id, { description });
      load();
      onchanged();
    } catch (err) {
      error = err.message;
    }
  }

  // The long note: why this book exists, what it is meant to do. Folded away by default
  // once written; saved on blur, so it behaves like a document, not a form.
  let notesOpen = $state(false);
  let notesVal = $state('');
  let notesSaving = $state(false);
  let notesDirty = $state(false);

  function toggleNotes() {
    notesOpen = !notesOpen;
    if (notesOpen) {
      notesVal = data?.portfolio?.notes ?? '';
      notesDirty = false;
    }
  }

  async function saveNotes() {
    if (!notesDirty) return;
    notesSaving = true;
    try {
      await portfoliosApi.update(id, { notes: notesVal });
      notesDirty = false;
      load();
    } catch (err) {
      error = err.message;
    } finally {
      notesSaving = false;
    }
  }

  function openOp(assetId, side = 'buy', price = '', quantity = '') {
    opFor = assetId;
    opForm = { side, quantity: quantity ?? '', price: price ?? '', fee: '', op_date: today(), note: '' };
  }
  function today() {
    const d = new Date(); // local date: toISOString() is UTC and shifts near midnight
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }

  async function submitOp() {
    try {
      await portfoliosApi.addOperation(opFor, {
        side: opForm.side,
        quantity: Number(opForm.quantity),
        price: Number(opForm.price),
        fee: Number(opForm.fee) || 0,
        op_date: opForm.op_date || today(),
        note: opForm.note
      });
      opFor = null;
      load();
      onchanged();
    } catch (e) {
      error = e.message;
    }
  }

  // One dialog, two callers. Deleting an asset also deletes its operations, and
  // neither is undoable: so both go through a real confirmation rather than the
  // browser's confirm(), which ConfirmModal exists to replace.
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});

  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  function deleteAsset(assetId) {
    askConfirm($t('portfolios.detail.confirmDeleteAsset'), async () => {
      await portfoliosApi.deleteAsset(assetId);
      load();
      onchanged();
    });
  }

  function deleteOp(opId) {
    askConfirm($t('portfolios.detail.confirmDeleteOp'), async () => {
      await portfoliosApi.deleteOperation(opId);
      load();
      onchanged();
    });
  }

  // Click a column header: toggle direction if same col, else sort asc on the new col.
  function sortBy(state, col) {
    if (state.col === col) return { col, dir: -state.dir };
    return { col, dir: 1 };
  }
  /** Arrow icon for the sorted column, or null. */
  function arrow(state, col) {
    if (state.col !== col) return null;
    return state.dir === 1 ? 'arrow-up' : 'arrow-down';
  }
  /** What a screen reader announces for a sortable column header. */
  function ariaSort(state, col) {
    if (state.col !== col) return 'none';
    return state.dir === 1 ? 'ascending' : 'descending';
  }
  function cmp(a, b, dir) {
    if (a == null) return 1;
    if (b == null) return -1;
    if (typeof a === 'string') return a.localeCompare(b) * dir;
    return (a - b) * dir;
  }

  let filteredAssets = $derived.by(() => {
    // Weight = share of total market value; added up-front so it sorts like any column.
    const totalMv = data?.summary?.market_value || 0;
    let list = (data?.assets ?? []).map((a) => ({
      ...a,
      weight: totalMv > 0 && a.market_value != null ? (a.market_value / totalMv) * 100 : null
    }));
    const f = assetSearch.trim().toLowerCase();
    if (f) {
      list = list.filter(
        (a) => a.symbol.toLowerCase().includes(f) || (a.name ?? '').toLowerCase().includes(f)
      );
    }
    const { col, dir } = assetSortState;
    return [...list].sort((a, b) => cmp(a[col], b[col], dir));
  });

  let filteredOps = $derived.by(() => {
    let ops = data?.operations ?? [];
    const f = opSearch.trim().toLowerCase();
    if (f) {
      ops = ops.filter(
        (o) =>
          o.symbol.toLowerCase().includes(f) ||
          o.operation.side.includes(f) ||
          (o.operation.note ?? '').toLowerCase().includes(f)
      );
    }
    const { col, dir } = opSortState;
    const get = (o) => (col === 'symbol' ? o.symbol : o.operation[col]);
    return [...ops].sort((a, b) => cmp(get(a), get(b), dir));
  });

  // Unrealized over cost: the percent carries the same sign as the money beside it.
  let totalGain = $derived(data ? unrealizedPct(data.summary.unrealized, data.summary.cost_basis) : null);

  // Best and worst holding by unrealized result, for the last tile.
  let bestAsset = $derived.by(() => {
    const withPnl = (data?.assets ?? []).filter((a) => a.unrealized != null);
    if (!withPnl.length) return null;
    return withPnl.reduce((m, a) => (a.unrealized > m.unrealized ? a : m));
  });
  let worstAsset = $derived.by(() => {
    const withPnl = (data?.assets ?? []).filter((a) => a.unrealized != null);
    if (!withPnl.length) return null;
    return withPnl.reduce((m, a) => (a.unrealized < m.unrealized ? a : m));
  });

  // Assets excluded from auto-refresh (unresolved source, or user-flagged manual).
  let attentionCount = $derived(
    (data?.assets ?? []).filter((a) => a.recon_status === 'unresolved' || a.recon_status === 'manual')
      .length
  );
</script>

<svelte:document onclick={onInfoDocClick} onkeydown={(e) => e.key === 'Escape' && (showSourcesInfo = false)} />

<!-- A sortable column header. The clickable part is a real <button>, so the column
     is reachable by Tab and operable by Enter/Space; aria-sort announces the current
     direction. A bare onclick on the <th> looked identical and did neither.
     `left` left-aligns text columns; numeric ones stay right-aligned via .num. -->
{#snippet sortHeader(state, col, label, setState, left = false)}
  {@const dir = arrow(state, col)}
  <th class:l={left} class:num={!left} aria-sort={ariaSort(state, col)}>
    <button class="sortable" class:on={!!dir} onclick={() => setState(sortBy(state, col))}>
      <span>{label}</span>
      <span class="sarrow" aria-hidden="true"><Icon name={dir ?? 'arrow-up'} size={11} /></span>
    </button>
  </th>
{/snippet}

<div class="detail">
  <header class="head">
    <Button variant="ghost" icon="arrow-left" onclick={onback} aria-label={$t('portfolios.detail.backAria')}>{$t('portfolios.detail.back')}</Button>
    <div class="title">
      {#if renaming}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="rename"
          bind:value={renameVal}
          autofocus
          onblur={commitRename}
          onkeydown={(e) => {
            if (e.key === 'Enter') commitRename();
            if (e.key === 'Escape') renaming = false;
          }}
        />
      {:else}
        <h1>{data?.portfolio?.name ?? $t('portfolios.detail.defaultName')}</h1>
        <button class="icon" onclick={startRename} disabled={!data} aria-label={$t('portfolios.detail.renameAria')} title={$t('portfolios.detail.renameAria')}><Icon name="pencil" size={14} /></button>
      {/if}
    </div>
    <div class="actions">
      <div class="ccysel">
        <span>{$t('portfolios.detail.currency')}</span>
        <Dropdown
          value={ccy}
          onpick={changeCurrency}
          disabled={!data}
          ariaLabel={$t('portfolios.detail.currency')}
          options={CURRENCIES.map((c) => ({ value: c, label: c }))}
        />
      </div>
      <label class="auto">
        <input
          type="checkbox"
          checked={data?.portfolio?.auto_refresh ?? false}
          onchange={toggleAuto}
          disabled={!data || autoBusy}
        />
        {autoBusy ? $t('portfolios.detail.checking') : $t('portfolios.detail.dailyAutoUpdate')}
      </label>
      {#if data?.portfolio?.auto_refresh}
        <Button variant="ghost" icon="settings" onclick={() => (showReconcile = true)} disabled={!data}>
          {$t('portfolios.detail.sources')}
          {#if attentionCount > 0}<span class="attn">{attentionCount}</span>{/if}
        </Button>
      {/if}
      <Button variant="ghost" icon="bell" onclick={() => (showSettings = true)} disabled={!data} title={$t('portfolios.settings.title')}>
        {$t('portfolios.settings.button')}
      </Button>
      <Button icon="refresh-cw" onclick={refresh} loading={refreshing} disabled={!data}>
        {refreshing ? $t('portfolios.detail.refreshing') : $t('portfolios.detail.refreshPrices')}
      </Button>
      <Button variant="primary" icon="plus" onclick={() => (showAddAsset = true)} disabled={!data}>{$t('portfolios.detail.addAsset')}</Button>
      <div class="infowrap" bind:this={infoRoot}>
        <button
          class="infobtn"
          class:on={showSourcesInfo}
          onclick={() => (showSourcesInfo = !showSourcesInfo)}
          aria-label={$t('portfolios.detail.sourcesInfo.aria')}
          aria-expanded={showSourcesInfo}
        >
          <Icon name="info" size={14} />
        </button>
        {#if showSourcesInfo}
          <!-- Click-away closes; the wrapper stops the bubble so clicks inside stay open. -->
          <div class="infopop" role="dialog" aria-label={$t('portfolios.detail.sourcesInfo.aria')}>
            <p class="ititle">{$t('portfolios.detail.sourcesInfo.title')}</p>
            <ul>
              <li><b>CoinGecko</b>: {$t('portfolios.detail.sourcesInfo.coingecko')}</li>
              <li><b>Yahoo Finance</b>: {$t('portfolios.detail.sourcesInfo.yahoo')}</li>
              <li><b>Binance · Kraken · Coinbase</b>: {$t('portfolios.detail.sourcesInfo.exchanges')}</li>
            </ul>
            <p class="ititle">{$t('portfolios.detail.sourcesInfo.limitsTitle')}</p>
            <p>{$t('portfolios.detail.sourcesInfo.limits')}</p>
            <p class="warn">{$t('portfolios.detail.sourcesInfo.warn')}</p>
          </div>
        {/if}
      </div>
    </div>
  </header>

  {#if loading && !data}
    <!-- Hold the headline + chart + tiles shape rather than collapsing to one line. -->
    <div class="overview">
      <div class="card"><Skeleton height="320px" /></div>
      <div class="card"><Skeleton height="320px" /></div>
    </div>
    <div class="tiles">
      {#each Array(6) as _, i (i)}<Skeleton height="64px" />{/each}
    </div>
  {:else if error && !data}
    <ErrorText {error} copyable />
  {:else if data}
    <div class="about">
      {#if editingDesc}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="descedit"
          bind:value={descVal}
          autofocus
          placeholder={$t('portfolios.detail.descPlaceholder')}
          onblur={commitDesc}
          onkeydown={(e) => {
            if (e.key === 'Enter') commitDesc();
            if (e.key === 'Escape') editingDesc = false;
          }}
        />
      {:else}
        <button class="desc" onclick={startDesc} title={$t('portfolios.detail.descEdit')}>
          {#if data.portfolio.description}
            {data.portfolio.description}
          {:else}
            <span class="dim">{$t('portfolios.detail.descPlaceholder')}</span>
          {/if}
          <Icon name="pencil" size={12} />
        </button>
      {/if}
      <button class="notestoggle" class:on={notesOpen} onclick={toggleNotes}>
        <Icon name={notesOpen ? 'chevron-down' : 'chevron-right'} size={13} />
        {$t('portfolios.detail.notes')}
        {#if !notesOpen && data.portfolio.notes}<span class="dot" aria-hidden="true"></span>{/if}
      </button>
    </div>

    {#if notesOpen}
      <div class="notes">
        <textarea
          bind:value={notesVal}
          oninput={() => (notesDirty = true)}
          onblur={saveNotes}
          rows="8"
          placeholder={$t('portfolios.detail.notesPlaceholder')}
        ></textarea>
        <p class="noteshint dim">
          {notesSaving
            ? $t('common.saving')
            : notesDirty
              ? $t('portfolios.detail.notesUnsaved')
              : $t('portfolios.detail.notesHint')}
        </p>
      </div>
    {/if}

    <div class="overview">
      <div class="card">
        <HeadlineBlock snapshots={data.snapshots} value={data.summary.market_value} currency={ccy} />
      </div>
      <div class="card">
        <AllocationPanel assets={data.assets} currency={ccy} exposure={data.exposure} />
      </div>
    </div>

    <!-- Balances (invested, cash) carry no sign; results (unrealized, realized) do. The
         last tile names the best and worst holding; its sign rides on the glyph. -->
    <div class="tiles">
      <StatCard
        label={$t('portfolios.analytics.book.invested')}
        value={fmtMoney(data.summary.cost_basis, ccy)}
        hint={$t('portfolios.card.assetCount', { count: data.assets.length })}
      />
      <StatCard
        label={$t('portfolios.analytics.book.cash')}
        value={data.summary.cash_tracked ? fmtMoney(data.summary.cash_total, ccy) : na()}
        hint={data.summary.cash_tracked ? '' : $t('portfolios.headline.cashUntracked')}
      />
      <StatCard
        label={$t('portfolios.detail.unrealized')}
        value={fmtSignedMoney(data.summary.unrealized, ccy)}
        delta={totalGain == null ? null : Number(totalGain.toFixed(2))}
      />
      <StatCard label={$t('portfolios.detail.realized')} value={fmtSignedMoney(data.summary.realized, ccy)} />
      <StatCard label={$t('portfolios.analytics.book.income')} value={fmtMoney(data.summary.income, ccy)} />
      <StatCard
        label={$t('portfolios.headline.bestWorst')}
        value={bestAsset ? `${bestAsset.symbol} / ${worstAsset.symbol}` : na()}
        hint={bestAsset ? `${fmtSignedMoney(bestAsset.unrealized, ccy)} / ${fmtSignedMoney(worstAsset.unrealized, ccy)}` : ''}
      />
    </div>

    <ErrorText error={error} copyable />

    <div class="tabs">
      <button class:active={tab === 'assets'} onclick={() => (tab = 'assets')}>
        {$t('portfolios.detail.tabAssets')} <span class="count">{data.assets.length}</span>
      </button>
      <button class:active={tab === 'operations'} onclick={() => (tab = 'operations')}>
        {$t('portfolios.detail.tabOperations')} <span class="count">{data.operations.length}</span>
      </button>
      <button class:active={tab === 'income'} onclick={() => (tab = 'income')}>
        {$t('portfolios.detail.tabIncome')}
      </button>
      <button class:active={tab === 'analysis'} onclick={() => (tab = 'analysis')}>
        {$t('portfolios.detail.tabAnalysis')}
      </button>
      <div class="tab-tools">
        {#if tab === 'assets'}
          <label class="filter">
            <Icon name="search" size={13} />
            <input type="search" placeholder={$t('portfolios.detail.searchAssets')} bind:value={assetSearch} />
          </label>
          <Button size="sm" icon="upload" onclick={() => (showImport = true)} disabled={!data}>
            {$t('portfolios.detail.import')}
          </Button>
          <Button size="sm" icon="download" onclick={() => (showBroker = true)} disabled={!data}>
            {$t('portfolios.detail.fromBroker')}
          </Button>
        {:else if tab === 'operations'}
          <label class="filter">
            <Icon name="search" size={13} />
            <input type="search" placeholder={$t('portfolios.detail.searchOperations')} bind:value={opSearch} />
          </label>
          <!-- Cash, income and costs are the ledger too. Booking them anywhere else would
               make them feel like settings rather than what the book actually did. -->
          <Button size="sm" icon="plus" onclick={() => (showCash = true)}>{$t('portfolios.detail.addCash')}</Button>
        {/if}
      </div>
    </div>

    {#if tab === 'income'}
      <IncomePanel portfolioId={id} currency={ccy} reloadKey={analyticsKey} onasset={(aid) => (assetFor = aid)} />
    {:else if tab === 'analysis'}
      <AnalyticsView
        portfolio={data.portfolio}
        currency={ccy}
        onfix={fixFrom}
        reloadKey={analyticsKey}
      />
    {:else if tab === 'assets'}
      <div class="table-wrap cards">
        <table class="tbl">
          <thead>
            <tr>
              {@render sortHeader(assetSortState, 'symbol', $t('portfolios.detail.colSymbol'), setAssetSort, true)}
              {@render sortHeader(assetSortState, 'quantity', $t('portfolios.detail.colQty'), setAssetSort)}
              {@render sortHeader(assetSortState, 'avg_cost', $t('portfolios.detail.colAvgCost'), setAssetSort)}
              {@render sortHeader(assetSortState, 'price', $t('portfolios.detail.colPrice'), setAssetSort)}
              {@render sortHeader(assetSortState, 'market_value', $t('portfolios.detail.colValue'), setAssetSort)}
              {@render sortHeader(assetSortState, 'weight', $t('portfolios.detail.colWeight'), setAssetSort)}
              {@render sortHeader(assetSortState, 'unrealized', $t('portfolios.detail.colUnreal'), setAssetSort)}
              {@render sortHeader(assetSortState, 'realized', $t('portfolios.detail.colRealized'), setAssetSort)}
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredAssets as a (a.id)}
              <tr>
                <td class="l symcell">
                  <span class="symline">
                    <button class="sym symbtn" onclick={() => (assetFor = a.id)} title={$t('portfolios.asset.open')}>{a.symbol}</button>
                    {#if overLimit(a.id)}
                      <Badge tone="warn" title={$t('portfolios.concentration.hint')}>{$t('portfolios.concentration.badge')}</Badge>
                    {/if}
                    {#if a.recon_status === 'unresolved'}
                      <Badge tone="warn" title={$t('portfolios.detail.unresolvedHint')}>{$t('portfolios.reconcile.status.unresolved')}</Badge>
                    {:else if a.recon_status === 'manual'}
                      <Badge title={$t('portfolios.detail.manualHint')}>{$t('portfolios.reconcile.status.manual')}</Badge>
                    {:else if a.spot_provider}
                      <Badge title={$t('portfolios.detail.pricedViaHint', { source: a.spot_provider })}>{a.spot_provider}</Badge>
                    {/if}
                    <span class="aname" title={a.name}>{a.name}</span>
                  </span>
                </td>
                <td class="num" data-label={$t('portfolios.detail.colQty')}>{fmtNum(a.quantity)}</td>
                <td class="num" data-label={$t('portfolios.detail.colAvgCost')}>{fmtMoney(a.avg_cost, ccy)}</td>
                <td class="num" data-label={$t('portfolios.detail.colPrice')}>{fmtMoney(a.price, ccy)}</td>
                <td class="num" data-label={$t('portfolios.detail.colValue')}>{fmtMoney(a.market_value, ccy)}</td>
                <td class="num" data-label={$t('portfolios.detail.colWeight')}>
                  {#if a.weight != null}
                    <span class="wcell">
                      <span class="wbar"><span class="wfill" style="width:{Math.min(a.weight, 100)}%"></span></span>
                      <span class="wv">{a.weight.toFixed(1)}%</span>
                    </span>
                  {:else}<span class="dim">{na()}</span>{/if}
                </td>
                <!-- PnL columns: sign in the text, color as the second channel. -->
                <td class="num" data-label={$t('portfolios.detail.colUnreal')} class:up={a.unrealized > 0} class:down={a.unrealized < 0}>
                  {fmtSignedMoney(a.unrealized, ccy)}
                </td>
                <td class="num" data-label={$t('portfolios.detail.colRealized')} class:up={a.realized > 0} class:down={a.realized < 0}>
                  {fmtSignedMoney(a.realized, ccy)}
                </td>
                <td class="ops">
                  <span class="rowacts">
                    <button
                      class="icon"
                      onclick={() => openOp(a.id, 'buy')}
                      title={$t('portfolios.detail.newOperation')}
                      aria-label={$t('portfolios.detail.newOperation')}><Icon name="plus" size={14} /></button
                    >
                    <button class="icon danger-hover" onclick={() => deleteAsset(a.id)} aria-label={$t('common.delete')} title={$t('common.delete')}><Icon name="trash" size={14} /></button>
                  </span>
                </td>
              </tr>
            {/each}
            {#if filteredAssets.length === 0}
              <tr><td colspan="9" class="empty">
                {data.assets.length === 0 ? $t('portfolios.detail.noAssetsYet') : $t('portfolios.detail.noAssetsMatch')}
              </td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="table-wrap cards">
        <table class="tbl">
          <thead>
            <tr>
              {@render sortHeader(opSortState, 'op_date', $t('portfolios.detail.colDate'), setOpSort, true)}
              {@render sortHeader(opSortState, 'symbol', $t('portfolios.detail.colSymbol'), setOpSort, true)}
              {@render sortHeader(opSortState, 'side', $t('portfolios.detail.colSide'), setOpSort, true)}
              {@render sortHeader(opSortState, 'quantity', $t('portfolios.detail.colQty'), setOpSort)}
              {@render sortHeader(opSortState, 'price', $t('portfolios.detail.colPrice'), setOpSort)}
              {@render sortHeader(opSortState, 'fee', $t('portfolios.detail.colFee'), setOpSort)}
              <th class="l">{$t('portfolios.detail.colNote')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredOps as o (o.operation.id)}
              <tr>
                <td class="l num" data-label={$t('portfolios.detail.colDate')}>{fmtDate(o.operation.op_date)}</td>
                <!-- A cash row (deposit, fee on the book) names no asset: it is the book's cash. -->
                <td class="l sym" data-label={$t('portfolios.detail.colSymbol')}>
                  {#if o.symbol}{o.symbol}{:else}<span class="dim">{$t('portfolios.class.cash')}</span>{/if}
                </td>
                <!-- A kind is a direction for a trade and a sign for everything else. The
                     word carries it; the tone reinforces it, the way long/short does in the
                     journal. -->
                <td class="l" data-label={$t('portfolios.detail.colSide')}>
                  <Badge tone={opKind(o.operation.side).sign > 0 ? 'success' : opKind(o.operation.side).sign < 0 ? 'danger' : 'neutral'}>
                    {$t(`portfolios.kind.${o.operation.side}`)}
                  </Badge>
                </td>
                <!-- A cash row has no units: showing a quantity of 1 would be noise the eye
                     has to filter on every dividend line. -->
                <td class="num" data-label={$t('portfolios.detail.colQty')}>{opKind(o.operation.side).trade ? fmtNum(o.operation.quantity) : ''}</td>
                <!-- Op amounts are stored raw in the asset's own currency, not converted. -->
                <td class="num" data-label={$t('portfolios.detail.colPrice')}>
                  {#if o.operation.side === 'split' || o.operation.side === 'reverse_split'}
                    {fmtNum(o.operation.ratio_num, 4)} : {fmtNum(o.operation.ratio_den, 4)}
                  {:else if o.operation.side === 'spin_off'}
                    {$t('portfolios.corp.spinCell', { qty: fmtNum(o.operation.child_qty), pct: fmtNum(o.operation.cost_share_pct, 2) })}
                  {:else}
                    {fmtMoney(o.operation.price, o.currency ?? 'USD')}
                  {/if}
                </td>
                <td class="num" data-label={$t('portfolios.detail.colFee')}>{fmtMoney(o.operation.fee, o.currency ?? 'USD')}</td>
                <td class="l note" data-label={$t('portfolios.detail.colNote')} title={o.operation.note || undefined}>{o.operation.note || ''}</td>
                <td class="op-del"><button class="icon danger-hover" onclick={() => deleteOp(o.operation.id)} aria-label={$t('common.delete')} title={$t('common.delete')}><Icon name="trash" size={14} /></button></td>
              </tr>
            {/each}
            {#if filteredOps.length === 0}
              <tr><td colspan="8" class="empty">
                {data.operations.length === 0 ? $t('portfolios.detail.noOperations') : $t('portfolios.detail.noOperationsMatch')}
              </td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
</div>

{#if showCash && data}
  <CashOpModal
    portfolioId={id}
    assets={data.assets}
    currency={ccy}
    onclose={() => (showCash = false)}
    onsaved={() => {
      analyticsKey += 1;
      load();
    }}
  />
{/if}

{#if showTargets}
  <TargetsModal
    portfolioId={id}
    portfolio={data?.portfolio}
    onsettings={() => ((showTargets = false), (showSettings = true))}
    onclose={() => (showTargets = false)}
    onsaved={() => (analyticsKey += 1)}
  />
{/if}

{#if showSettings && data}
  <PortfolioSettingsModal
    portfolio={data.portfolio}
    onclose={() => (showSettings = false)}
    onsaved={() => load()}
  />
{/if}

{#if assetOpen}
  <AssetDetailModal
    portfolioId={id}
    asset={assetOpen}
    currency={ccy}
    classification={data?.exposure?.assets?.[assetOpen.id] ?? null}
    overLimit={overLimit(assetOpen.id)}
    onclose={() => (assetFor = null)}
    onchanged={() => {
      analyticsKey += 1;
      load();
      onchanged();
    }}
    onbook={(side, price, qty) => {
      const aid = assetOpen.id;
      assetFor = null;
      openOp(aid, side, price, qty);
    }}
  />
{/if}

{#if showMeasure && data}
  <MeasureModal
    portfolio={data.portfolio}
    onclose={() => (showMeasure = false)}
    onchanged={() => {
      analyticsKey += 1;
      load();
    }}
  />
{/if}

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('portfolios.card.deleteAria')}
  message={confirmMessage}
  confirmLabel={$t('portfolios.card.deleteAria')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={onConfirmYes}
/>

<AddAssetModal
  bind:open={showAddAsset}
  portfolioId={id}
  portfolioCurrency={data?.portfolio?.currency ?? 'USD'}
  onadded={() => (load(), onchanged())}
/>

<ImportModal
  bind:open={showImport}
  portfolioId={id}
  portfolioCurrency={data?.portfolio?.currency ?? 'USD'}
  assets={data?.assets ?? []}
  onimported={() => (load(), onchanged())}
/>

<BrokerHoldingsModal
  bind:open={showBroker}
  portfolioId={id}
  portfolioCurrency={data?.portfolio?.currency ?? 'USD'}
  assets={data?.assets ?? []}
  onimported={() => (load(), onchanged())}
/>

<ReconcileModal
  bind:open={showReconcile}
  portfolioId={id}
  assets={data?.assets ?? []}
  onenabled={() => (load(), onchanged())}
/>

<!-- Inline op form modal -->
<Modal open={opFor != null} size="sm" fit title={$t('portfolios.detail.newOperation')} onclose={() => (opFor = null)}>
  <div class="form">
    <div class="sides" role="group" aria-label={$t('portfolios.detail.colSide')}>
      <button class="side-buy" class:active={opForm.side === 'buy'} aria-pressed={opForm.side === 'buy'} onclick={() => (opForm.side = 'buy')}>{$t('portfolios.detail.buy')}</button>
      <button class="side-sell" class:active={opForm.side === 'sell'} aria-pressed={opForm.side === 'sell'} onclick={() => (opForm.side = 'sell')}>{$t('portfolios.detail.sell')}</button>
    </div>
    <Input label={$t('portfolios.detail.date')} type="date" bind:value={opForm.op_date} />
    <div class="pair">
      <Input label={$t('portfolios.detail.quantity')} type="number" step="any" bind:value={opForm.quantity} />
      <Input label={$t('portfolios.detail.priceIn', { ccy: opCurrency })} type="number" step="any" bind:value={opForm.price} />
    </div>
    <Input label={$t('portfolios.detail.feeIn', { ccy: opCurrency })} type="number" step="any" bind:value={opForm.fee} />
    <Input label={$t('portfolios.detail.note')} bind:value={opForm.note} />
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (opFor = null)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={submitOp}>{$t('portfolios.detail.addOperation')}</Button>
  {/snippet}
</Modal>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .title h1 {
    font-size: 22px;
    font-weight: 600;
    line-height: var(--lh-tight);
  }
  .rename {
    padding: var(--space-1) var(--space-2);
    font-size: 22px;
    font-weight: 600;
    min-width: 260px;
  }
  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .ccysel,
  .auto {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
    color: var(--muted);
  }
  /* Attention counter on "Sources": a hairline amber tag, mono figure. */
  .attn {
    border: var(--hairline) solid color-mix(in srgb, var(--amber) 45%, var(--border));
    border-radius: var(--radius-sm);
    color: var(--amber-ink);
    font-family: var(--mono);
    font-size: 10px;
    padding: 0 5px;
    line-height: 1.5;
  }
  /* Info popover anchored to the refresh button; right-aligned so it never overflows the
     header's right edge when the actions row wraps. */
  .infowrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .infobtn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .infobtn:hover,
  .infobtn.on {
    color: var(--text);
    background: var(--surface-2);
  }
  .infopop {
    position: absolute;
    top: calc(100% + var(--space-1));
    right: 0;
    z-index: 20;
    width: 340px;
    max-width: min(340px, calc(100vw - var(--space-4)));
    padding: var(--space-3);
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-1);
    font-size: 12px;
    color: var(--muted);
    text-align: left;
    white-space: normal;
    cursor: default;
  }
  .infopop p {
    margin: 0 0 var(--space-2);
    line-height: 1.5;
  }
  .infopop p:last-child {
    margin-bottom: 0;
  }
  .infopop .ititle {
    color: var(--text);
    font-weight: 600;
    margin-bottom: var(--space-1);
  }
  .infopop ul {
    margin: 0 0 var(--space-3);
    padding-left: var(--space-3);
    line-height: 1.5;
  }
  .infopop li + li {
    margin-top: var(--space-1);
  }
  .infopop b {
    color: var(--text);
    font-weight: 600;
  }
  .infopop .warn {
    color: var(--amber-ink);
  }
  /* Description + the note's disclosure, on one line under the title. */
  .about {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-top: calc(-1 * var(--space-2));
  }
  /* The description is a button so it reads as editable, but it must look like the text
     it replaces: the pencil only appears on hover. */
  .desc {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: text;
  }
  .desc :global(svg) {
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .desc:hover {
    color: var(--text);
  }
  .desc:hover :global(svg) {
    opacity: 0.6;
  }
  .descedit {
    flex: 1 1 320px;
    max-width: 720px;
    font-size: 13px;
  }
  .notestoggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--dim);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
  }
  .notestoggle:hover,
  .notestoggle.on {
    color: var(--text);
  }
  /* A written-but-folded note gets a dot: the only way to know it is there. */
  .notestoggle .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .notes textarea {
    width: 100%;
    padding: var(--space-3);
    font: inherit;
    font-size: 13px;
    line-height: var(--lh-base);
    resize: vertical;
  }
  .noteshint {
    font-size: 12px;
  }
  .dim {
    color: var(--dim);
  }

  /* Headline + allocation side by side, the tiles under both. */
  .overview {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(300px, 1fr);
    gap: var(--space-3);
    align-items: stretch;
  }
  .card {
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    min-width: 0;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .tiles :global(.statcard) {
    background: var(--surface);
    border-radius: var(--radius);
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
  }
  .tabs > button {
    background: transparent;
    border: none;
    border-bottom: 1.5px solid transparent;
    color: var(--muted);
    font-size: 13px;
    font-weight: 600;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    margin-bottom: -1px;
    transition: color 160ms ease;
  }
  .tabs > button:hover {
    color: var(--text);
  }
  .tabs > button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    font-size: 11px;
    color: var(--dim);
    font-family: var(--mono);
    font-weight: 400;
    margin-left: var(--space-1);
  }
  .tab-tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-1);
  }
  /* Search: one frame (the label), the input inside it is bare. */
  .filter {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 26px;
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--dim);
    transition: border-color 160ms ease;
  }
  .filter:focus-within {
    border-color: var(--accent);
  }
  .filter input {
    width: 180px;
    height: 24px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
    color: var(--text);
    font-size: 12px;
  }
  .filter input:focus {
    outline: none;
    box-shadow: none;
  }
  .filter input::placeholder {
    color: var(--dim);
  }

  /* Holdings and ledger: one hairline frame, sticky 10px uppercase header, 36px rows,
     numbers right aligned in tabular mono, a hover band. The frame scrolls on its own
     so the header can stick. */
  .table-wrap {
    overflow: auto;
    max-height: min(72vh, 760px);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th,
  td {
    height: 36px;
    padding: 0 var(--space-3);
    text-align: right;
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 32px;
    background: var(--surface-raised);
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  tbody tr:last-child td {
    border-bottom: 0;
  }
  tbody tr {
    transition: background-color 140ms ease;
  }
  tbody tr:hover {
    background: var(--surface-hover);
  }
  td.num {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  /* The header's clickable part is a <button> (keyboard-operable), but it should
     look and hit like the whole cell: inherit the type, fill the box, no chrome. */
  .sortable {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0;
    background: none;
    border: none;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: inherit;
    cursor: pointer;
    user-select: none;
  }
  th.num .sortable {
    flex-direction: row-reverse;
  }
  .sortable:hover,
  .sortable.on {
    color: var(--text);
  }
  .sarrow {
    display: inline-flex;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .sortable:hover .sarrow,
  .sortable.on .sarrow {
    opacity: 1;
  }
  .l,
  td.l {
    text-align: left;
  }
  .symcell {
    max-width: 420px;
  }
  .symline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .sym {
    font-weight: 600;
    color: var(--text);
  }
  .symbtn {
    flex: none;
    background: none;
    border: 0;
    padding: 0;
    font: inherit;
    font-weight: 600;
    color: var(--text);
    cursor: pointer;
  }
  .symbtn:hover {
    color: var(--accent);
  }
  .aname {
    color: var(--dim);
    font-size: 12px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .note {
    color: var(--muted);
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ops,
  .op-del {
    width: 1%;
    padding-right: var(--space-2);
  }
  /* Row actions: icon buttons, quiet until the row is hovered. */
  .rowacts {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .rowacts button,
  .op-del button {
    width: 26px;
    height: 26px;
  }
  .wcell {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .wbar {
    width: 44px;
    height: 4px;
    border-radius: 2px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .wfill {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .wv {
    min-width: 44px;
    text-align: right;
  }
  .empty {
    text-align: center;
    color: var(--dim);
    font-family: var(--font);
    height: 72px;
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }

  /* The inline operation form. */
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    padding: 2px;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
  }
  .sides button {
    height: 28px;
    background: transparent;
    border: 0;
    border-radius: calc(var(--radius-sm) - 2px);
    color: var(--muted);
    font-weight: 600;
    cursor: pointer;
  }
  .sides button.active {
    background: var(--surface-2);
  }
  .sides button.side-buy.active {
    color: var(--green);
  }
  .sides button.side-sell.active {
    color: var(--red);
  }

  @media (max-width: 1279px) {
    .tiles {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  /* Below the desktop baseline the tab tools wrap under the tabs instead of widening the
     page (a tablet already ran them past the right edge). */
  @media (max-width: 1099px) {
    .overview {
      grid-template-columns: minmax(0, 1fr);
    }
    .tabs {
      flex-wrap: wrap;
    }
    .tab-tools {
      flex-wrap: wrap;
      justify-content: flex-end;
    }
  }

  @media (max-width: 767px) {
    .title {
      flex: 1 1 auto;
    }
    .title h1 {
      overflow-wrap: anywhere;
    }
    .rename {
      min-width: 0;
      width: 100%;
    }
    .actions {
      margin-left: 0;
    }
    .infobtn {
      min-width: 44px;
      min-height: 44px;
    }
    .desc :global(svg) {
      opacity: 0.6;
    }
    .tiles {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .tabs > button {
      min-height: 44px;
    }
    .tab-tools {
      margin-left: 0;
      width: 100%;
      justify-content: flex-start;
    }
    .filter {
      flex: 1 1 100%;
      height: 44px;
    }
    .filter input {
      width: 100%;
    }
    .table-wrap {
      max-height: none;
    }

    /* Holdings and ledger rows become record cards: symbol first, every figure keeps its
       column name as a caption, the row actions stay on the card. The header row stays
       as the sort bar, so sorting is still one tap away. */
    .cards {
      overflow-x: visible;
    }
    .cards table,
    .cards tbody {
      display: block;
    }
    .cards thead tr {
      display: flex;
      flex-wrap: wrap;
      gap: 0 var(--space-3);
      padding: var(--space-2) var(--space-3);
      border-bottom: 0.5px solid var(--border);
    }
    .cards table.tbl thead th {
      display: block;
      border: none;
      padding: 0;
      text-align: left;
    }
    .cards table.tbl thead th:empty {
      display: none;
    }
    .cards .sortable {
      min-height: 44px;
    }
    .cards tbody tr {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-2) var(--space-3);
      padding: var(--space-3);
      border-bottom: 0.5px solid var(--border);
    }
    .cards tbody tr:last-child {
      border-bottom: none;
    }
    .cards table.tbl td {
      display: block;
      height: auto;
      min-width: 0;
      padding: 0;
      border: none;
      text-align: left;
      overflow-wrap: anywhere;
    }
    .cards table.tbl td[data-label]::before {
      content: attr(data-label);
      display: block;
      font-family: var(--font);
      font-size: 10px;
      font-weight: var(--fw-normal);
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--dim);
    }
    .cards table.tbl td.l:first-child:not([data-label]),
    .cards table.tbl td.note,
    .cards table.tbl td.empty {
      grid-column: 1 / -1;
    }
    .cards table.tbl td.empty {
      text-align: center;
      padding: var(--space-4);
    }
    .cards .note {
      max-width: none;
      white-space: normal;
    }
    .cards table.tbl td.ops,
    .cards table.tbl td.op-del {
      grid-column: 1 / -1;
      display: flex;
      justify-content: flex-end;
      gap: var(--space-2);
    }
    .cards .icon {
      min-width: 44px;
      min-height: 44px;
      justify-content: center;
    }
  }
</style>
