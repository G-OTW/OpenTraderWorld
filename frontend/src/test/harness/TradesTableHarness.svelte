<script>
  // Test harness: the shared Table the way a module page drives it. The page owns the sort
  // and orders the rows; the table renders headers, cells and the empty state. Money goes
  // through format.js, as on every real page.
  import Table from '$lib/ui/Table.svelte';
  import { fmtSignedMoney } from '$lib/format.js';

  let { trades = [] } = $props();

  const columns = [
    { key: 'ticker', label: 'Ticker', sortable: true },
    { key: 'qty', label: 'Qty', numeric: true },
    { key: 'pnl', label: 'Net PnL', numeric: true, sortable: true }
  ];

  let sort = $state({ key: 'ticker', dir: 'asc' });
  const rows = $derived(
    [...trades].sort((a, b) => {
      const x = a[sort.key];
      const y = b[sort.key];
      const c = typeof x === 'number' ? x - y : String(x).localeCompare(String(y));
      return sort.dir === 'asc' ? c : -c;
    })
  );
</script>

<Table {columns} {rows} {sort} onsort={(s) => (sort = s)}>
  {#snippet cell(row, col)}
    {col.key === 'pnl' ? fmtSignedMoney(row.pnl, 'USD') : row[col.key]}
  {/snippet}
  {#snippet empty()}
    <span>No trades yet.</span>
  {/snippet}
</Table>
