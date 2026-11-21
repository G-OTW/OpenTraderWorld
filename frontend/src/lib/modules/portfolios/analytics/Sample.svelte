<script>
  // What a block is standing on: how many observations, over what span, and how much of the
  // book it covers. Shown under every answer, because a number computed over 43 days and one
  // computed over a decade look identical without it.
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtRatioPct } from '$lib/format.js';
  import { fmtDate } from '../api.js';

  let { sample = null } = $props();
  const partial = $derived(sample?.coverage != null && sample.coverage < 0.999);
</script>

{#if sample}
  <p class="sample">
    <span>{$t('portfolios.analytics.sampleRows', { n: sample.rows })}</span>
    {#if sample.from && sample.to}
      <span class="sep" aria-hidden="true">·</span>
      <span>{fmtDate(sample.from)} → {fmtDate(sample.to)}</span>
    {/if}
    {#if sample.coverage != null}
      <span class="sep" aria-hidden="true">·</span>
      <span class:partial>
        {#if partial}<Icon name="alert-triangle" size={12} />{/if}
        {$t('portfolios.analytics.coverage', { pct: fmtRatioPct(sample.coverage, 0) })}
      </span>
    {/if}
  </p>
{/if}

<style>
  .sample {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    font-size: 12px;
    color: var(--dim);
  }
  .sep {
    opacity: 0.6;
  }
  .partial {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--amber-ink);
    font-weight: 600;
  }
</style>
