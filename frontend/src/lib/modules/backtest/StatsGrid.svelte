<script>
  // Headline KPI row for a run: six tiles with a secondary line each. Signed values are
  // tinted green/red; the detailed breakdown lives in PerfTable / TradesTable below.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { stats = {} } = $props();

  const vsBH = $derived((stats.return_pct ?? 0) - (stats.buy_hold_return_pct ?? 0));

  const hero = $derived([
    {
      label: $t('backtest.stats.return'),
      value: `${fmtNum(stats.return_pct)}%`,
      signed: stats.return_pct,
      sub: $t('backtest.stats.returnSub', {
        buyHold: `${fmtNum(stats.buy_hold_return_pct)}%`,
        vsBH: `${vsBH >= 0 ? '+' : ''}${fmtNum(vsBH)}%`
      })
    },
    {
      label: $t('backtest.stats.netPnl'),
      value: fmtNum(stats.net_pnl),
      signed: stats.net_pnl,
      sub: $t('backtest.stats.netPnlSub', {
        fees: fmtNum(stats.total_fees),
        final: fmtNum(stats.final_equity)
      })
    },
    {
      label: $t('backtest.stats.winRate'),
      value: `${fmtNum(stats.win_rate)}%`,
      sub: $t('backtest.stats.winRateSub', {
        wins: stats.wins ?? 0,
        losses: stats.losses ?? 0,
        trades: fmtNum(stats.trades, 0)
      })
    },
    {
      label: $t('backtest.stats.profitFactor'),
      value: fmtNum(stats.profit_factor),
      signed: stats.profit_factor != null ? stats.profit_factor - 1 : null,
      sub: $t('backtest.stats.profitFactorSub', { expectancy: `${fmtNum(stats.expectancy_pct)}%` })
    },
    {
      label: $t('backtest.stats.maxDrawdown'),
      value: `−${fmtNum(stats.max_drawdown_pct)}%`,
      signed: -(stats.max_drawdown_pct || 0),
      sub: stats.max_drawdown != null ? `−${fmtNum(stats.max_drawdown)}` : ''
    },
    {
      label: $t('backtest.stats.sharpe'),
      value: fmtNum(stats.sharpe),
      signed: stats.sharpe,
      sub: $t('backtest.stats.sharpeSub', { sortino: fmtNum(stats.sortino) })
    }
  ]);
</script>

<div class="hero">
  {#each hero as c (c.label)}
    <div class="kpi" class:pos={c.signed > 0} class:neg={c.signed < 0}>
      <span class="label">{c.label}</span>
      <span class="value">{c.value}</span>
      {#if c.sub}<span class="sub">{c.sub}</span>{/if}
    </div>
  {/each}
</div>

<style>
  /* Continuous filet grid: cells on --bg separated by 0.5px --border hairlines. */
  .hero {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .kpi {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--bg);
    padding: 14px 16px;
    min-width: 0;
  }
  .label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .value {
    font-family: var(--mono);
    font-size: 15px;
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .kpi.pos .value {
    color: var(--green);
  }
  .kpi.neg .value {
    color: var(--red);
  }
  .sub {
    font-size: 11.5px;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
