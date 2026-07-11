<script>
  import Icon from '$lib/ui/Icon.svelte';
  // One portfolio's full-page detail: a header with grouped actions (back, currency dropdown,
  // auto-update, add asset, refresh), an expanded totals bandeau, the value chart, then a single
  // tabbed panel switching between the Assets table and the Operations ledger — each with its own
  // search box and per-column sorting. Prices come from the server in the portfolio's display
  // currency; changing the currency dropdown re-prices via a PATCH + reload.
  import Modal from '$lib/ui/Modal.svelte';
  import AddAssetModal from './AddAssetModal.svelte';
  import ReconcileModal from './ReconcileModal.svelte';
  import ValueChart from './ValueChart.svelte';
  import AllocationDonut from './AllocationDonut.svelte';
  import {
    portfoliosApi,
    fmtMoney,
    fmtSignedMoney,
    fmtNum,
    fmtPct,
    gainPct,
    CURRENCIES
  } from './api.js';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { id, onback = () => {}, onchanged = () => {} } = $props();

  let data = $state(null);
  let loading = $state(false);
  let error = $state('');
  let refreshing = $state(false);
  let showAddAsset = $state(false);
  let showReconcile = $state(false);

  // Inline operation form state, keyed by asset id when open.
  let opFor = $state(null);
  let opForm = $state({ side: 'buy', quantity: '', price: '', fee: '', op_date: '', note: '' });

  // Which table tab is showing.
  let tab = $state('assets'); // 'assets' | 'operations'

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
    } catch (e) {
      error = e.message;
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

  async function changeCurrency(e) {
    const currency = e.target.value;
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

  function openOp(assetId, side = 'buy') {
    opFor = assetId;
    opForm = { side, quantity: '', price: '', fee: '', op_date: today(), note: '' };
  }
  function today() {
    const d = new Date(); // local date — toISOString() is UTC and shifts near midnight
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
  // neither is undoable — so both go through a real confirmation rather than the
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
  function arrow(state, col) {
    if (state.col !== col) return '';
    return state.dir === 1 ? ' ▲' : ' ▼';
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

  let totalGain = $derived(
    data ? gainPct(data.summary.market_value, data.summary.cost_basis) : null
  );

  // Extra bandeau metrics derived from the assets.
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

<!-- A sortable column header. The clickable part is a real <button>, so the column
     is reachable by Tab and operable by Enter/Space; aria-sort announces the current
     direction. A bare onclick on the <th> looked identical and did neither.
     `left` left-aligns text columns; numeric ones stay right-aligned via .num. -->
{#snippet sortHeader(state, col, label, setState, left = false)}
  <th class:l={left} aria-sort={ariaSort(state, col)}>
    <button class="sortable" onclick={() => setState(sortBy(state, col))}>
      {label}{arrow(state, col)}
    </button>
  </th>
{/snippet}

<div class="detail">
  <header class="head">
    <button class="btn back" onclick={onback} aria-label={$t('portfolios.detail.backAria')}>← {$t('portfolios.detail.back')}</button>
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
        <button class="pencil" onclick={startRename} disabled={!data} aria-label={$t('portfolios.detail.renameAria')}><Icon name="pencil" size={14} /></button>
      {/if}
    </div>
    <div class="actions">
      <label class="ccysel">
        {$t('portfolios.detail.currency')}
        <select value={ccy} onchange={changeCurrency} disabled={!data}>
          {#each CURRENCIES as c (c)}<option value={c}>{c}</option>{/each}
        </select>
      </label>
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
        <button class="btn ghost" onclick={() => (showReconcile = true)} disabled={!data}>
          <Icon name="settings" size={13} /> {$t('portfolios.detail.sources')}
          {#if attentionCount > 0}<span class="badge">{attentionCount}</span>{/if}
        </button>
      {/if}
      <button class="btn" onclick={() => (showAddAsset = true)} disabled={!data}>+ {$t('portfolios.detail.addAsset')}</button>
      <button class="btn primary" onclick={refresh} disabled={refreshing || !data}>
        {#if refreshing}{$t('portfolios.detail.refreshing')}{:else}<Icon name="refresh-cw" size={13} /> {$t('portfolios.detail.refreshPrices')}{/if}
      </button>
    </div>
  </header>

  {#if loading && !data}
    <!-- Hold the totals bandeau + chart shape rather than collapsing to one line. -->
    <div class="totals">
      {#each Array(8) as _, i (i)}<Skeleton height="56px" />{/each}
    </div>
    <Skeleton height="220px" />
  {:else if error && !data}
    <ErrorText {error} copyable />
  {:else if data}
    {#if data.portfolio.description}<p class="desc">{data.portfolio.description}</p>{/if}

    {@const totalPl = data.summary.unrealized + data.summary.realized}
    <div class="totals">
      <!-- Value and cost basis are balances: a "+" in front of them says nothing.
           Unrealized / realized / total are results, so they carry their sign. -->
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.value')}</span>
        <strong class="num">{fmtMoney(data.summary.market_value, ccy)}</strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.costBasis')}</span>
        <strong class="num">{fmtMoney(data.summary.cost_basis, ccy)}</strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.unrealized')}</span>
        <strong class="num" class:up={data.summary.unrealized > 0} class:down={data.summary.unrealized < 0}>
          {fmtSignedMoney(data.summary.unrealized, ccy)} <small>{fmtPct(totalGain)}</small>
        </strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.realized')}</span>
        <strong class="num" class:up={data.summary.realized > 0} class:down={data.summary.realized < 0}>
          {fmtSignedMoney(data.summary.realized, ccy)}
        </strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.totalPl')}</span>
        <strong class="num" class:up={totalPl > 0} class:down={totalPl < 0}>
          {fmtSignedMoney(totalPl, ccy)}
        </strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.assets')}</span>
        <strong class="num">{data.assets.length}</strong>
      </div>
      <!-- Color follows the figure, not the rank: in a portfolio that is entirely
           down, the "best" asset is still a loss and must not read as green. -->
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.best')}</span>
        {#if bestAsset}
          <strong class:up={bestAsset.unrealized > 0} class:down={bestAsset.unrealized < 0}>
            {bestAsset.symbol} <small class="num">{fmtSignedMoney(bestAsset.unrealized, ccy)}</small>
          </strong>
        {:else}<strong>—</strong>{/if}
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.worst')}</span>
        {#if worstAsset}
          <strong class:up={worstAsset.unrealized > 0} class:down={worstAsset.unrealized < 0}>
            {worstAsset.symbol} <small class="num">{fmtSignedMoney(worstAsset.unrealized, ccy)}</small>
          </strong>
        {:else}<strong>—</strong>{/if}
      </div>
    </div>

    <ErrorText error={error} copyable />

    <div class="charts">
      <div class="panel chart">
        <ValueChart snapshots={data.snapshots} currency={ccy} />
      </div>
      <div class="panel side">
        <AllocationDonut assets={data.assets} currency={ccy} />
      </div>
    </div>

    <div class="tabs">
      <button class:active={tab === 'assets'} onclick={() => (tab = 'assets')}>
        {$t('portfolios.detail.tabAssets')} <span class="count">{data.assets.length}</span>
      </button>
      <button class:active={tab === 'operations'} onclick={() => (tab = 'operations')}>
        {$t('portfolios.detail.tabOperations')} <span class="count">{data.operations.length}</span>
      </button>
      <div class="tab-tools">
        {#if tab === 'assets'}
          <input class="filter" type="search" placeholder={$t('portfolios.detail.searchAssets')} bind:value={assetSearch} />
          <button class="btn sm" onclick={() => (showAddAsset = true)}>+ {$t('portfolios.detail.addAsset')}</button>
        {:else}
          <input class="filter" type="search" placeholder={$t('portfolios.detail.searchOperations')} bind:value={opSearch} />
        {/if}
      </div>
    </div>

    {#if tab === 'assets'}
      <div class="table-wrap">
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
                <td class="l">
                  <span class="sym">{a.symbol}</span>
                  {#if a.recon_status === 'unresolved'}
                    <span class="rchip bad" title={$t('portfolios.detail.unresolvedHint')}>{$t('portfolios.reconcile.status.unresolved')}</span>
                  {:else if a.recon_status === 'manual'}
                    <span class="rchip manual" title={$t('portfolios.detail.manualHint')}>{$t('portfolios.reconcile.status.manual')}</span>
                  {:else if a.spot_provider}
                    <span class="rchip src" title={$t('portfolios.detail.pricedViaHint', { source: a.spot_provider })}>{a.spot_provider}</span>
                  {/if}
                  <span class="aname">{a.name}</span>
                </td>
                <td class="num">{fmtNum(a.quantity)}</td>
                <td class="num">{fmtMoney(a.avg_cost, ccy)}</td>
                <td class="num">{fmtMoney(a.price, ccy)}</td>
                <td class="num">{fmtMoney(a.market_value, ccy)}</td>
                <td class="num">
                  {#if a.weight != null}
                    <span class="wcell">
                      <span class="wbar"><span class="wfill" style="width:{Math.min(a.weight, 100)}%"></span></span>
                      {a.weight.toFixed(1)}%
                    </span>
                  {:else}—{/if}
                </td>
                <!-- PnL columns: sign in the text, color as the second channel. -->
                <td class="num" class:up={a.unrealized > 0} class:down={a.unrealized < 0}>
                  {fmtSignedMoney(a.unrealized, ccy)}
                </td>
                <td class="num" class:up={a.realized > 0} class:down={a.realized < 0}>
                  {fmtSignedMoney(a.realized, ccy)}
                </td>
                <td class="ops">
                  <button class="link" onclick={() => openOp(a.id, 'buy')}>+ {$t('portfolios.detail.op')}</button>
                  <button class="link del" onclick={() => deleteAsset(a.id)}><Icon name="x" size={13} /></button>
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
      <div class="table-wrap">
        <table class="tbl">
          <thead>
            <tr>
              {@render sortHeader(opSortState, 'op_date', $t('portfolios.detail.colDate'), setOpSort, true)}
              {@render sortHeader(opSortState, 'symbol', $t('portfolios.detail.colSymbol'), setOpSort, true)}
              {@render sortHeader(opSortState, 'side', $t('portfolios.detail.colSide'), setOpSort, true)}
              {@render sortHeader(opSortState, 'quantity', $t('portfolios.detail.colQty'), setOpSort)}
              {@render sortHeader(opSortState, 'price', $t('portfolios.detail.colPriceUsd'), setOpSort)}
              {@render sortHeader(opSortState, 'fee', $t('portfolios.detail.colFee'), setOpSort)}
              <th class="l">{$t('portfolios.detail.colNote')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredOps as o (o.operation.id)}
              <tr>
                <td class="l num">{o.operation.op_date}</td>
                <td class="l sym">{o.symbol}</td>
                <!-- Buy/sell is a direction, not a result. The word carries it; the
                     tone reinforces it, the way long/short does in the journal. -->
                <td class="l">
                  <Badge tone={o.operation.side === 'buy' ? 'success' : 'danger'}>
                    {o.operation.side === 'buy' ? $t('portfolios.detail.buy') : $t('portfolios.detail.sell')}
                  </Badge>
                </td>
                <td class="num">{fmtNum(o.operation.quantity)}</td>
                <td class="num">{fmtMoney(o.operation.price, 'USD')}</td>
                <td class="num">{fmtMoney(o.operation.fee, 'USD')}</td>
                <td class="l note">{o.operation.note || '—'}</td>
                <td><button class="link del" onclick={() => deleteOp(o.operation.id)}><Icon name="x" size={13} /></button></td>
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

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('portfolios.card.deleteAria')}
  message={confirmMessage}
  confirmLabel={$t('portfolios.card.deleteAria')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={onConfirmYes}
/>

<AddAssetModal bind:open={showAddAsset} portfolioId={id} onadded={() => (load(), onchanged())} />

<ReconcileModal
  bind:open={showReconcile}
  portfolioId={id}
  assets={data?.assets ?? []}
  onenabled={() => (load(), onchanged())}
/>

<!-- Inline op form modal -->
<Modal open={opFor != null} size="sm" title={$t('portfolios.detail.newOperation')} onclose={() => (opFor = null)}>
  <div class="form">
    <div class="sides">
      <button class:active={opForm.side === 'buy'} onclick={() => (opForm.side = 'buy')}>{$t('portfolios.detail.buy')}</button>
      <button class:active={opForm.side === 'sell'} onclick={() => (opForm.side = 'sell')}>{$t('portfolios.detail.sell')}</button>
    </div>
    <label>{$t('portfolios.detail.date')}<input type="date" bind:value={opForm.op_date} /></label>
    <label>{$t('portfolios.detail.quantity')}<input type="number" step="any" bind:value={opForm.quantity} /></label>
    <label>{$t('portfolios.detail.priceUsd')}<input type="number" step="any" bind:value={opForm.price} /></label>
    <label>{$t('portfolios.detail.feeUsd')}<input type="number" step="any" bind:value={opForm.fee} /></label>
    <label>{$t('portfolios.detail.note')}<input type="text" bind:value={opForm.note} /></label>
    <button class="btn primary" onclick={submitOp}>{$t('portfolios.detail.addOperation')}</button>
  </div>
</Modal>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .back {
    font-weight: var(--fw-semibold);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .pencil {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: 2px;
  }
  .pencil:hover {
    color: var(--text);
  }
  .rename {
    background: var(--surface-2);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
    min-width: 260px;
  }
  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .ccysel {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .auto {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .desc {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .totals {
    display: flex;
    gap: var(--space-6);
    flex-wrap: wrap;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
  }
  .t {
    display: flex;
    flex-direction: column;
  }
  .t .lbl {
    font-size: var(--text-xs);
    text-transform: uppercase;
    color: var(--muted);
  }
  .t strong {
    font-size: var(--text-lg);
  }
  .t small {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .charts {
    display: flex;
    gap: var(--space-3);
    align-items: stretch;
    flex-wrap: wrap;
  }
  .charts .chart {
    flex: 2 1 480px;
    min-width: 0;
  }
  .charts .side {
    flex: 1 1 300px;
    min-width: 280px;
  }
  .panel {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
  }
  .wcell {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-variant-numeric: tabular-nums;
  }
  .wbar {
    width: 44px;
    height: 5px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .wfill {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 3px;
  }
  .btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .btn.sm {
    font-size: var(--text-sm);
  }
  .btn.primary {
    border-color: var(--accent);
    color: var(--text);
  }
  .btn.ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
  }
  /* Attention counter on an amber fill. Black, not a token: --amber stays light in
     both themes, so --text (near-white on dark) would vanish on it. */
  .btn .badge {
    background: var(--amber);
    color: #000;
    font-size: var(--text-xs);
    font-weight: var(--fw-semibold);
    border-radius: 999px;
    padding: 0 6px;
    line-height: 1.4;
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .rchip {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.02em;
    border-radius: var(--radius);
    padding: 0 var(--space-1);
    margin-left: var(--space-1);
    border: 1px solid var(--border);
    color: var(--muted);
    vertical-align: middle;
  }
  .rchip.bad {
    color: var(--amber);
    border-color: color-mix(in srgb, var(--amber) 45%, transparent);
  }
  .rchip.src {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
  }
  .tabs > button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    margin-bottom: -1px;
  }
  .tabs > button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
    margin-left: 2px;
  }
  .tab-tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .filter {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
  }
  .table-wrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  th,
  td {
    padding: var(--space-1) var(--space-2);
    text-align: right;
    border-bottom: 1px solid var(--border);
  }
  /* The header's clickable part is a <button> (keyboard-operable), but it should
     look and hit like the whole cell: inherit the type, fill the box, no chrome. */
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
  .l {
    text-align: left;
  }
  .sym {
    font-weight: var(--fw-semibold);
  }
  .aname {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-left: var(--space-1);
  }
  .note {
    color: var(--muted);
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ops {
    white-space: nowrap;
  }
  .link {
    background: transparent;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 2px 4px;
  }
  .link.del {
    color: var(--red);
  }
  .empty {
    text-align: center;
    color: var(--muted);
    padding: var(--space-4);
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .form label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .sides {
    display: flex;
    gap: var(--space-2);
  }
  .sides button {
    flex: 1;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1);
    color: var(--muted);
    cursor: pointer;
  }
  .sides button.active {
    color: var(--text);
    border-color: var(--accent);
  }
  .muted {
    color: var(--muted);
  }
</style>
