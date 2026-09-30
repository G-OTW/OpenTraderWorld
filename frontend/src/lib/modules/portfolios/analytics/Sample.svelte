<script>
  // What a block is standing on: how many observations, over what span, and how much of the
  // book it covers. Shown under every answer, because a number computed over 43 days and one
  // computed over a decade look identical without it.
  import { t } from '$lib/i18n';
  import { fmtDate, fmtRatioPct } from '$lib/format.js';

  let { sample = null } = $props();
  const partial = $derived(sample?.coverage != null && sample.coverage < 0.999);
</script>

{#if sample}
  <p class="sample">
    <span>{$t('portfolios.analytics.sampleRows', { n: sample.rows })}</span>
    {#if sample.from && sample.to}
      <span class="sep">·</span>
      <span>{fmtDate(sample.from)} → {fmtDate(sample.to)}</span>
    {/if}
    {#if sample.coverage != null}
      <span class="sep">·</span>
      <span class:partial>
        {$t('portfolios.analytics.coverage', { pct: fmtRatioPct(sample.coverage, 0) })}
      </span>
    {/if}
  </p>
{/if}

<style>
  .sample {
    margin: var(--space-2) 0 0;
    font-size: 11px;
    color: var(--muted);
  }
  .sep {
    margin: 0 var(--space-1);
    opacity: 0.5;
  }
  .partial {
    color: var(--amber);
    font-weight: 600;
  }
</style>
