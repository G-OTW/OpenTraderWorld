<script>
  // Before and after, metric by metric: the shared verdict table of the what-if lab.
  // The delta is coloured by which way is good for that metric (fees and drawdown down).
  import { fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import { summaryRows } from './lab.js';
  import { t } from '$lib/i18n';

  let { before = null, after = null, currency = 'USD', afterLabel = '' } = $props();

  const rows = $derived(summaryRows(before, after));

  function fmt(kind, v, signed = false) {
    if (v == null) return '-';
    if (kind === 'money') return signed ? fmtSignedMoney(v, currency) : fmtMoney(v, currency);
    if (kind === 'pct') return signed ? `${v > 0 ? '+' : ''}${fmtPct(v, 1)}` : fmtPct(v, 1);
    if (kind === 'int') return signed ? `${v > 0 ? '+' : ''}${Math.round(v)}` : String(Math.round(v));
    return signed ? `${v > 0 ? '+' : ''}${fmtNum(v, 2)}` : fmtNum(v, 2);
  }
</script>

<div class="table-wrap">
  <table class="tbl">
    <thead>
      <tr>
        <th></th>
        <th class="num">{$t('journal.lab.col.before')}</th>
        <th class="num">{afterLabel || $t('journal.lab.col.after')}</th>
        <th class="num">{$t('journal.lab.col.delta')}</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as r (r.key)}
        <tr>
          <td>{$t(`journal.lab.metric.${r.key}`)}</td>
          <td class="num">{fmt(r.kind, r.before)}</td>
          <td class="num">{fmt(r.kind, r.after)}</td>
          <td class="num" class:pos={r.tone === 'pos'} class:neg={r.tone === 'neg'}>{fmt(r.kind, r.delta, true)}</td>
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
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
