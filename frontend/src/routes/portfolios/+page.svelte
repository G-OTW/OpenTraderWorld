<script>
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  // Portfolio Tracker, first page: the cross-book overview, then the user's portfolios as
  // cards, each with its value, the unrealized result (glyph + arrow + tone), the trend
  // sparkline toned by sign and the asset count. Create a portfolio inline;
  // click a card to open its full-page detail (assets, operations ledger, value-over-time chart).
  // Prices are CoinGecko (crypto) and Yahoo (stocks/ETF), fetched in USD and converted to each
  // portfolio's display currency.
  import {
    portfoliosApi,
    fmtMoney,
    unrealizedPct,
    CURRENCIES
  } from '$lib/modules/portfolios/api.js';
  import PortfolioDetail from '$lib/modules/portfolios/PortfolioDetail.svelte';
  import Sparkline from '$lib/ui/charts/Sparkline.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { fmtDate } from '$lib/format.js';
  import OverviewBlock from '$lib/modules/portfolios/OverviewBlock.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let portfolios = $state([]);
  let error = $state('');
  let loading = $state(false);
  let selected = $state(null); // portfolio id of the open full-page detail
  let openBroker = $state(false);

  // A finished background broker read links here with the portfolio it was for.
  $effect(() => {
    const params = new URLSearchParams(location.search);
    const id = params.get('id');
    if (!id) return;
    selected = id;
    openBroker = params.has('broker');
    history.replaceState(history.state, '', location.pathname);
  });

  let showCreate = $state(false);
  let creating = $state(false);
  let form = $state({ name: '', currency: 'USD', description: '' });

  // Deleting a portfolio takes its assets and its whole operations ledger with it,
  // so it goes through a real dialog rather than the browser's confirm().
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function load() {
    loading = true;
    error = '';
    portfoliosApi
      .list()
      .then((r) => (portfolios = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  }
  $effect(load);

  async function create() {
    if (creating || !form.name.trim()) return; // a second click would create a second portfolio
    creating = true;
    try {
      const pf = await portfoliosApi.create(form);
      showCreate = false;
      form = { name: '', currency: 'USD', description: '' };
      load();
      selected = pf.id;
    } catch (e) {
      error = e.message;
    } finally {
      creating = false;
    }
  }

  function askRemove(e, p) {
    e.stopPropagation(); // the card itself opens the detail view
    pendingDelete = p;
    confirmOpen = true;
  }

  async function confirmRemove() {
    const p = pendingDelete;
    pendingDelete = null;
    if (!p) return;
    try {
      await portfoliosApi.remove(p.id);
      load();
    } catch (e) {
      error = e.message;
    }
  }

  // Change since the previous snapshot (%), from the card's sparkline series.
  function lastChangePct(spark) {
    if (!spark || spark.length < 2) return null;
    const prev = spark[spark.length - 2];
    const last = spark[spark.length - 1];
    if (!prev) return null;
    return ((last - prev) / prev) * 100;
  }
</script>

{#if selected}
  <div class="page">
    <PortfolioDetail id={selected} bind:openBroker onback={() => (selected = null)} onchanged={load} />
  </div>
{:else}
  <div class="page">
    <PageHeader title={$t('portfolios.page.title')}>
      {#snippet actions()}
        <Button icon="plus" onclick={() => (showCreate = !showCreate)}>
          {$t('portfolios.page.newPortfolio')}
        </Button>
      {/snippet}
    </PageHeader>

    {#if showCreate}
      <div class="create">
        <input placeholder={$t('portfolios.page.namePlaceholder')} bind:value={form.name} />
        <div class="ccy">
          <Dropdown
            bind:value={form.currency}
            ariaLabel={$t('portfolios.addAsset.currency')}
            options={CURRENCIES.map((c) => ({ value: c, label: c }))}
          />
        </div>
        <input class="desc" placeholder={$t('portfolios.page.descPlaceholder')} bind:value={form.description} />
        <Button variant="primary" onclick={create} loading={creating}>
          {$t('portfolios.page.create')}
        </Button>
      </div>
    {/if}

    <ErrorText error={error} copyable />

    {#if loading && portfolios.length === 0}
      <div class="grid">
        {#each [0, 1, 2] as i (i)}<Skeleton height="168px" />{/each}
      </div>
    {:else if portfolios.length === 0}
      <EmptyState icon="pie-chart" description={$t('portfolios.card.emptyState')}>
        {#snippet action()}
          <Button variant="primary" icon="plus" onclick={() => (showCreate = true)}>
            {$t('portfolios.page.newPortfolio')}
          </Button>
        {/snippet}
      </EmptyState>
    {:else}
      <OverviewBlock reloadKey={portfolios.length} onopen={(pid) => (selected = pid)} />
      <div class="grid">
        {#each portfolios as p (p.id)}
          {@const gain = unrealizedPct(p.unrealized, p.cost_basis)}
          {@const dayChg = lastChangePct(p.sparkline)}
          <!-- A card is a link to the detail view. It carries a nested delete button, so
               it can't be a <button> (invalid HTML); it gets the keyboard behavior a
               button would have instead of silencing the linter. -->
          <div
            class="card"
            role="button"
            tabindex="0"
            onclick={() => (selected = p.id)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                selected = p.id;
              }
            }}
          >
            <div class="top">
              <h3 title={p.name}>{p.name}</h3>
              <span class="ccy">{p.currency}</span>
              <button class="icon danger-hover del" onclick={(e) => askRemove(e, p)} aria-label={$t('portfolios.card.deleteAria')} title={$t('portfolios.card.deleteAria')}><Icon name="x" size={14} /></button>
            </div>
            {#if p.description}<p class="desc-text" title={p.description}>{p.description}</p>{/if}

            <div class="body">
              <div class="figures">
                <div class="value">{fmtMoney(p.market_value, p.currency)}</div>
                <!-- Unrealized result: glyph and arrow carry the sign, the tone repeats it. -->
                <div class="row">
                  <Delta value={p.unrealized} format={(v) => fmtMoney(v, p.currency)} sensitive empty={$t('portfolios.na')} />
                  <Delta value={gain} arrow={false} empty="" />
                </div>
              </div>
              <div class="sparkwrap">
                <Sparkline
                  values={p.sparkline ?? []}
                  width="100%"
                  height={40}
                  label={$t('portfolios.sparkline.trend')}
                />
              </div>
            </div>

            <div class="meta">
              <span>{$t('portfolios.card.assetCount', { count: p.asset_count })}</span>
              {#if p.auto_refresh}<Badge tone="accent">{$t('portfolios.card.daily')}</Badge>{/if}
              {#if p.refreshed_at}<span>{fmtDate(p.refreshed_at)}</span>{/if}
              <span class="grow"></span>
              {#if dayChg != null}
                <span class="last" title={$t('portfolios.card.lastUpdateHint')}>
                  <span class="lbl">{$t('portfolios.card.lastUpdateShort')}</span>
                  <Delta value={dayChg} />
                </span>
              {:else if (p.sparkline ?? []).length < 2}
                <span class="dim">{$t('portfolios.sparkline.notEnoughHistory')}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <ConfirmModal
    bind:open={confirmOpen}
    title={$t('portfolios.card.deleteAria')}
    message={$t('portfolios.card.confirmDelete')}
    confirmLabel={$t('portfolios.card.deleteAria')}
    cancelLabel={$t('common.cancel')}
    danger
    onconfirm={confirmRemove}
  />
{/if}

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  /* Header is PageHeader.svelte; the buttons are Button.svelte. */
  .create {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    align-items: center;
  }
  .create .desc {
    flex: 1 1 240px;
  }
  .create .ccy {
    width: 96px;
    flex: none;
  }
  /* Cards on a 12px gutter: one surface, one hairline, no frame inside. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 420px), 1fr));
    gap: var(--space-3);
  }
  .card {
    text-align: left;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    color: var(--text);
    transition:
      background-color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .card:hover {
    background: var(--surface-hover);
    border-color: var(--border-control);
  }
  .card:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .top h3 {
    font-size: 14px;
    font-weight: 600;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .top .ccy {
    font-size: 10px;
    letter-spacing: 0.04em;
    color: var(--dim);
    font-family: var(--mono);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-1);
  }
  .del {
    width: 24px;
    height: 24px;
  }
  /* One line: a long description must not push into the figures row below it. */
  .desc-text {
    font-size: 12px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .body {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    margin-top: var(--space-2);
    min-width: 0;
  }
  /* The figures are the content; the sparkline is the one that gives way when the card
     narrows. */
  .figures {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1 1 auto;
    min-width: 0;
  }
  .sparkwrap {
    flex: 0 1 45%;
    min-width: 96px;
    display: flex;
    align-items: center;
    height: 40px;
  }
  .sparkwrap :global(.spark) {
    width: 100%;
  }
  /* A headline figure: proportional digits at 20px, tabular so it does not shimmer on a
     refresh. */
  .value {
    font-size: 20px;
    font-weight: 600;
    line-height: var(--lh-tight);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
    color: var(--dim);
    margin-top: var(--space-3);
    padding-top: var(--space-2);
    border-top: var(--hairline) solid var(--border);
    min-width: 0;
  }
  .grow {
    flex: 1;
  }
  .last {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .lbl {
    color: var(--dim);
  }
  .dim {
    color: var(--dim);
  }
  @media (max-width: 767px) {
    .page {
      padding: var(--space-4) var(--space-4) 80px;
    }
    .create input {
      flex: 1 1 100%;
      min-width: 0;
    }
    .create .ccy {
      flex: 1 1 auto;
    }
    .top,
    .meta {
      flex-wrap: wrap;
    }
    .del {
      min-width: 44px;
      min-height: 44px;
    }
  }
</style>
