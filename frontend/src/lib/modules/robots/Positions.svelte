<script>
  // What the robot holds, at its own size, marked to the live price when the hub streams it
  // and to the last stored close otherwise. The server's `unrealized` is the stored mark; a
  // live mark re-reads it from the same quantity and average price, nothing else.
  import Table from '$lib/ui/Table.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';
  import { shortDuration } from './fleet.js';

  let { positions = [], quotes = {}, multiplier = 1 } = $props();

  const fmt = (v, d = 4) => (typeof v === 'number' && Number.isFinite(v) ? v.toLocaleString(undefined, { maximumFractionDigits: d }) : '-');
  const signed = (v) => (typeof v === 'number' && Number.isFinite(v) ? `${v > 0 ? '+' : ''}${fmt(v, 2)}` : '-');
  const rows = $derived(
    positions.map((p) => {
      const live = quotes[p.ticker];
      const mark = live?.price ?? p.price;
      const sign = p.direction === 'short' ? -1 : 1;
      const unrealized = live && Number.isFinite(p.avg_price) ? (mark - p.avg_price) * p.qty * multiplier * sign : p.unrealized;
      return { ...p, mark, live: !!live, unrealized };
    })
  );
  const columns = $derived([
    { key: 'ticker', label: $t('robots.col.instrument') },
    { key: 'direction', label: $t('robots.col.side') },
    { key: 'qty', label: $t('robots.col.qty'), numeric: true },
    { key: 'avg_price', label: $t('robots.col.avg'), numeric: true },
    { key: 'mark', label: $t('robots.col.price'), numeric: true },
    { key: 'unrealized', label: $t('robots.col.unrealized'), numeric: true },
    { key: 'stop', label: $t('robots.col.stop'), numeric: true },
    { key: 'target', label: $t('robots.col.target'), numeric: true },
    { key: 'age_secs', label: $t('robots.col.age'), numeric: true }
  ]);
</script>

{#if rows.length}
  <div class="scroll">
    <Table {columns} {rows} rowKey={(r) => r.key}>
      {#snippet cell(r, col)}
        {#if col.key === 'direction'}
          <span class:pos={r.direction === 'long'} class:neg={r.direction === 'short'}>{$t(`robots.dashboard.${r.direction}`)}</span>
        {:else if col.key === 'qty'}
          {fmt(r.qty, 8)}{#if r.scale && r.scale !== '1'}<small> x{r.scale}</small>{/if}
        {:else if col.key === 'mark'}
          {fmt(r.mark)}<small class:live={r.live}>{r.live ? $t('robots.live') : $t('robots.stored')}</small>
        {:else if col.key === 'unrealized'}
          <span class:pos={r.unrealized > 0} class:neg={r.unrealized < 0}>{signed(r.unrealized)}</span>
        {:else if col.key === 'age_secs'}
          {shortDuration(r.age_secs)}
        {:else if col.key === 'ticker'}
          <b>{r.ticker}</b>
        {:else}
          {fmt(r[col.key])}
        {/if}
      {/snippet}
    </Table>
  </div>
{:else}
  <EmptyState icon="layers" title={$t('robots.dashboard.noPositions')} description={$t('robots.dashboard.noPositionsHint')} compact />
{/if}

<style>
  .scroll {
    overflow-x: auto;
  }
  b {
    font-weight: var(--fw-medium);
  }
  /* Scale and quote source: small caps tags beside the figure, inheriting its mono. */
  small {
    margin-left: var(--space-1);
    color: var(--dim);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  small.live {
    color: var(--green);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
