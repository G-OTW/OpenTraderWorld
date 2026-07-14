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
  .hero {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: var(--space-2);
  }
  .kpi {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    min-width: 0;
  }
  .kpi.pos {
    background: color-mix(in srgb, var(--green) 8%, var(--surface-2));
    border-color: color-mix(in srgb, var(--green) 30%, var(--border));
  }
  .kpi.neg {
    background: color-mix(in srgb, var(--red) 8%, var(--surface-2));
    border-color: color-mix(in srgb, var(--red) 30%, var(--border));
  }
  .label {
    font-size: 0.66rem;
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .value {
    font-size: 1.35rem;
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }
  .kpi.pos .value {
    color: var(--green);
  }
  .kpi.neg .value {
    color: var(--red);
  }
  .sub {
    font-size: 0.7rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
