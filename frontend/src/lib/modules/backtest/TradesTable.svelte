<script>
  // List of trades: every closed trade with entry/exit, size, PnL and
  // running cumulative PnL, plus a chip row summarising how trades ended. Newest last.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { trades = [], exitReasons = {} } = $props();

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

  const rows = $derived.by(() => {
    let cum = 0;
    return trades.map((t, i) => {
      cum += t.pnl;
      return { ...t, n: i + 1, cum };
    });
  });

  const chips = $derived(
    Object.entries(exitReasons ?? {})
      .sort((a, b) => b[1] - a[1])
      .map(([k, v]) => ({ label: REASONS[k] ?? k, count: v }))
  );

  const fmtTs = (ts) => {
    const d = new Date(ts);
    return Number.isNaN(d.getTime())
      ? ts
      : d.toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' });
  };
</script>

<div class="trades">
  {#if chips.length}
    <div class="chips">
      <span class="chips-label">{$t('backtest.trades.exitReasons')}</span>
      {#each chips as c (c.label)}
        <span class="chip">{c.label} <b>{c.count}</b></span>
      {/each}
    </div>
  {/if}

  <div class="wrap">
    <table>
      <thead>
        <tr>
          <th class="num">#</th>
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
          <th>{$t('backtest.trades.cumPnl')}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as tr (tr.n)}
          <tr>
            <td class="num">{tr.n}</td>
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
            <td class:pos={tr.cum > 0} class:neg={tr.cum < 0}>{fmtNum(tr.cum)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
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
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .chip {
    font-size: 0.74rem;
    color: var(--muted);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 10px;
  }
  .chip b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .wrap {
    overflow: auto;
    max-height: 420px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  th,
  td {
    padding: var(--space-1) var(--space-3);
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--surface-2);
    font-size: 0.64rem;
    font-weight: 600;
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
    font-size: 0.76rem;
  }
  tbody tr:nth-child(even) {
    background: color-mix(in srgb, var(--surface) 45%, transparent);
  }
  .side {
    text-transform: capitalize;
    color: var(--green);
    font-weight: 600;
  }
  .side.short {
    color: var(--red);
  }
  .stack {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
