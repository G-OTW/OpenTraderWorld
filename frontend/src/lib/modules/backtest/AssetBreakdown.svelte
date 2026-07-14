<script>
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';
  // Per-asset performance breakdown for a portfolio run (trades, PnL, win rate, exposure).
  // Hidden for single-asset runs (the portfolio stats already cover it).
  let { perAsset = [] } = $props();
</script>

{#if perAsset.length > 1}
  <div class="wrap">
    <div class="cap">{$t('backtest.asset.title')}</div>
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>{$t('backtest.asset.ticker')}</th>
            <th class="r">{$t('backtest.asset.trades')}</th>
            <th class="r">{$t('backtest.asset.winRate')}</th>
            <th class="r">{$t('backtest.asset.netPnl')}</th>
            <th class="r">{$t('backtest.asset.fees')}</th>
            <th class="r">{$t('backtest.asset.exposure')}</th>
          </tr>
        </thead>
        <tbody>
          {#each perAsset as a (a.ticker)}
            <tr>
              <td class="tk">{a.ticker}</td>
              <td class="r">{fmtNum(a.trades, 0)}</td>
              <td class="r">{fmtNum(a.win_rate, 1)}%</td>
              <td class="r" class:pos={a.net_pnl > 0} class:neg={a.net_pnl < 0}>{fmtNum(a.net_pnl)}</td>
              <td class="r muted">{fmtNum(a.total_fees)}</td>
              <td class="r muted">{fmtNum(a.exposure_pct, 1)}%</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
{/if}

<style>
  .wrap {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .cap {
    font-size: 0.7rem;
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--surface-2) 40%, transparent);
    border-bottom: 1px solid var(--border);
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    padding: var(--space-2) var(--space-3);
    text-align: left;
    white-space: nowrap;
  }
  th {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    font-weight: var(--fw-semibold);
    border-bottom: 1px solid var(--border);
  }
  tbody tr:not(:last-child) td {
    border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }
  .r {
    text-align: right;
  }
  .tk {
    font-weight: var(--fw-semibold);
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
