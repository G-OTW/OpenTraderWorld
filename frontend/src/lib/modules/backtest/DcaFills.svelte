<script>
  // The fills of a savings plan: its list of orders, which is what a DCA run has instead of
  // round trips. The engine caps the list it sends back, so the caption says how many there
  // really were.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { events = [], total = null, ticker = '', tall = false } = $props();

  const tone = (v) => (v > 0 ? 'pos' : v < 0 ? 'neg' : '');
  const rows = $derived(ticker ? events.filter((e) => e.ticker === ticker) : events);
  // A ticker filter makes the engine's total meaningless: it counted every asset.
  const capped = $derived(!ticker && total != null && total > events.length ? total : null);
</script>

{#if rows.length}
  <div class="tbl-wrap" class:tall>
    <table class="tbl">
      <caption>
        {$t('backtest.dca.fills')} <span class="count">{rows.length}</span>
        {#if capped}<span class="count">{$t('backtest.dca.fillsCapped', { n: capped })}</span>{/if}
      </caption>
      <thead>
        <tr>
          <th>{$t('backtest.dca.date')}</th>
          <th>{$t('backtest.asset.ticker')}</th>
          <th>{$t('backtest.dca.action')}</th>
          <th>{$t('backtest.dca.source')}</th>
          <th class="r">{$t('backtest.dca.price')}</th>
          <th class="r">{$t('backtest.dca.qty')}</th>
          <th class="r">{$t('backtest.dca.amount')}</th>
          <th class="r">{$t('backtest.dca.fee')}</th>
          <th class="r">{$t('backtest.dca.unitsAfter')}</th>
          <th class="r">{$t('backtest.dca.avgAfter')}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as e, i (i)}
          <tr>
            <td class="ts">{e.ts.slice(0, 16).replace('T', ' ')}</td>
            <td>{e.ticker}</td>
            <td class={e.action === 'sell' ? 'neg' : 'pos'}>{$t(`backtest.dca.action_${e.action}`)}</td>
            <td class="muted">{e.source}</td>
            <td class="r">{fmtNum(e.price)}</td>
            <td class="r">{fmtNum(e.qty, 4)}</td>
            <td class="r {tone(e.amount)}">{fmtNum(e.amount)}</td>
            <td class="r">{fmtNum(e.fee)}</td>
            <td class="r">{fmtNum(e.units_after, 4)}</td>
            <td class="r">{fmtNum(e.avg_cost_after)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .tbl-wrap {
    overflow: auto;
    max-height: 340px;
  }
  .tall {
    max-height: 70vh;
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
    position: sticky;
    top: 0;
    background: var(--surface);
  }
  .r {
    text-align: right;
  }
  .muted {
    color: var(--muted);
  }
  .ts {
    color: var(--muted);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
