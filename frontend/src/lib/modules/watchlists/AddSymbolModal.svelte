<script>
  // Add a symbol to a watchlist. Pick the class (crypto / stock-etf), search the provider,
  // select an exact match — the provider's own id is stored so pricing is never ambiguous.
  // The backend quotes the new item inline, so it lands in the table with a live price.
  import Modal from '$lib/ui/Modal.svelte';
  import { watchlistsApi } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { open = $bindable(false), watchlistId, onadded = () => {} } = $props();

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
      results = await watchlistsApi.search(kind, query);
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
      await watchlistsApi.addItem(watchlistId, {
        asset_class: hit.asset_class,
        provider: hit.provider,
        provider_id: hit.provider_id,
        symbol: hit.symbol,
        name: hit.name
      });
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

<!-- Stays open after a pick: building a list is a batch activity, close it via Esc/backdrop. -->
<Modal bind:open size="md" title={$t('watchlists.add.title')} onclose={() => ((query = ''), (results = []))}>
  <div class="kinds">
    <button class:active={kind === 'crypto'} onclick={() => (kind = 'crypto')}>{$t('watchlists.add.crypto')}</button>
    <button class:active={kind === 'stock'} onclick={() => (kind = 'stock')}>{$t('watchlists.add.stockEtf')}</button>
  </div>

  <input
    class="search"
    type="search"
    placeholder={kind === 'crypto' ? $t('watchlists.add.searchCryptoPlaceholder') : $t('watchlists.add.searchStockPlaceholder')}
    bind:value={query}
  />

  <ErrorText error={error} copyable />

  <div class="results">
    {#if searching}
      <p class="muted">{$t('watchlists.add.searching')}</p>
    {:else if results.length === 0 && query.trim()}
      <p class="muted">{$t('watchlists.add.noMatches')}</p>
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
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-2);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-base);
  }
  .kinds button.active {
    color: var(--text);
    border-color: var(--accent);
  }
  .search {
    width: 100%;
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
  }
  .search::placeholder {
    color: var(--dim);
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
    border: 0.5px solid transparent;
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .hit:hover {
    background: var(--surface-2);
  }
  .sym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
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
    font-size: var(--text-xs);
    text-transform: uppercase;
    color: var(--muted);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: 1px var(--space-1);
  }
  .muted {
    color: var(--muted);
  }
</style>
