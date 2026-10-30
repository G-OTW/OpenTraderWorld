<script>
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';
  // In-sample vs out-of-sample side-by-side stat blocks — the cheap overfitting check. A big
  // gap between the two columns (return, win rate, profit factor) is the warning sign.
  let { oos = null } = $props();

  const fmtTs = (ts) => (ts ? String(ts).slice(0, 10) : '—');
  const ROWS = $derived([
    { key: 'return_pct', label: $t('backtest.oos.return'), suffix: '%' },
    { key: 'win_rate', label: $t('backtest.oos.winRate'), suffix: '%' },
    { key: 'profit_factor', label: $t('backtest.oos.profitFactor'), suffix: '' },
    { key: 'trades', label: $t('backtest.oos.trades'), suffix: '', d: 0 },
    { key: 'max_drawdown_pct', label: $t('backtest.oos.maxDd'), suffix: '%' }
  ]);
</script>

{#if oos}
  <div class="oos">
    <div class="cap">
      {$t('backtest.oos.title')}
      <span class="split">{$t('backtest.oos.splitAt', { pct: Math.round(oos.split_pct * 100), ts: fmtTs(oos.split_ts) })}</span>
    </div>
    <table>
      <thead>
        <tr>
          <th></th>
          <th class="r">{$t('backtest.oos.inSample')}</th>
          <th class="r">{$t('backtest.oos.outSample')}</th>
        </tr>
      </thead>
      <tbody>
        {#each ROWS as row (row.key)}
          <tr>
            <td class="lbl">{row.label}</td>
            <td class="r">{fmtNum(oos.in_sample[row.key], row.d ?? 2)}{row.suffix}</td>
            <td class="r">{fmtNum(oos.out_sample[row.key], row.d ?? 2)}{row.suffix}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    <p class="note">{$t('backtest.oos.note')}</p>
  </div>
{/if}

<style>
  .oos {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .cap {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: 0.7rem;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--surface-2) 40%, transparent);
    border-bottom: 1px solid var(--border);
  }
  .split {
    text-transform: none;
    letter-spacing: 0;
    font-weight: var(--fw-medium);
    font-size: var(--text-xs);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    padding: var(--space-1) var(--space-3);
    text-align: left;
  }
  th {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
  }
  .r {
    text-align: right;
  }
  .lbl {
    color: var(--muted);
  }
  tbody tr:not(:last-child) td {
    border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }
  .note {
    font-size: var(--text-xs);
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    font-style: italic;
  }
</style>
