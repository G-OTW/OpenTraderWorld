<script>
  // What a savings plan produced. A DCA run has no round trips to rank, so the headline is
  // money: what went in, what it is worth, and the two returns that answer different questions
  // (time-weighted ignores the deposits, money-weighted is what the user actually earned).
  // `compact` renders the tiles alone, for the summary tab.
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';
  import DcaHoldings from './DcaHoldings.svelte';
  import DcaFills from './DcaFills.svelte';

  let { dca, compact = false, perAsset = [], asOf = '' } = $props();

  const pct = (v) => `${fmtNum(v, 2)}%`;
  const tone = (v) => (v > 0 ? 'pos' : v < 0 ? 'neg' : '');

  const tiles = $derived([
    { k: 'moneyIn', v: fmtNum(dca.contributed), hint: $t('backtest.dca.deposited', { n: fmtNum(dca.deposits) }) },
    { k: 'value', v: fmtNum(dca.final_value), hint: $t('backtest.dca.inCash', { n: fmtNum(dca.cash) }) },
    // A withdrawing sell takes money out of the portfolio: without this tile the value tile
    // and the return tile contradict each other with nothing to explain the gap.
    ...(dca.withdrawn > 0
      ? [{ k: 'withdrawn', v: fmtNum(dca.withdrawn), hint: $t('backtest.dca.withdrawnHint') }]
      : []),
    { k: 'totalReturn', v: pct(dca.total_return_pct), tone: tone(dca.total_return_pct) },
    { k: 'twr', v: pct(dca.twr_pct), tone: tone(dca.twr_pct), hint: $t('backtest.dca.twrHint') },
    ...(dca.irr_pct == null
      ? []
      : [{ k: 'irr', v: `${fmtNum(dca.irr_pct, 2)}%`, tone: tone(dca.irr_pct), hint: $t('backtest.dca.perYear') }]),
    { k: 'lumpSum', v: pct(dca.lump_sum_return_pct), hint: fmtNum(dca.lump_sum_value) },
    { k: 'realized', v: fmtNum(dca.realized_pnl), tone: tone(dca.realized_pnl) },
    { k: 'unrealized', v: fmtNum(dca.unrealized_pnl), tone: tone(dca.unrealized_pnl) },
    { k: 'invested', v: fmtNum(dca.invested) },
    { k: 'fees', v: fmtNum(dca.fees) },
    { k: 'fills', v: $t('backtest.dca.fillCount', { b: dca.buys, s: dca.sells }) }
  ]);

</script>

<div class="dca">
  <div class="tiles">
    {#each tiles as x (x.k)}
      <div class="tile">
        <span class="lbl">{$t(`backtest.dca.tile_${x.k}`)}</span>
        <span class="val {x.tone ?? ''}">{x.v}</span>
        {#if x.hint}<span class="hint">{x.hint}</span>{/if}
      </div>
    {/each}
  </div>

  {#if dca.underfunded}
    <p class="warn">{$t('backtest.dca.underfunded', { n: dca.underfunded })}</p>
  {/if}

  {#if !compact}
    <DcaHoldings assets={dca.assets ?? []} {perAsset} {asOf} />
    <DcaFills events={dca.events ?? []} total={dca.events_total} />
  {/if}
</div>

<style>
  .dca {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-2);
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    border: var(--hairline) solid var(--border);
    background: var(--surface);
    padding: var(--space-2) var(--space-3);
    min-width: 0;
  }
  .tile .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .tile .val {
    font-size: var(--text-md);
    font-variant-numeric: tabular-nums;
  }
  .tile .hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .warn {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
