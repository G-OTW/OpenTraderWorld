<script>
  // Add a symbol to a watchlist. Pick the class (crypto / stock-etf), search the provider,
  // select an exact match: the provider's own id is stored so pricing is never ambiguous.
  // The backend quotes the new item inline, so it lands in the table with a live price.
  import Modal from '$lib/ui/Modal.svelte';
  import { watchlistsApi } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let { open = $bindable(false), watchlistId, onadded = () => {} } = $props();

  let kind = $state('crypto'); // 'crypto' | 'stock'
  let mode = $state('search'); // 'search' | 'paste'

  // Paste tab: resolve first, add only what the user confirms. An ambiguous ticker offers
  // its exact matches to choose from; an unknown one names the fix. Nothing is guessed.
  let pasteText = $state('');
  let review = $state(null); // { resolved: [{ token, hit, on }], unresolved: [{ token, reason, candidates, pick }] }
  let resolving = $state(false);

  async function resolvePaste() {
    resolving = true;
    error = '';
    try {
      const r = await watchlistsApi.resolveBatch(watchlistId, pasteText, kind);
      review = {
        resolved: r.resolved.map((x) => ({ ...x, on: true })),
        unresolved: r.unresolved.map((x) => ({ ...x, pick: '' }))
      };
    } catch (e) {
      error = e.message;
    } finally {
      resolving = false;
    }
  }

  const confirmed = $derived(
    review
      ? [
          ...review.resolved.filter((r) => r.on).map((r) => r.hit),
          ...review.unresolved
            .filter((u) => u.pick)
            .map((u) => u.candidates.find((c) => c.provider_id === u.pick))
            .filter(Boolean)
        ]
      : []
  );

  async function addConfirmed() {
    if (!confirmed.length) return;
    adding = true;
    error = '';
    try {
      await watchlistsApi.addItems(
        watchlistId,
        confirmed.map((h) => ({
          asset_class: h.asset_class,
          provider: h.provider,
          provider_id: h.provider_id,
          symbol: h.symbol,
          name: h.name
        }))
      );
      pasteText = '';
      review = null;
      onadded();
    } catch (e) {
      error = e.message;
    } finally {
      adding = false;
    }
  }
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
<!-- Stays open after a pick: building a list is a batch activity, close it via Esc/backdrop. -->
<Modal bind:open size="md" title={$t('watchlists.add.title')} onclose={() => ((query = ''), (results = []), (review = null))}>
  <div class="head">
    <div class="tabs modes" role="tablist">
      <button role="tab" aria-selected={mode === 'search'} class:active={mode === 'search'} onclick={() => (mode = 'search')}>{$t('watchlists.add.tabSearch')}</button>
      <button role="tab" aria-selected={mode === 'paste'} class:active={mode === 'paste'} onclick={() => (mode = 'paste')}>{$t('watchlists.add.tabPaste')}</button>
    </div>
    <div class="kinds" role="group">
      <button class="chip" class:active={kind === 'crypto'} aria-pressed={kind === 'crypto'} onclick={() => ((kind = 'crypto'), (review = null))}>{$t('watchlists.add.crypto')}</button>
      <button class="chip" class:active={kind === 'stock'} aria-pressed={kind === 'stock'} onclick={() => ((kind = 'stock'), (review = null))}>{$t('watchlists.add.stockEtf')}</button>
    </div>
  </div>

  {#if mode === 'paste'}
    <textarea
      class="paste"
      rows="4"
      bind:value={pasteText}
      oninput={() => (review = null)}
      placeholder={$t('watchlists.add.pastePlaceholder')}
      aria-label={$t('watchlists.add.tabPaste')}
    ></textarea>
    <div class="pastebar">
      <Button variant={review ? 'secondary' : 'primary'} onclick={resolvePaste} loading={resolving} disabled={!pasteText.trim()}>
        {resolving ? $t('watchlists.add.resolving') : $t('watchlists.add.resolve')}
      </Button>
      {#if review}
        <Button variant="primary" icon="plus" onclick={addConfirmed} loading={adding} disabled={confirmed.length === 0}>
          {$t('watchlists.add.addN', { n: confirmed.length })}
        </Button>
      {/if}
    </div>
    <ErrorText error={error} copyable />
    {#if review}
      <div class="results">
        {#each review.resolved as r (r.token)}
          <label class="hit rev">
            <input type="checkbox" bind:checked={r.on} />
            <span class="sym">{r.hit.symbol}</span>
            <span class="name" title={r.hit.name}>{r.hit.name}</span>
            <Badge>{r.hit.asset_class}</Badge>
          </label>
        {/each}
        {#each review.unresolved as u (u.token)}
          <div class="hit rev bad">
            <span class="warnicon"><Icon name="alert-triangle" size={12} /></span>
            <span class="sym">{u.token}</span>
            {#if u.candidates?.length}
              <select bind:value={u.pick} aria-label={$t('watchlists.add.choose', { token: u.token })}>
                <option value="">{$t('watchlists.add.choose', { token: u.token })}</option>
                {#each u.candidates as c (c.provider_id)}
                  <option value={c.provider_id}>{c.name} ({c.provider_id})</option>
                {/each}
              </select>
            {:else}
              <span class="reason">{u.reason}</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <span class="searchbox">
      <Icon name="search" size={14} />
      <input
        class="search"
        type="search"
        placeholder={kind === 'crypto' ? $t('watchlists.add.searchCryptoPlaceholder') : $t('watchlists.add.searchStockPlaceholder')}
        aria-label={kind === 'crypto' ? $t('watchlists.add.searchCryptoPlaceholder') : $t('watchlists.add.searchStockPlaceholder')}
        bind:value={query}
      />
    </span>

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
            <span class="name" title={r.name}>{r.name}</span>
            <Badge>{r.asset_class}</Badge>
            <span class="addicon"><Icon name="plus" size={13} /></span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</Modal>

<style>
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  .modes {
    border-bottom: none;
    margin-bottom: -1px;
  }
  .kinds {
    display: inline-flex;
    gap: var(--space-1);
    padding-bottom: var(--space-2);
  }
  .paste {
    width: 100%;
    font-family: var(--mono);
    font-size: 12px;
  }
  .pastebar {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin: var(--space-2) 0;
  }
  .rev {
    cursor: default;
    align-items: center;
  }
  .warnicon {
    display: inline-flex;
    color: var(--amber);
  }
  .reason {
    color: var(--muted);
    font-size: 12px;
    flex: 1;
    min-width: 0;
  }
  .rev select {
    flex: 1;
    min-width: 0;
  }
  /* One frame: the input owns the hairline; the glyph sits in its padding. */
  .searchbox {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--dim);
  }
  .searchbox > :global(svg) {
    position: absolute;
    left: var(--space-3);
    pointer-events: none;
  }
  .search {
    width: 100%;
    padding-left: calc(var(--space-3) + 22px);
    font-size: 13px;
  }
  .results {
    margin-top: var(--space-3);
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .hit {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 36px;
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: var(--hairline) solid var(--border);
    border-radius: 0;
    padding: 0 var(--space-2);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .hit:hover {
    background: var(--surface-hover);
  }
  .hit:last-child {
    border-bottom: none;
  }
  .sym {
    font-weight: 600;
    min-width: 72px;
  }
  .name {
    color: var(--muted);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .addicon {
    display: inline-flex;
    color: var(--dim);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .hit:hover .addicon,
  .hit:focus-visible .addicon {
    opacity: 1;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
    padding: var(--space-2);
  }
  @media (max-width: 520px) {
    .head {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
