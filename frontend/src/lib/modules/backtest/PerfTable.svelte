<script>
  // Performance summary table: one metric per row, columns for
  // All / Long / Short. Reads the scoped SideStats blocks the engine returns.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { stats = {} } = $props();

  const scopes = $derived([stats.all ?? {}, stats.long ?? {}, stats.short ?? {}]);

  // fmt: currency-ish number (signed tint), pct, ratio, int, bars
  const ROW_DEFS = [
    { labelKey: 'backtest.perf.netProfit', key: 'net_pnl', fmt: 'signed' },
    { labelKey: 'backtest.perf.grossProfit', key: 'gross_profit', fmt: 'num' },
    { labelKey: 'backtest.perf.grossLoss', key: 'gross_loss', fmt: 'neg' },
    { labelKey: 'backtest.perf.profitFactor', key: 'profit_factor', fmt: 'ratio' },
    { labelKey: 'backtest.perf.totalFees', key: 'total_fees', fmt: 'num' },
    { labelKey: 'backtest.perf.totalTrades', key: 'trades', fmt: 'int' },
    { labelKey: 'backtest.perf.winningTrades', key: 'wins', fmt: 'int' },
    { labelKey: 'backtest.perf.losingTrades', key: 'losses', fmt: 'int' },
    { labelKey: 'backtest.perf.winRate', key: 'win_rate', fmt: 'pct' },
    { labelKey: 'backtest.perf.avgTrade', key: 'avg_trade', fmt: 'signed' },
    { labelKey: 'backtest.perf.avgWinningTrade', key: 'avg_win', fmt: 'num' },
    { labelKey: 'backtest.perf.avgLosingTrade', key: 'avg_loss', fmt: 'neg' },
    { labelKey: 'backtest.perf.payoffRatio', key: 'payoff_ratio', fmt: 'ratio' },
    { labelKey: 'backtest.perf.expectancyPerTrade', key: 'expectancy_pct', fmt: 'signedPct' },
    { labelKey: 'backtest.perf.largestWinningTrade', key: 'largest_win', fmt: 'num' },
    { labelKey: 'backtest.perf.largestLosingTrade', key: 'largest_loss', fmt: 'neg' },
    { labelKey: 'backtest.perf.maxConsecWins', key: 'max_consec_wins', fmt: 'int' },
    { labelKey: 'backtest.perf.maxConsecLosses', key: 'max_consec_losses', fmt: 'int' },
    { labelKey: 'backtest.perf.avgBarsInTrade', key: 'avg_bars_held', fmt: 'bars' }
  ];
  const ROWS = $derived(ROW_DEFS.map((r) => ({ ...r, label: $t(r.labelKey) })));

  function cell(scope, row) {
    const v = scope[row.key];
    if (v == null) return { text: '–', tone: 0 };
    switch (row.fmt) {
      case 'signed':
        return { text: fmtNum(v), tone: Math.sign(v) };
      case 'signedPct':
        return { text: `${fmtNum(v)}%`, tone: Math.sign(v) };
      case 'neg':
        return { text: v > 0 ? `−${fmtNum(v)}` : fmtNum(0), tone: v > 0 ? -1 : 0 };
      case 'pct':
        return { text: `${fmtNum(v)}%`, tone: 0 };
      case 'ratio':
        return { text: fmtNum(v), tone: v === 0 ? 0 : Math.sign(v - 1) };
      case 'int':
        return { text: fmtNum(v, 0), tone: 0 };
      case 'bars':
        return { text: fmtNum(v, 1), tone: 0 };
      default:
        return { text: fmtNum(v), tone: 0 };
    }
  }
</script>

<div class="wrap">
  <table>
    <thead>
      <tr>
        <th class="metric">{$t('backtest.perf.metric')}</th>
        <th>{$t('backtest.perf.all')}</th>
        <th><span class="dot long"></span>{$t('backtest.side.long')}</th>
        <th><span class="dot short"></span>{$t('backtest.side.short')}</th>
      </tr>
    </thead>
    <tbody>
      {#each ROWS as row (row.labelKey)}
        <tr>
          <td class="metric">{row.label}</td>
          {#each scopes as s, i (i)}
            {@const c = cell(s, row)}
            <td class:pos={c.tone > 0} class:neg={c.tone < 0}>{c.text}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .wrap {
    overflow-x: auto;
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
    padding: var(--space-2) var(--space-4);
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  th,
  .metric {
    font-family: var(--font);
  }
  th {
    font-size: 0.66rem;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
  }
  .metric {
    text-align: left;
    color: var(--muted);
  }
  td.metric {
    font-size: var(--text-sm);
  }
  tbody tr:nth-child(even) {
    background: color-mix(in srgb, var(--surface) 45%, transparent);
  }
  td {
    color: var(--text);
  }
  td.pos {
    color: var(--green);
  }
  td.neg {
    color: var(--red);
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-right: 6px;
    vertical-align: middle;
  }
  .dot.long {
    background: var(--green);
  }
  .dot.short {
    background: var(--red);
  }
</style>
