<script>
  // Watchlist widget: a scrollable quote list for one chosen watchlist — name with the
  // ticker below it, price, 24h and 7d change. Config: { watchlist_id }; unset falls back
  // to the first list. Quotes are read from the server cache, same as the module page.
  import { watchlistsApi, fmtQuote, fmtSignedPct } from '$lib/modules/watchlists/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();

  let items = $state(null);
  let noList = $state(false);
  let err = $state('');

  async function load(wantedId) {
    err = '';
    noList = false;
    try {
      const lists = await watchlistsApi.list();
      const id = lists.some((w) => w.id === wantedId) ? wantedId : lists[0]?.id;
      if (!id) {
        noList = true;
        items = [];
        return;
      }
      items = (await watchlistsApi.detail(id)).items;
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    const wanted = item.config?.watchlist_id;
    if (!editing) load(wanted);
  });

  function cls(n) {
    if (n == null || !isFinite(n)) return '';
    return n < 0 ? 'neg' : 'pos';
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.watchlist.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if items === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else if noList}
  <p class="hint">{$t('dashboard.widgets.watchlist.noList')}</p>
{:else if items.length === 0}
  <p class="hint">{$t('dashboard.widgets.watchlist.empty')}</p>
{:else}
  <div class="rows">
    <div class="row head">
      <span></span>
      <span class="num">{$t('watchlists.table.price')}</span>
      <span class="num">{$t('watchlists.table.h24')}</span>
      <span class="num">{$t('watchlists.table.d7')}</span>
    </div>
    {#each items as it (it.id)}
      <div class="row">
        <span class="ident">
          <span class="name" title={it.name}>{it.name}</span>
          <span class="ticker">{it.symbol}</span>
        </span>
        <span class="num price">{fmtQuote(it.quote?.price_usd)}</span>
        <span class="num {cls(it.quote?.change_24h)}">{fmtSignedPct(it.quote?.change_24h)}</span>
        <span class="num {cls(it.quote?.change_7d)}">{fmtSignedPct(it.quote?.change_7d)}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .hint {
    color: var(--dim);
  }
  .sk {
    padding: var(--space-1) 0;
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto auto;
    gap: var(--space-3);
    align-items: center;
    padding: 3px 0;
    font-size: var(--text-sm);
  }
  .row.head {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    border-bottom: 0.5px solid var(--border);
    padding-bottom: var(--space-1);
    margin-bottom: var(--space-1);
    position: sticky;
    top: 0;
    background: var(--bg);
  }
  .ident {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ticker {
    font-size: var(--text-xs);
    color: var(--dim);
    text-transform: uppercase;
  }
  .num {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
