<script>
  // Watchlists — two-pane page: a rail of the user's lists (name, symbol count, auto-sync
  // marker) and the selected list's quotes table. Lists are created blank, from a curated
  // template, or by mirroring a Portfolio Tracker portfolio. Prices are CoinGecko (crypto)
  // and Yahoo (stocks/ETFs) by default, quoted in USD server-side; the Sources tab manages
  // the shared provider connectors a list or a single symbol can quote through instead.
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import WatchlistDetail from '$lib/modules/watchlists/WatchlistDetail.svelte';
  import NewWatchlistModal from '$lib/modules/watchlists/NewWatchlistModal.svelte';
  import ProviderSettings from '$lib/modules/histdata/ProviderSettings.svelte';
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';
  import { histdataApi } from '$lib/modules/histdata/api.js';
  import { t } from '$lib/i18n';

  let lists = $state([]);
  let loading = $state(false);
  let error = $state('');
  let selected = $state(null);
  let showNew = $state(false);
  let tab = $state('lists'); // 'lists' | 'sources'

  // Sources tab data — the same connectors histdata manages (shared store).
  let connectors = $state([]);
  let providers = $state([]);

  async function load(quiet = false) {
    if (!quiet) {
      loading = true;
      error = '';
    }
    try {
      lists = await watchlistsApi.list();
      // Keep a valid selection: fall back to the first list when the current one is gone.
      if (!lists.some((l) => l.id === selected)) selected = lists[0]?.id ?? null;
    } catch (e) {
      if (!quiet) error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    load();
  });

  async function loadSources() {
    try {
      [connectors, providers] = await Promise.all([
        histdataApi.connectors('watchlists'),
        histdataApi.providers()
      ]);
    } catch (e) {
      error = e.message;
    }
  }
  $effect(() => {
    if (tab === 'sources') loadSources();
  });

  function created(wl) {
    selected = wl.id;
    load(true);
  }
</script>

<div class="page">
  <PageHeader title={$t('watchlists.page.title')}>
    {#snippet actions()}
      <Button onclick={() => (tab = tab === 'sources' ? 'lists' : 'sources')}>
        {tab === 'sources' ? $t('watchlists.page.tabLists') : $t('watchlists.page.tabSources')}
      </Button>
      {#if tab === 'lists'}
        <Button icon="plus" variant="primary" onclick={() => (showNew = true)}>
          {$t('watchlists.page.newWatchlist')}
        </Button>
      {/if}
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

  {#if tab === 'sources'}
    <p class="sourceshint">{$t('watchlists.page.sourcesHint')}</p>
    <ProviderSettings {connectors} {providers} scope="watchlists" onchanged={loadSources} />
  {:else}

  {#if loading && lists.length === 0}
    <div class="split">
      <div class="rail"><Skeleton height="120px" /></div>
      <div class="main"><Skeleton height="320px" /></div>
    </div>
  {:else if lists.length === 0}
    <EmptyState icon="star" description={$t('watchlists.page.emptyState')}>
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={() => (showNew = true)}>
          {$t('watchlists.page.newWatchlist')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="split">
      <nav class="rail" aria-label={$t('watchlists.page.title')}>
        {#each lists as l (l.id)}
          <button class="railitem" class:active={l.id === selected} onclick={() => (selected = l.id)}>
            <span class="rname">{l.name}</span>
            <span class="rmeta">
              {#if l.sync_enabled}
                <span class="synced" title={$t('watchlists.detail.sync')}><Icon name="refresh-cw" size={11} /></span>
              {/if}
              <span class="rcount">{l.item_count}</span>
            </span>
          </button>
        {/each}
      </nav>
      <section class="main">
        {#if selected}
          <WatchlistDetail
            id={selected}
            onchanged={() => load(true)}
            ondeleted={() => ((selected = null), load(true))}
          />
        {/if}
      </section>
    </div>
    {/if}
  {/if}
</div>

<NewWatchlistModal bind:open={showNew} oncreated={created} />

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  .sourceshint {
    color: var(--muted);
    font-size: var(--text-sm);
    max-width: 72ch;
  }
  .split {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
    min-height: 0;
  }
  .rail {
    flex: 0 0 220px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    position: sticky;
    top: 0;
  }
  .railitem {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-left: 1.5px solid transparent;
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    cursor: pointer;
    text-align: left;
    font-size: var(--fs-body);
  }
  .railitem:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .railitem.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .rname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .rmeta {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex: none;
  }
  .synced {
    color: var(--accent);
    display: inline-flex;
  }
  .rcount {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: 0 var(--space-2);
    line-height: 18px;
  }
  .main {
    flex: 1;
    min-width: 0;
  }

  @media (max-width: 720px) {
    .split {
      flex-direction: column;
    }
    .rail {
      flex: none;
      width: 100%;
      flex-direction: row;
      flex-wrap: wrap;
      position: static;
    }
  }
</style>
