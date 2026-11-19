<script>
  // Watchlists: two-pane page. A rail of the user's lists (name, symbol count, auto-sync
  // marker) and the selected list, whose own header carries the page title (the list
  // name), its actions and the Connectors shortcut. Lists are created blank, from a
  // curated template, or by mirroring a Portfolio Tracker portfolio. Prices are CoinGecko
  // (crypto) and Yahoo (stocks/ETFs) by default, quoted in USD server-side; a list or a
  // single symbol can instead quote through a connector from the shared data broker (no
  // Sources tab: the broker is the same everywhere).
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import WatchlistDetail from '$lib/modules/watchlists/WatchlistDetail.svelte';
  import NewWatchlistModal from '$lib/modules/watchlists/NewWatchlistModal.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';
  import { t } from '$lib/i18n';

  let lists = $state([]);
  let loading = $state(false);
  let error = $state('');
  let selected = $state(null);
  let showNew = $state(false);
  let connectorsTick = $state(0);

  // `?list=<id>` opens straight onto that list: how a fired price-alert notification links
  // back to the symbol it fired on. Consumed once, on the first load: after that the rail
  // owns the selection, or the param would keep yanking it back on every refresh.
  let deepLink = typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('list');

  async function load(quiet = false) {
    if (!quiet) {
      loading = true;
      error = '';
    }
    try {
      lists = await watchlistsApi.list();
      if (deepLink && lists.some((l) => l.id === deepLink)) {
        selected = deepLink;
        deepLink = null;
      } else if (!lists.some((l) => l.id === selected)) {
        // Keep a valid selection: fall back to the first list when the current one is gone.
        selected = lists[0]?.id ?? null;
      }
    } catch (e) {
      if (!quiet) error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    load();
  });

  function created(wl) {
    selected = wl.id;
    load(true);
  }
</script>

<div class="page">
  <ErrorText error={error} copyable />

  {#if loading && lists.length === 0}
    <div class="split">
      <div class="rail"><Skeleton height="120px" /></div>
      <div class="main"><Skeleton height="320px" /></div>
    </div>
  {:else if lists.length === 0}
    <PageHeader title={$t('watchlists.page.title')}>
      {#snippet actions()}
        <ConnectorButton module="watchlists" onchanged={() => (connectorsTick++, load(true))} />
        <Button icon="plus" variant="primary" onclick={() => (showNew = true)}>
          {$t('watchlists.page.newWatchlist')}
        </Button>
      {/snippet}
    </PageHeader>
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
        <div class="railhead">
          <span class="raillbl">{$t('watchlists.page.title')}</span>
          <button
            class="icon"
            onclick={() => (showNew = true)}
            aria-label={$t('watchlists.page.newWatchlist')}
            title={$t('watchlists.page.newWatchlist')}
          >
            <Icon name="plus" size={14} />
          </button>
        </div>
        {#each lists as l (l.id)}
          <button class="railitem" class:active={l.id === selected} aria-current={l.id === selected ? 'page' : undefined} onclick={() => (selected = l.id)} title={l.name}>
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
            {connectorsTick}
            onchanged={() => load(true)}
            ondeleted={() => ((selected = null), load(true))}
          />
        {/if}
      </section>
    </div>
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
  .split {
    display: flex;
    gap: var(--space-6);
    align-items: flex-start;
    min-height: 0;
  }
  .rail {
    flex: 0 0 208px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    position: sticky;
    top: 0;
  }
  .railhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0 var(--space-2) var(--space-3);
    margin-bottom: var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
  }
  .raillbl {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .railitem {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    height: 32px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    padding: 0 var(--space-3);
    color: var(--muted);
    cursor: pointer;
    text-align: left;
    font-size: 13px;
    transition:
      background-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  .railitem:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .railitem.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 500;
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
    gap: var(--space-2);
    flex: none;
  }
  .synced {
    color: var(--dim);
    display: inline-flex;
  }
  .railitem.active .synced {
    color: var(--accent);
  }
  .rcount {
    font-size: 11px;
    color: var(--dim);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .main {
    flex: 1;
    min-width: 0;
  }

  /* Below 1180px the table needs the width more than the rail does: the lists become a
     wrapping row of tabs above the list. */
  @media (max-width: 1180px) {
    .split {
      flex-direction: column;
      flex: none;
      min-height: auto;
      gap: var(--space-4);
    }
    .rail {
      flex: none;
      width: 100%;
      flex-direction: row;
      flex-wrap: wrap;
      gap: var(--space-1);
      position: static;
    }
    .railhead {
      flex: 1 1 100%;
    }
    .railitem {
      max-width: 100%;
      min-width: 0;
      border: var(--hairline) solid var(--border);
    }
    .main {
      flex: none;
      width: 100%;
    }
  }
  @media (max-width: 720px) {
    .page {
      padding: var(--space-4) var(--space-4) 80px;
    }
    .railitem {
      min-height: 44px;
    }
  }
</style>
