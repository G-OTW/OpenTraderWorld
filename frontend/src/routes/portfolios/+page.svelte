<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Portfolio Tracker — first page: the user's portfolios as wide cards, each with headline figures
  // (value, unrealized PnL, asset count) and a green/red trend sparkline. Create a portfolio inline;
  // click a card to open its full-page detail (assets, operations ledger, value-over-time chart).
  // Prices are CoinGecko (crypto) and Yahoo (stocks/ETF), fetched in USD and converted to each
  // portfolio's display currency.
  import {
    portfoliosApi,
    fmtMoney,
    fmtSignedMoney,
    fmtPct,
    gainPct,
    CURRENCIES
  } from '$lib/modules/portfolios/api.js';
  import PortfolioDetail from '$lib/modules/portfolios/PortfolioDetail.svelte';
  import Sparkline from '$lib/modules/portfolios/Sparkline.svelte';
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
    <PortfolioDetail id={selected} onback={() => (selected = null)} onchanged={load} />
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
        <select bind:value={form.currency}>
          {#each CURRENCIES as c (c)}<option value={c}>{c}</option>{/each}
        </select>
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
      <div class="grid">
        {#each portfolios as p (p.id)}
          {@const gain = gainPct(p.market_value, p.cost_basis)}
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
              <h3>{p.name}</h3>
              <span class="ccy">{p.currency}</span>
              <button class="del" onclick={(e) => askRemove(e, p)} aria-label={$t('portfolios.card.deleteAria')}><Icon name="x" size={13} /></button>
            </div>
            {#if p.description}<p class="desc-text">{p.description}</p>{/if}

            <div class="body">
              <div class="figures">
                <div class="value num">{fmtMoney(p.market_value, p.currency)}</div>
                <div class="row">
                  <!-- Unrealized PnL: the sign is in the text, the color reinforces it. -->
                  <span class="num" class:up={p.unrealized > 0} class:down={p.unrealized < 0}>
                    {fmtSignedMoney(p.unrealized, p.currency)}
                  </span>
                  <span class="pct num" class:up={gain > 0} class:down={gain < 0}>{fmtPct(gain)}</span>
                </div>
                {#if dayChg != null}
                  <span class="daychip" class:up={dayChg > 0} class:down={dayChg < 0}>
                    {$t('portfolios.card.lastUpdate', { pct: fmtPct(dayChg) })}
                  </span>
                {/if}
              </div>
              <div class="sparkwrap">
                <Sparkline values={p.sparkline ?? []} />
              </div>
            </div>

            <div class="meta">
              <span>{$t('portfolios.card.assetCount', { count: p.asset_count })}</span>
              {#if p.auto_refresh}<Badge tone="accent">{$t('portfolios.card.daily')}</Badge>{/if}
              {#if p.refreshed_at}<span class="muted">· {new Date(p.refreshed_at).toLocaleDateString()}</span>{/if}
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
  /* Continuous hairline grid — cells share filets, no gaps, no radius. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .card {
    text-align: left;
    background: var(--bg);
    border: none;
    border-radius: 0;
    padding: var(--space-4);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    color: var(--text);
    transition: background-color var(--dur-fast) var(--ease);
  }
  .card:hover {
    background: var(--surface-2);
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .top h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    flex: 1;
    min-width: 0;
  }
  .ccy {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: 1px var(--space-1);
  }
  .desc-text {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .body {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    margin-top: var(--space-2);
  }
  .figures {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 0 0 auto;
  }
  .sparkwrap {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
  }
  /* The headline figure: the jump from the 13px row below is what reads as a product. */
  .value {
    font-size: var(--text-xl);
    font-weight: var(--fw-normal);
    font-family: var(--mono);
    line-height: var(--lh-tight);
  }
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    font-size: var(--text-base);
  }
  .pct {
    font-size: var(--text-sm);
  }
  .daychip {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    margin-top: var(--space-2);
  }
  /* The "daily" pill is Badge.svelte now. */
  .del {
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease);
  }
  .del:hover {
    color: var(--red);
  }

  /* Color is the second channel; fmtSignedMoney/fmtPct already put the sign in the text. */
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .muted {
    color: var(--muted);
  }
</style>
