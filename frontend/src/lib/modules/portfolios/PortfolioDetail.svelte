<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
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
  import { portfoliosApi, fmtMoney, fmtNum, fmtPct, gainPct, CURRENCIES } from './api.js';
  import { t } from '$lib/i18n';

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

  async function deleteAsset(assetId) {
    if (!confirm($t('portfolios.detail.confirmDeleteAsset'))) return;
    await portfoliosApi.deleteAsset(assetId);
    load();
    onchanged();
  }

  async function deleteOp(opId) {
    if (!confirm($t('portfolios.detail.confirmDeleteOp'))) return;
    await portfoliosApi.deleteOperation(opId);
    load();
    onchanged();
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
    <p class="muted">{$t('common.loading')}</p>
  {:else if error && !data}
    <p class="err" title="click to copy" use:copyLog={error}>{error}</p>
  {:else if data}
    {#if data.portfolio.description}<p class="desc">{data.portfolio.description}</p>{/if}

    <div class="totals">
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.value')}</span>
        <strong>{fmtMoney(data.summary.market_value, ccy)}</strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.costBasis')}</span>
        <strong>{fmtMoney(data.summary.cost_basis, ccy)}</strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.unrealized')}</span>
        <strong class:up={data.summary.unrealized > 0} class:down={data.summary.unrealized < 0}>
          {fmtMoney(data.summary.unrealized, ccy)} <small>{fmtPct(totalGain)}</small>
        </strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.realized')}</span>
        <strong class:up={data.summary.realized > 0} class:down={data.summary.realized < 0}>
          {fmtMoney(data.summary.realized, ccy)}
        </strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.totalPl')}</span>
        {#if true}
          {@const tot = data.summary.unrealized + data.summary.realized}
          <strong class:up={tot > 0} class:down={tot < 0}>{fmtMoney(tot, ccy)}</strong>
        {/if}
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.assets')}</span>
        <strong>{data.assets.length}</strong>
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.best')}</span>
        {#if bestAsset}
          <strong class="up">{bestAsset.symbol} <small>{fmtMoney(bestAsset.unrealized, ccy)}</small></strong>
        {:else}<strong>—</strong>{/if}
      </div>
      <div class="t">
        <span class="lbl">{$t('portfolios.detail.worst')}</span>
        {#if worstAsset}
          <strong class="down">{worstAsset.symbol} <small>{fmtMoney(worstAsset.unrealized, ccy)}</small></strong>
        {:else}<strong>—</strong>{/if}
      </div>
    </div>

    {#if error}<p class="err" title="click to copy" use:copyLog={error}>{error}</p>{/if}

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
              <th class="l sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'symbol'))}>{$t('portfolios.detail.colSymbol')}{arrow(assetSortState, 'symbol')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'quantity'))}>{$t('portfolios.detail.colQty')}{arrow(assetSortState, 'quantity')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'avg_cost'))}>{$t('portfolios.detail.colAvgCost')}{arrow(assetSortState, 'avg_cost')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'price'))}>{$t('portfolios.detail.colPrice')}{arrow(assetSortState, 'price')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'market_value'))}>{$t('portfolios.detail.colValue')}{arrow(assetSortState, 'market_value')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'weight'))}>{$t('portfolios.detail.colWeight')}{arrow(assetSortState, 'weight')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'unrealized'))}>{$t('portfolios.detail.colUnreal')}{arrow(assetSortState, 'unrealized')}</th>
              <th class="sortable" onclick={() => (assetSortState = sortBy(assetSortState, 'realized'))}>{$t('portfolios.detail.colRealized')}{arrow(assetSortState, 'realized')}</th>
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
                <td>{fmtNum(a.quantity)}</td>
                <td>{fmtMoney(a.avg_cost, ccy)}</td>
                <td>{fmtMoney(a.price, ccy)}</td>
                <td>{fmtMoney(a.market_value, ccy)}</td>
                <td>
                  {#if a.weight != null}
                    <span class="wcell">
                      <span class="wbar"><span class="wfill" style="width:{Math.min(a.weight, 100)}%"></span></span>
                      {a.weight.toFixed(1)}%
                    </span>
                  {:else}—{/if}
                </td>
                <td class:up={a.unrealized > 0} class:down={a.unrealized < 0}>
                  {fmtMoney(a.unrealized, ccy)}
                </td>
                <td class:up={a.realized > 0} class:down={a.realized < 0}>{fmtMoney(a.realized, ccy)}</td>
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
              <th class="l sortable" onclick={() => (opSortState = sortBy(opSortState, 'op_date'))}>{$t('portfolios.detail.colDate')}{arrow(opSortState, 'op_date')}</th>
              <th class="l sortable" onclick={() => (opSortState = sortBy(opSortState, 'symbol'))}>{$t('portfolios.detail.colSymbol')}{arrow(opSortState, 'symbol')}</th>
              <th class="l sortable" onclick={() => (opSortState = sortBy(opSortState, 'side'))}>{$t('portfolios.detail.colSide')}{arrow(opSortState, 'side')}</th>
              <th class="sortable" onclick={() => (opSortState = sortBy(opSortState, 'quantity'))}>{$t('portfolios.detail.colQty')}{arrow(opSortState, 'quantity')}</th>
              <th class="sortable" onclick={() => (opSortState = sortBy(opSortState, 'price'))}>{$t('portfolios.detail.colPriceUsd')}{arrow(opSortState, 'price')}</th>
              <th class="sortable" onclick={() => (opSortState = sortBy(opSortState, 'fee'))}>{$t('portfolios.detail.colFee')}{arrow(opSortState, 'fee')}</th>
              <th class="l">{$t('portfolios.detail.colNote')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredOps as o (o.operation.id)}
              <tr>
                <td class="l">{o.operation.op_date}</td>
                <td class="l sym">{o.symbol}</td>
                <td class="l" class:up={o.operation.side === 'buy'} class:down={o.operation.side === 'sell'}>
                  {o.operation.side === 'buy' ? $t('portfolios.detail.buy') : $t('portfolios.detail.sell')}
                </td>
                <td>{fmtNum(o.operation.quantity)}</td>
                <td>{fmtMoney(o.operation.price, 'USD')}</td>
                <td>{fmtMoney(o.operation.fee, 'USD')}</td>
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
    font-weight: 600;
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title h1 {
    font-size: 1.35rem;
    font-weight: 700;
  }
  .pencil {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.9rem;
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
    font-size: 1.2rem;
    font-weight: 700;
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
    font-size: 0.82rem;
    color: var(--muted);
  }
  .auto {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 0.82rem;
    color: var(--muted);
  }
  .desc {
    color: var(--muted);
    font-size: 0.9rem;
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
    font-size: 0.72rem;
    text-transform: uppercase;
    color: var(--muted);
  }
  .t strong {
    font-size: 1.15rem;
  }
  .t small {
    font-size: 0.78rem;
    font-weight: 500;
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
    font-size: 0.85rem;
    cursor: pointer;
  }
  .btn.sm {
    font-size: 0.8rem;
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
  .btn .badge {
    background: var(--amber);
    color: #000;
    font-size: 0.68rem;
    font-weight: 700;
    border-radius: 999px;
    padding: 0 6px;
    line-height: 1.4;
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .rchip {
    font-size: 0.66rem;
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
    font-size: 0.9rem;
    font-weight: 600;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    margin-bottom: -1px;
  }
  .tabs > button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    font-size: 0.72rem;
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
    font-size: 0.82rem;
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
  th.sortable {
    cursor: pointer;
    user-select: none;
  }
  th.sortable:hover {
    color: var(--text);
  }
  .l {
    text-align: left;
  }
  .sym {
    font-weight: 700;
  }
  .aname {
    color: var(--muted);
    font-size: 0.78rem;
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
    font-size: 0.8rem;
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
    font-size: 0.8rem;
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
  .err {
    color: var(--red);
  }
</style>
