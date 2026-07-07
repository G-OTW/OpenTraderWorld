<script>
  // Renders the engine's computed breakdown: per-line tax, totals, effective rate, warnings,
  // and the mandatory disclaimer.
  import { fmtMoney, fmtPct } from '$lib/modules/taxcalc/api.js';
  import { t } from '$lib/i18n';
  let { result = null } = $props();
</script>

{#if result}
  <div class="breakdown">
    <table class="tbl">
      <thead>
        <tr>
          <th class="l">{$t('taxcalc.breakdown.item')}</th>
          <th>{$t('taxcalc.breakdown.taxable')}</th>
          <th>{$t('taxcalc.breakdown.allowance')}</th>
          <th>{$t('taxcalc.breakdown.base')}</th>
          <th>{$t('taxcalc.breakdown.rate')}</th>
          <th>{$t('taxcalc.breakdown.tax')}</th>
        </tr>
      </thead>
      <tbody>
        {#each result.lines as line}
          <tr>
            <td class="l">{line.label}</td>
            <td>{fmtMoney(line.taxable ?? line.gross, result.currency)}</td>
            <td>{line.allowance ? fmtMoney(line.allowance, result.currency) : '—'}</td>
            <td>{fmtMoney(line.base, result.currency)}</td>
            <td>{line.rate_pct == null ? '—' : fmtPct(line.rate_pct)}</td>
            <td class="tax">{fmtMoney(line.tax, result.currency)}</td>
          </tr>
        {/each}
      </tbody>
      <tfoot>
        <tr>
          <td class="l" colspan="3">{$t('taxcalc.breakdown.total', { context: result.context })}</td>
          <td>{fmtMoney(result.total_base, result.currency)}</td>
          <td>{fmtPct(result.effective_rate_pct)}</td>
          <td class="tax">{fmtMoney(result.total_tax, result.currency)}</td>
        </tr>
      </tfoot>
    </table>

    {#if result.warnings?.length}
      <ul class="warn">
        {#each result.warnings as w}
          <li>{w}</li>
        {/each}
      </ul>
    {/if}

    <p class="disclaimer">{result.disclaimer}</p>
  </div>
{/if}

<style>
  .breakdown {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  th,
  td {
    padding: var(--space-2) var(--space-3);
    text-align: right;
    border-bottom: 1px solid var(--border);
  }
  .l {
    text-align: left;
  }
  .tax {
    font-weight: 600;
    color: var(--text);
  }
  tfoot td {
    font-weight: 700;
    background: var(--surface-2);
    border-bottom: none;
  }
  .warn {
    margin: 0;
    padding: var(--space-3) var(--space-4) var(--space-3) var(--space-6);
    color: var(--amber);
    font-size: 0.82rem;
  }
  .disclaimer {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    font-size: 0.75rem;
    color: var(--muted);
    border-top: 1px solid var(--border);
  }
</style>
