<script>
  // What is working: the engine's intents (a market entry filled at the next open, a resting
  // limit) and, for a robot on a broker, the orders its last sync saw at the venue.
  import Table from '$lib/ui/Table.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { pending = [] } = $props();

  const fmt = (v, d = 4) => {
    const n = typeof v === 'string' ? Number(v) : v;
    return typeof n === 'number' && Number.isFinite(n) ? n.toLocaleString(undefined, { maximumFractionDigits: d }) : '-';
  };
  const when = (v) => (v && Number.isFinite(Date.parse(v)) ? new Date(v).toLocaleString() : '-');
  const rows = $derived(pending.map((p, i) => ({ ...p, _k: `${p.source}-${p.client_id ?? i}` })));
  const columns = $derived([
    { key: 'source', label: $t('robots.col.source') },
    { key: 'ticker', label: $t('robots.col.instrument') },
    { key: 'what', label: $t('robots.col.order') },
    { key: 'qty', label: $t('robots.col.qty'), numeric: true },
    { key: 'price', label: $t('robots.col.orderPrice'), numeric: true },
    { key: 'status', label: $t('robots.col.status') }
  ]);
</script>

{#if rows.length}
  <div class="scroll">
    <Table {columns} {rows} rowKey={(r) => r._k}>
      {#snippet cell(r, col)}
        {#if col.key === 'source'}
          {$t(`robots.pending.${r.source}`)}
        {:else if col.key === 'what'}
          {#if r.source === 'engine'}
            {$t(`robots.dashboard.${r.direction ?? 'long'}`)} · {r.order === 'market' ? $t('robots.dashboard.marketOrder') : $t('robots.dashboard.limitOrder')}
          {:else}
            {r.side} · {r.role}
          {/if}
        {:else if col.key === 'price'}
          {#if r.source === 'engine'}{r.order === 'market' ? $t('robots.dashboard.nextPrice') : fmt(r.price)}{:else}{fmt(r.limit ?? r.stop)}{/if}
        {:else if col.key === 'status'}
          {#if r.source === 'engine'}{$t('robots.dashboard.barsLeft', { n: r.bars_left ?? 0 })} · <span class="t">{when(r.signal_ts)}</span>{:else}{r.status}{/if}
        {:else if col.key === 'qty'}
          {fmt(r.qty, 8)}
        {:else}
          <b>{r[col.key] ?? '-'}</b>
        {/if}
      {/snippet}
    </Table>
  </div>
{:else}
  <EmptyState icon="clipboard-list" title={$t('robots.dashboard.noOrders')} description={$t('robots.dashboard.noOrdersHint')} compact />
{/if}

<style>
  .scroll {
    overflow-x: auto;
  }
  b {
    font-weight: var(--fw-medium);
  }
  /* A time inside a text column still reads as a figure. */
  .t {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
</style>
