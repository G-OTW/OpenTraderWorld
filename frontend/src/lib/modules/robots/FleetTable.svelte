<script>
  // The fleet on a desktop: one line per robot, sortable, the same figures as the cards.
  import Table from '$lib/ui/Table.svelte';
  import RobotBadges from './RobotBadges.svelte';
  import { t } from '$lib/i18n';
  import { age, countdown } from './fleet.js';

  let { robots = [], now = Date.now() } = $props();

  let sort = $state({ key: 'priority', dir: 'asc' });
  const money = (v) => (typeof v === 'number' && Number.isFinite(v) ? v.toLocaleString(undefined, { maximumFractionDigits: 2 }) : '-');
  const signed = (v) => (typeof v === 'number' && Number.isFinite(v) ? `${v > 0 ? '+' : ''}${money(v)}` : '-');
  const columns = $derived([
    { key: 'name', label: $t('robots.col.robot'), sortable: true },
    { key: 'state', label: $t('robots.col.state') },
    { key: 'tickers', label: $t('robots.col.instrument') },
    { key: 'account', label: $t('robots.col.account'), sortable: true },
    { key: 'equity', label: $t('robots.fleet.equity'), numeric: true, sortable: true },
    { key: 'today_pnl', label: $t('robots.fleet.today'), numeric: true, sortable: true },
    { key: 'open', label: $t('robots.fleet.open'), numeric: true, sortable: true },
    { key: 'unseen', label: $t('robots.fleet.unseen'), numeric: true, sortable: true },
    { key: 'next_run_at', label: $t('robots.fleet.next'), numeric: true, sortable: true },
    { key: 'exec_synced_at', label: $t('robots.fleet.synced'), numeric: true },
    { key: 'priority', label: $t('robots.col.priority'), numeric: true, sortable: true }
  ]);
  const rows = $derived.by(() => {
    const list = [...robots];
    const k = sort.key;
    const d = sort.dir === 'asc' ? 1 : -1;
    list.sort((a, b) => {
      const x = a[k] ?? '';
      const y = b[k] ?? '';
      return (typeof x === 'number' && typeof y === 'number' ? x - y : String(x).localeCompare(String(y))) * d;
    });
    return list;
  });
</script>

<div class="scroll" data-fleet-table>
  <Table {columns} {rows} {sort} onsort={(s) => (sort = s)}>
    {#snippet cell(r, col)}
      {#if col.key === 'name'}
        <a href={`/robots/${r.id}`}><b>{r.name}</b></a>{#if r.strategy}<small>{r.strategy}</small>{/if}
      {:else if col.key === 'state'}
        <RobotBadges robot={r} />
      {:else if col.key === 'tickers'}
        {(r.tickers ?? []).join(', ')}
      {:else if col.key === 'account'}
        {r.account ?? $t('robots.env.simulated')}
      {:else if col.key === 'equity'}
        {money(r.equity)}
      {:else if col.key === 'today_pnl'}
        <span class:pos={r.today_pnl > 0} class:neg={r.today_pnl < 0}>{signed(r.today_pnl)}</span>
      {:else if col.key === 'unseen'}
        {#if r.unseen > 0}<a class="unseen" href={`/robots/${r.id}`}>{r.unseen}</a>{:else}-{/if}
      {:else if col.key === 'next_run_at'}
        {countdown(r.next_run_at, now) ?? '-'}
      {:else if col.key === 'exec_synced_at'}
        {age(r.exec_synced_at, now) ?? '-'}
      {:else}
        {r[col.key] ?? '-'}
      {/if}
    {/snippet}
  </Table>
</div>

<style>
  .scroll {
    overflow-x: auto;
  }
  a {
    color: var(--text);
    text-decoration: none;
  }
  /* The accent is for orientation: a hovered name underlines instead of changing hue. */
  a:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  b {
    font-weight: var(--fw-medium);
  }
  small {
    display: block;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  .unseen {
    color: var(--text);
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
