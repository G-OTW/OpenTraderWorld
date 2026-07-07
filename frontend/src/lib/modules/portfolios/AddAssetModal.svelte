<script>
  import { copyLog } from '$lib/ui/copyLog.js';
  // Add an asset to a portfolio. Pick the class (crypto / stock-etf), search the provider, and
  // select an exact match — we store the provider's own id so pricing is never ambiguous
  // (BTC vs BTC/USD vs BTC-USD). Crypto → CoinGecko, stocks/ETFs → Yahoo.
  import Modal from '$lib/ui/Modal.svelte';
  import { portfoliosApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), portfolioId, onadded = () => {} } = $props();

  let kind = $state('crypto'); // 'crypto' | 'stock'
  let query = $state('');
  let results = $state([]);
  let searching = $state(false);
  let error = $state('');
  let adding = $state(false);

  let debounce;
  $effect(() => {
    query;
    kind;
    clearTimeout(debounce);
    if (!query.trim()) {
      results = [];
      return;
    }
    debounce = setTimeout(runSearch, 250);
    return () => clearTimeout(debounce);
  });

  async function runSearch() {
    searching = true;
    error = '';
    try {
      results = await portfoliosApi.search(kind, query);
    } catch (e) {
      error = e.message;
      results = [];
    } finally {
      searching = false;
    }
  }

  async function pick(hit) {
    adding = true;
    error = '';
    try {
      await portfoliosApi.addAsset(portfolioId, {
        asset_class: hit.asset_class,
        provider: hit.provider,
        provider_id: hit.provider_id,
        symbol: hit.symbol,
        name: hit.name
      });
      open = false;
      query = '';
      results = [];
      onadded();
    } catch (e) {
      error = e.message;
    } finally {
      adding = false;
    }
  }
</script>

<Modal bind:open size="md" title={$t('portfolios.addAsset.title')} onclose={() => ((query = ''), (results = []))}>
  <div class="kinds">
    <button class:active={kind === 'crypto'} onclick={() => (kind = 'crypto')}>{$t('portfolios.addAsset.crypto')}</button>
    <button class:active={kind === 'stock'} onclick={() => (kind = 'stock')}>{$t('portfolios.addAsset.stockEtf')}</button>
  </div>

  <input
    class="search"
    type="search"
    placeholder={kind === 'crypto' ? $t('portfolios.addAsset.searchCryptoPlaceholder') : $t('portfolios.addAsset.searchStockPlaceholder')}
    bind:value={query}
  />

  {#if error}<p class="err" title="click to copy" use:copyLog={error}>{error}</p>{/if}

  <div class="results">
    {#if searching}
      <p class="muted">{$t('portfolios.addAsset.searching')}</p>
    {:else if results.length === 0 && query.trim()}
      <p class="muted">{$t('portfolios.addAsset.noMatches')}</p>
    {:else}
      {#each results as r (r.provider + r.provider_id)}
        <button class="hit" disabled={adding} onclick={() => pick(r)}>
          <span class="sym">{r.symbol}</span>
          <span class="name">{r.name}</span>
          <span class="cls">{r.asset_class}</span>
        </button>
      {/each}
    {/if}
  </div>
</Modal>

<style>
  .kinds {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .kinds button {
    flex: 1;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.88rem;
  }
  .kinds button.active {
    color: var(--text);
    border-color: var(--accent);
  }
  .search {
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.9rem;
  }
  .results {
    margin-top: var(--space-3);
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .hit {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .hit:hover {
    background: var(--surface-2);
    border-color: var(--border);
  }
  .sym {
    font-weight: 700;
    min-width: 64px;
  }
  .name {
    color: var(--muted);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cls {
    font-size: 0.72rem;
    text-transform: uppercase;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px var(--space-1);
  }
  .muted {
    color: var(--muted);
  }
  .err {
    color: var(--red);
    margin-top: var(--space-2);
  }
</style>
