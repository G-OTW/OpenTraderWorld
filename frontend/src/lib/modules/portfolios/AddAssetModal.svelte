<script>
  // Add an asset to a portfolio. Pick the class (crypto / stock-etf), search the provider, and
  // select an exact match: we store the provider's own id so pricing is never ambiguous
  // (BTC vs BTC/USD vs BTC-USD). Crypto → CoinGecko, stocks/ETFs → Yahoo.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { portfoliosApi, CURRENCIES } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let {
    open = $bindable(false),
    portfolioId,
    // Portfolio display currency: the default for operations on the new asset.
    portfolioCurrency = 'USD',
    onadded = () => {}
  } = $props();

  // Currency the asset's operations will be entered in (an asset can trade in a currency
  // other than the portfolio's display one). Re-defaults each time the modal opens.
  let currency = $state('USD');
  $effect(() => {
    if (open) currency = portfolioCurrency;
  });

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
        name: hit.name,
        currency
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

<Modal bind:open size="md" fit title={$t('portfolios.addAsset.title')} onclose={() => ((query = ''), (results = []))}>
  <div class="kinds" role="group" aria-label={$t('portfolios.addAsset.title')}>
    <button class:active={kind === 'crypto'} aria-pressed={kind === 'crypto'} onclick={() => (kind = 'crypto')}>{$t('portfolios.addAsset.crypto')}</button>
    <button class:active={kind === 'stock'} aria-pressed={kind === 'stock'} onclick={() => (kind = 'stock')}>{$t('portfolios.addAsset.stockEtf')}</button>
  </div>

  <!-- One frame: the label owns the hairline and the focus colour, the input is bare. -->
  <label class="search">
    <Icon name="search" size={14} />
    <input
      type="search"
      placeholder={kind === 'crypto' ? $t('portfolios.addAsset.searchCryptoPlaceholder') : $t('portfolios.addAsset.searchStockPlaceholder')}
      bind:value={query}
    />
  </label>

  <div class="ccy">
    <span>{$t('portfolios.addAsset.currency')}</span>
    <Dropdown
      bind:value={currency}
      ariaLabel={$t('portfolios.addAsset.currency')}
      options={CURRENCIES.map((c) => ({ value: c, label: c }))}
    />
    <small>{$t('portfolios.addAsset.currencyHint')}</small>
  </div>

  <ErrorText error={error} copyable />

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
  .ccy {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: var(--space-3) 0;
    font-size: 12px;
    color: var(--muted);
  }
  .ccy small {
    color: var(--dim);
    font-size: 12px;
  }
  .kinds {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    padding: 2px;
    margin-bottom: var(--space-3);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
  }
  .kinds button {
    height: 28px;
    background: transparent;
    border: 0;
    border-radius: calc(var(--radius-sm) - 2px);
    color: var(--muted);
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    transition:
      background-color 160ms ease,
      color 160ms ease;
  }
  .kinds button:hover {
    color: var(--text);
  }
  .kinds button.active {
    color: var(--text);
    background: var(--surface-2);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: var(--control-h);
    padding: 0 var(--space-3);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--dim);
    transition: border-color 160ms ease;
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
    color: var(--text);
    font-size: 13px;
  }
  .search input:focus {
    outline: none;
    box-shadow: none;
  }
  .search input::placeholder {
    color: var(--dim);
  }
  .results {
    max-height: min(420px, 50vh);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .hit {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 36px;
    text-align: left;
    background: transparent;
    border: 0;
    border-bottom: var(--hairline) solid var(--border);
    padding: 0 var(--space-2);
    color: var(--text);
    cursor: pointer;
    font-size: 12px;
    transition: background-color 140ms ease;
  }
  .hit:hover {
    background: var(--surface-hover);
  }
  .sym {
    font-family: var(--mono);
    font-weight: 600;
    min-width: 72px;
  }
  .name {
    color: var(--muted);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cls {
    font-size: 10px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--dim);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
  }
  .muted {
    color: var(--dim);
    font-size: 12px;
    padding: var(--space-2) 0;
  }
</style>
