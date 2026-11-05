<script>
  // Per-trade before and after, shared by every tab of the what-if lab. A trade the
  // scenario does not take shows no "after" and counts its whole result as the delta.
  // rows: [{ id, ticker, side, at, before, after (null = not taken), outcome, outcomeLabel,
  //          muted, fee?: { paid, alternative, target_qty, remainder_qty } }]
  import { fmtSignedMoney, fmtMoney, fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let {
    rows = [],
    currency = 'USD',
    focus = null,
    onpick = null,
    beforeLabel = '',
    afterLabel = '',
    dateLabel = '',
    fees = false
  } = $props();

  const fmtDate = (iso) => (iso ? new Date(iso).toLocaleDateString() : '-');
  const delta = (r) => (r.after == null ? -r.before : r.after - r.before);
</script>

<div class="table-wrap">
  <table class="tbl">
    <thead>
      <tr>
        <th>{$t('journal.trades.col.ticker')}</th>
        <th>{$t('journal.trades.col.side')}</th>
        <th>{dateLabel || $t('journal.lab.col.date')}</th>
        {#if fees}
          <th class="num">{$t('journal.lab.fees.col.paid')}</th>
          <th class="num">{$t('journal.lab.fees.col.alternative')}</th>
          <th class="num" title={$t('journal.lab.fees.col.qtyHint')}>{$t('journal.lab.fees.col.qty')}</th>
        {/if}
        <th class="num">{beforeLabel || $t('journal.lab.col.before')}</th>
        <th class="num">{afterLabel || $t('journal.lab.col.after')}</th>
        <th class="num">{$t('journal.lab.col.delta')}</th>
        <th>{$t('journal.lab.col.outcome')}</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as r (r.id)}
        {@const d = delta(r)}
        <tr class:focus={focus === r.id} class:pickable={!!onpick} onclick={() => onpick?.(r.id)}>
          <td class="strong">{r.ticker}</td>
          <td><span class="side {r.side}">{r.side}</span></td>
          <td class="mono">{fmtDate(r.at)}</td>
          {#if fees}
            <td class="num">{r.fee ? fmtMoney(r.fee.paid, currency) : '-'}</td>
            <td class="num">{r.fee ? fmtMoney(r.fee.alternative, currency) : '-'}</td>
            <td class="num">
              {#if r.fee}
                {fmtNum(r.fee.target_qty, 0)}{#if r.fee.remainder_qty > 0}<span class="rem"> + {fmtNum(r.fee.remainder_qty, 0)}</span>{/if}
              {:else}-{/if}
            </td>
          {/if}
          <td class="num">{fmtSignedMoney(r.before, currency)}</td>
          <td class="num" class:muted={r.after == null}>{r.after == null ? '-' : fmtSignedMoney(r.after, currency)}</td>
          <td class="num" class:pos={d > 1e-9} class:neg={d < -1e-9}>{fmtSignedMoney(d, currency)}</td>
          <td><span class="outcome" class:muted={r.muted}>{r.outcomeLabel ?? ''}</span></td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .table-wrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
  }
  tr.pickable {
    cursor: pointer;
  }
  tr.pickable:hover td,
  tr.focus td {
    background: var(--surface-2);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .side,
  .outcome {
    text-transform: uppercase;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
  }
  .side.short {
    color: var(--red);
  }
  .side.long {
    color: var(--green);
  }
  .muted,
  .rem {
    color: var(--muted);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
