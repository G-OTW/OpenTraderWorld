<script>
  // Single-asset metric cards: HV, Max DD, VaR (hist + parametric), CVaR. Each card states
  // the metric and the plain-language question it answers, to stay friendly for non-quants.
  import { fmtRatioPct } from '$lib/modules/quant/api.js';
  import { t } from '$lib/i18n';

  let { result } = $props();

  let cards = $derived([
    {
      label: $t('quant.metrics.historicalVolatility'),
      value: fmtRatioPct(result.hv_annual, 1),
      tone: 'amber',
      hint: $t('quant.metrics.historicalVolatilityHint')
    },
    {
      label: $t('quant.metrics.maxDrawdown'),
      value: `−${fmtRatioPct(result.max_drawdown, 1)}`,
      tone: 'red',
      hint: $t('quant.metrics.maxDrawdownHint')
    },
    {
      label: $t('quant.metrics.valueAtRisk', { confidence: fmtRatioPct(result.confidence, 0) }),
      value: `−${fmtRatioPct(result.var_hist, 2)}`,
      tone: 'red',
      hint: $t('quant.metrics.valueAtRiskHint', { param: fmtRatioPct(result.var_param, 2) })
    },
    {
      label: $t('quant.metrics.conditionalVar', { confidence: fmtRatioPct(result.confidence, 0) }),
      value: `−${fmtRatioPct(result.cvar, 2)}`,
      tone: 'red',
      hint: $t('quant.metrics.conditionalVarHint')
    }
  ]);
</script>

<div class="grid">
  {#each cards as c (c.label)}
    <div class="card">
      <span class="lbl">{c.label}</span>
      <span class="val" class:red={c.tone === 'red'} class:amber={c.tone === 'amber'}>{c.value}</span>
      <span class="hint">{c.hint}</span>
    </div>
  {/each}
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lbl {
    font-size: var(--text-sm);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .val {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .val.red {
    color: var(--red);
  }
  .val.amber {
    color: var(--amber);
  }
  .hint {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: 1.3;
  }
</style>
