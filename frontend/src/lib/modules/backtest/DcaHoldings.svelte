<script>
  // What the plan holds at the last bar. A savings plan books a Trade only when it sells, so
  // every trade-derived view is empty while it only accumulates: this is the state that
  // replaces them, marked to the last close.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { assets = [], perAsset = [], ticker = '', asOf = '' } = $props();

  const tone = (v) => (v > 0 ? 'pos' : v < 0 ? 'neg' : '');
  // Position return needs no contract multiplier: it cancels out of price over cost.
  const ret = (a) => (a.avg_cost > 0 ? (a.last_price / a.avg_cost - 1) * 100 : 0);
  const exposure = (tk) => perAsset.find((p) => p.ticker === tk)?.exposure_pct;

  const rows = $derived(ticker ? assets.filter((a) => a.ticker === ticker) : assets);
  const sum = (f) => rows.reduce((n, a) => n + (f(a) || 0), 0);
  const totals = $derived(
    rows.length > 1
      ? {
          value: sum((a) => a.value),
          weight_pct: sum((a) => a.weight_pct),
          target_weight_pct: sum((a) => a.target_weight_pct),
          invested: sum((a) => a.invested),
          realized_pnl: sum((a) => a.realized_pnl),
          unrealized_pnl: sum((a) => a.unrealized_pnl),
          fees: sum((a) => a.fees),
          buys: sum((a) => a.buys),
          sells: sum((a) => a.sells)
        }
      : null
  );
  const withExposure = $derived(perAsset.length > 0);
</script>

{#if rows.length}
  <div class="tbl-wrap">
    <table class="tbl">
      <caption>
        {$t('backtest.dca.holdings')}
        {#if asOf}
          <span class="count">{$t('backtest.dca.atBar', { d: asOf.slice(0, 16).replace('T', ' ') })}</span>
        {/if}
      </caption>
      <thead>
        <tr>
          <th>{$t('backtest.asset.ticker')}</th>
          <th class="r">{$t('backtest.dca.units')}</th>
          <th class="r">{$t('backtest.dca.avgCost')}</th>
          <th class="r">{$t('backtest.dca.last')}</th>
          <th class="r">{$t('backtest.dca.returnCol')}</th>
          <th class="r">{$t('backtest.dca.value')}</th>
          <th class="r">{$t('backtest.dca.weight')}</th>
          <th class="r">{$t('backtest.dca.target')}</th>
          <th class="r">{$t('backtest.dca.investedCol')}</th>
          <th class="r">{$t('backtest.dca.realizedCol')}</th>
          <th class="r">{$t('backtest.dca.unrealizedCol')}</th>
          <th class="r">{$t('backtest.dca.fee')}</th>
          <th class="r">{$t('backtest.dca.fillsCol')}</th>
          {#if withExposure}<th class="r">{$t('backtest.asset.exposure')}</th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as a (a.ticker)}
          <tr>
            <td>{a.ticker}</td>
            <td class="r">{fmtNum(a.units, 4)}</td>
            <td class="r">{fmtNum(a.avg_cost)}</td>
            <td class="r">{fmtNum(a.last_price)}</td>
            <td class="r {tone(ret(a))}">{fmtNum(ret(a), 2)}%</td>
            <td class="r">{fmtNum(a.value)}</td>
            <td class="r">{fmtNum(a.weight_pct, 1)}%</td>
            <td class="r muted">{fmtNum(a.target_weight_pct, 1)}%</td>
            <td class="r">{fmtNum(a.invested)}</td>
            <td class="r {tone(a.realized_pnl)}">{fmtNum(a.realized_pnl)}</td>
            <td class="r {tone(a.unrealized_pnl)}">{fmtNum(a.unrealized_pnl)}</td>
            <td class="r muted">{fmtNum(a.fees)}</td>
            <td class="r muted">{$t('backtest.dca.fillCount', { b: a.buys, s: a.sells })}</td>
            {#if withExposure}
              <td class="r muted"
                >{exposure(a.ticker) == null ? '' : `${fmtNum(exposure(a.ticker), 1)}%`}</td>
            {/if}
          </tr>
        {/each}
        {#if totals}
          <tr class="tot">
            <td>{$t('backtest.dca.total')}</td>
            <td class="r"></td>
            <td class="r"></td>
            <td class="r"></td>
            <td class="r"></td>
            <td class="r">{fmtNum(totals.value)}</td>
            <td class="r">{fmtNum(totals.weight_pct, 1)}%</td>
            <td class="r muted">{fmtNum(totals.target_weight_pct, 1)}%</td>
            <td class="r">{fmtNum(totals.invested)}</td>
            <td class="r {tone(totals.realized_pnl)}">{fmtNum(totals.realized_pnl)}</td>
            <td class="r {tone(totals.unrealized_pnl)}">{fmtNum(totals.unrealized_pnl)}</td>
            <td class="r muted">{fmtNum(totals.fees)}</td>
            <td class="r muted">{$t('backtest.dca.fillCount', { b: totals.buys, s: totals.sells })}</td>
            {#if withExposure}<td class="r"></td>{/if}
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .tbl-wrap {
    overflow-x: auto;
  }
  table.tbl {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  caption {
    text-align: left;
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding-bottom: var(--space-1);
  }
  .count {
    color: var(--muted);
    text-transform: none;
    letter-spacing: 0;
  }
  th,
  td {
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .r {
    text-align: right;
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
  tr.tot td {
    border-bottom: none;
    font-weight: 500;
  }
</style>
