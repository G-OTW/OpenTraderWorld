<script>
  // List of trades: every closed trade with entry/exit, size, PnL and
  // running cumulative PnL, plus a chip row summarising how trades ended. Newest last.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { trades = [], exitReasons = {} } = $props();

  // Show the ticker column only for a multi-asset run (any trade carries a ticker).
  const showTicker = $derived(trades.some((t) => t.ticker));

  const REASON_KEYS = {
    signal: 'backtest.trades.reason.signal',
    exit_signal: 'backtest.trades.reason.exitSignal',
    stop_loss: 'backtest.trades.reason.stopLoss',
    take_profit: 'backtest.trades.reason.takeProfit',
    reverse: 'backtest.trades.reason.reverse',
    end: 'backtest.trades.reason.end'
  };
  const REASONS = $derived(
    Object.fromEntries(Object.entries(REASON_KEYS).map(([k, key]) => [k, $t(key)]))
  );

  // Filter by exit reason ('all' or a reason key). Cumulative PnL is computed over the full
  // (unfiltered) sequence so the "cum" column stays meaningful when filtered.
  let filter = $state('all');
  // Reset to "all" if the current filter has no trades in a new result.
  $effect(() => {
    if (filter !== 'all' && !trades.some((t) => t.exit_reason === filter)) filter = 'all';
  });

  const filtered = $derived.by(() => {
    let cum = 0;
    return trades
      .map((t, i) => {
        cum += t.pnl;
        return { ...t, n: i + 1, cum };
      })
      .filter((t) => filter === 'all' || t.exit_reason === filter);
  });

  // Pagination: 100 rows per page over the filtered set. Reset to page 1 whenever the filtered
  // set changes (new result or filter switch).
  const PAGE_SIZE = 100;
  let pageNum = $state(1);
  const pageCount = $derived(Math.max(1, Math.ceil(filtered.length / PAGE_SIZE)));
  $effect(() => {
    filtered.length; // reset paging when the visible set changes
    pageNum = 1;
  });
  const start = $derived((pageNum - 1) * PAGE_SIZE);
  const rows = $derived(filtered.slice(start, start + PAGE_SIZE));

  // Filter chips: "All" + one per exit reason present, with counts.
  const chips = $derived([
    { key: 'all', label: $t('backtest.trades.filterAll'), count: trades.length },
    ...Object.entries(exitReasons ?? {})
      .sort((a, b) => b[1] - a[1])
      .map(([k, v]) => ({ key: k, label: REASONS[k] ?? k, count: v }))
  ]);

  const fmtTs = (ts) => {
    const d = new Date(ts);
    return Number.isNaN(d.getTime())
      ? ts
      : d.toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' });
  };
</script>

<div class="trades">
  {#if chips.length > 1}
    <div class="chips">
      <span class="chips-label">{$t('backtest.trades.filterBy')}</span>
      {#each chips as c (c.key)}
        <button type="button" class="chip" class:on={filter === c.key} onclick={() => (filter = c.key)}>
          {c.label} <b>{c.count}</b>
        </button>
      {/each}
    </div>
  {/if}

  <div class="wrap">
    <table>
      <thead>
        <tr>
          <th class="num">#</th>
          {#if showTicker}<th class="left">{$t('backtest.trades.asset')}</th>{/if}
          <th class="left">{$t('backtest.trades.side')}</th>
          <th class="left">{$t('backtest.trades.entry')}</th>
          <th>{$t('backtest.trades.entryPrice')}</th>
          <th class="left">{$t('backtest.trades.exit')}</th>
          <th>{$t('backtest.trades.exitPrice')}</th>
          <th class="left">{$t('backtest.trades.reasonCol')}</th>
          <th>{$t('backtest.trades.qty')}</th>
          <th>{$t('backtest.trades.bars')}</th>
          <th>{$t('backtest.trades.pnl')}</th>
          <th>{$t('backtest.trades.pnlPct')}</th>
          <th title={$t('backtest.trades.maeTitle')}>{$t('backtest.trades.mae')}</th>
          <th title={$t('backtest.trades.mfeTitle')}>{$t('backtest.trades.mfe')}</th>
          <th>{$t('backtest.trades.cumPnl')}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as tr (tr.n)}
          <tr>
            <td class="num">{tr.n}</td>
            {#if showTicker}<td class="left tk">{tr.ticker}</td>{/if}
            <td class="left side" class:short={tr.direction === 'short'}>{tr.direction}</td>
            <td class="left ts">{fmtTs(tr.entry_ts)}</td>
            <td>{fmtNum(tr.entry_price)}</td>
            <td class="left ts">{fmtTs(tr.exit_ts)}</td>
            <td>{fmtNum(tr.exit_price)}</td>
            <td class="left reason">{REASONS[tr.exit_reason] ?? tr.exit_reason}</td>
            <td>{fmtNum(tr.qty, 4)}{#if tr.entries > 1}<span class="stack" title={$t('backtest.trades.stackedEntries', { count: tr.entries })}> ×{tr.entries}</span>{/if}</td>
            <td>{fmtNum(tr.bars_held, 0)}</td>
            <td class:pos={tr.pnl > 0} class:neg={tr.pnl < 0}>{fmtNum(tr.pnl)}</td>
            <td class:pos={tr.return_pct > 0} class:neg={tr.return_pct < 0}>{fmtNum(tr.return_pct)}%</td>
            <td class="muted">{fmtNum(tr.mae)}</td>
            <td class="muted">{fmtNum(tr.mfe)}</td>
            <td class:pos={tr.cum > 0} class:neg={tr.cum < 0}>{fmtNum(tr.cum)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if filtered.length > PAGE_SIZE}
    <div class="pager">
      <span class="showing">
        {$t('backtest.trades.showing', {
          from: (start + 1).toLocaleString(),
          to: Math.min(start + PAGE_SIZE, filtered.length).toLocaleString(),
          total: filtered.length.toLocaleString()
        })}
      </span>
      <div class="pager-btns">
        <button type="button" disabled={pageNum <= 1} onclick={() => (pageNum -= 1)}>{$t('backtest.trades.prev')}</button>
        <span class="pnum">{$t('backtest.trades.pageOf', { page: pageNum, pages: pageCount })}</span>
        <button type="button" disabled={pageNum >= pageCount} onclick={() => (pageNum += 1)}>{$t('backtest.trades.next')}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .trades {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .chips {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .chips-label {
    font-size: 0.66rem;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .chip {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: 2px 10px;
    cursor: pointer;
    font-family: inherit;
    transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  .chip:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  .chip.on {
    color: var(--text);
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .chip b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .pager {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .showing {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .pager-btns {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .pager-btns button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    font-family: inherit;
    padding: 3px 12px;
    cursor: pointer;
  }
  .pager-btns button:hover:not(:disabled) {
    border-color: var(--border-control);
  }
  .pager-btns button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .pnum {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    min-width: 6em;
    text-align: center;
  }
  .wrap {
    overflow: auto;
    max-height: 70vh;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    padding: var(--space-1) var(--space-3);
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  th,
  .left {
    font-family: var(--font);
  }
  th {
    position: sticky;
    top: 0;
    background: var(--surface-2);
    font-size: 0.64rem;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    z-index: 1;
  }
  .left {
    text-align: left;
  }
  .num {
    color: var(--muted);
    text-align: right;
  }
  .ts,
  .reason {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  tbody tr:nth-child(even) {
    background: color-mix(in srgb, var(--surface) 45%, transparent);
  }
  .side {
    text-transform: capitalize;
    color: var(--green);
    font-weight: var(--fw-medium);
  }
  .side.short {
    color: var(--red);
  }
  .stack {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .tk {
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--muted);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
