<script>
  // Horizontal bars for a weight vector (e.g. risk-parity weights, or a frontier portfolio's
  // weights). Optionally overlays each asset's risk contribution so equal-risk is visible.
  import { fmtPct } from '$lib/modules/quant/api.js';
  import { t } from '$lib/i18n';

  let { labels = [], weights = [], risk = null, title = undefined } = $props();
  const resolvedTitle = $derived(title ?? $t('quant.weights.defaultTitle'));

  let rows = $derived(
    labels.map((l, i) => ({ label: l, w: weights[i] ?? 0, rc: risk ? (risk[i] ?? 0) : null }))
  );
  let maxW = $derived(Math.max(0.0001, ...rows.map((r) => r.w)));
</script>

<div class="wrap">
  <h4>{resolvedTitle}</h4>
  {#each rows as r (r.label)}
    <div class="row">
      <span class="lbl">{r.label}</span>
      <div class="track">
        <div class="fill" style="width:{(r.w / maxW) * 100}%"></div>
        {#if r.rc != null}
          <div class="risk" style="width:{(r.rc / maxW) * 100}%" title={$t('quant.weights.riskContribution')}></div>
        {/if}
      </div>
      <span class="val">{fmtPct(r.w, 1)}{#if r.rc != null}<span class="rc"> · {$t('quant.weights.riskInline', { pct: fmtPct(r.rc, 0) })}</span>{/if}</span>
    </div>
  {/each}
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  h4 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .row {
    display: grid;
    grid-template-columns: 90px 1fr 140px;
    align-items: center;
    gap: var(--space-2);
  }
  .lbl {
    font-size: 0.85rem;
    font-weight: 600;
  }
  .track {
    position: relative;
    height: 16px;
    background: var(--surface-2);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--accent);
    opacity: 0.7;
  }
  .risk {
    position: absolute;
    inset: 0 auto 0 0;
    border-right: 2px solid var(--amber);
  }
  .val {
    font-size: 0.8rem;
    color: var(--muted);
    text-align: right;
  }
  .rc {
    color: var(--amber);
  }
</style>
