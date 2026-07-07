<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  // Portfolio Tracker — first page: the user's portfolios as wide cards, each with headline figures
  // (value, unrealized PnL, asset count) and a green/red trend sparkline. Create a portfolio inline;
  // click a card to open its full-page detail (assets, operations ledger, value-over-time chart).
  // Prices are CoinGecko (crypto) and Yahoo (stocks/ETF), fetched in USD and converted to each
  // portfolio's display currency.
  import { portfoliosApi, fmtMoney, fmtPct, gainPct, CURRENCIES } from '$lib/modules/portfolios/api.js';
  import PortfolioDetail from '$lib/modules/portfolios/PortfolioDetail.svelte';
  import Sparkline from '$lib/modules/portfolios/Sparkline.svelte';
  import { t } from '$lib/i18n';

  let portfolios = $state([]);
  let error = $state('');
  let loading = $state(false);
  let selected = $state(null); // portfolio id of the open full-page detail

  let showCreate = $state(false);
  let form = $state({ name: '', currency: 'USD', description: '' });

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
    if (!form.name.trim()) return;
    try {
      const pf = await portfoliosApi.create(form);
      showCreate = false;
      form = { name: '', currency: 'USD', description: '' };
      load();
      selected = pf.id;
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(e, id) {
    e.stopPropagation();
    if (!confirm($t('portfolios.card.confirmDelete'))) return;
    await portfoliosApi.remove(id);
    load();
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
    <header>
      <h1>{$t('portfolios.page.title')}</h1>
      <button class="btn" onclick={() => (showCreate = !showCreate)}>+ {$t('portfolios.page.newPortfolio')}</button>
    </header>

    {#if showCreate}
      <div class="create">
        <input placeholder={$t('portfolios.page.namePlaceholder')} bind:value={form.name} />
        <select bind:value={form.currency}>
          {#each CURRENCIES as c (c)}<option value={c}>{c}</option>{/each}
        </select>
        <input class="desc" placeholder={$t('portfolios.page.descPlaceholder')} bind:value={form.description} />
        <button class="btn primary" onclick={create}>{$t('portfolios.page.create')}</button>
      </div>
    {/if}

    {#if error}<p class="err" title="click to copy" use:copyLog={error}>{error}</p>{/if}

    <div class="grid">
      {#each portfolios as p (p.id)}
        {@const gain = gainPct(p.market_value, p.cost_basis)}
        {@const dayChg = lastChangePct(p.sparkline)}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="card" role="button" tabindex="0" onclick={() => (selected = p.id)}>
          <div class="top">
            <h3>{p.name}</h3>
            <span class="ccy">{p.currency}</span>
            <button class="del" onclick={(e) => remove(e, p.id)} aria-label={$t('portfolios.card.deleteAria')}><Icon name="x" size={13} /></button>
          </div>
          {#if p.description}<p class="desc-text">{p.description}</p>{/if}

          <div class="body">
            <div class="figures">
              <div class="value">{fmtMoney(p.market_value, p.currency)}</div>
              <div class="row">
                <span class:up={p.unrealized > 0} class:down={p.unrealized < 0}>
                  {fmtMoney(p.unrealized, p.currency)}
                </span>
                <span class="pct" class:up={gain > 0} class:down={gain < 0}>{fmtPct(gain)}</span>
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
            {#if p.auto_refresh}<span class="badge">{$t('portfolios.card.daily')}</span>{/if}
            {#if p.refreshed_at}<span class="muted">· {new Date(p.refreshed_at).toLocaleDateString()}</span>{/if}
          </div>
        </div>
      {/each}
      {#if !loading && portfolios.length === 0}
        <p class="empty">{$t('portfolios.card.emptyState')}</p>
      {/if}
    </div>
  </div>
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
  header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  h1 {
    font-size: 1.4rem;
    font-weight: 700;
  }
  .btn {
    margin-left: auto;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
  }
  .btn.primary {
    margin-left: 0;
    border-color: var(--accent);
  }
  .create {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    align-items: center;
  }
  .create .desc {
    flex: 1 1 240px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
    gap: var(--space-4);
  }
  .card {
    text-align: left;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    color: var(--text);
  }
  .card:hover {
    border-color: var(--accent);
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .top h3 {
    font-size: 1.05rem;
    font-weight: 600;
    flex: 1;
  }
  .ccy {
    font-size: 0.72rem;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px var(--space-1);
  }
  .desc-text {
    font-size: 0.8rem;
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
  .value {
    font-size: 1.5rem;
    font-weight: 700;
  }
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    font-size: 0.9rem;
  }
  .pct {
    font-size: 0.82rem;
  }
  .daychip {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.78rem;
    color: var(--muted);
    margin-top: var(--space-2);
  }
  .badge {
    color: var(--accent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
  }
  .del {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.8rem;
  }
  .del:hover {
    color: var(--red);
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .muted {
    color: var(--muted);
  }
  .empty {
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
</style>
