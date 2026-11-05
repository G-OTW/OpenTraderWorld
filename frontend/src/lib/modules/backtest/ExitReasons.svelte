<script>
  import { fmtNum, EXIT_REASON_KEYS } from './api.js';
  import { t } from '$lib/i18n';
  // How the run's trades ended, biggest bucket first: the report's compact version of the
  // trade list's filter chips (which is why it shares their labels).
  let { reasons = {} } = $props();

  const rows = $derived.by(() => {
    const items = Object.entries(reasons ?? {});
    const total = items.reduce((n, [, v]) => n + v, 0);
    return items
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .map(([k, v]) => ({
        key: k,
        label: EXIT_REASON_KEYS[k] ? $t(EXIT_REASON_KEYS[k]) : k,
        count: v,
        share: total ? (v * 100) / total : 0
      }));
  });
</script>

{#if rows.length}
  <div class="wrap">
    <div class="cap">{$t('backtest.trades.exitReasons')}</div>
    <table>
      <thead>
        <tr>
          <th>{$t('backtest.trades.reasonCol')}</th>
          <th class="r">{$t('backtest.report.count')}</th>
          <th class="r">{$t('backtest.report.share')}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.key)}
          <tr>
            <td>{r.label}</td>
            <td class="r">{fmtNum(r.count, 0)}</td>
            <td class="r muted">{fmtNum(r.share, 1)}%</td>
          </tr>
        {/each}
      </tbody>
    </table>
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
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--surface-2) 40%, transparent);
    border-bottom: 1px solid var(--border);
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
    font-weight: var(--fw-medium);
    border-bottom: 1px solid var(--border);
  }
  tbody tr:not(:last-child) td {
    border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }
  .r {
    text-align: right;
  }
  .muted {
    color: var(--muted);
  }
</style>
