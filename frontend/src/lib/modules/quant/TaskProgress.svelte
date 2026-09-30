<script>
  // Progress of a provider task: requests done out of planned, and what is being fetched.
  import { t } from '$lib/i18n';
  let { progress = null } = $props();
  const pct = $derived(progress?.total ? Math.min(100, (100 * progress.done) / progress.total) : 0);
</script>

<div class="tp">
  <div class="bar"><div class="fill" style="width: {pct}%"></div></div>
  <span class="qp-hint">
    {#if progress?.total}
      {$t('quant.market.progress', { done: progress.done, total: progress.total, step: progress.step })}
    {:else}
      {$t('quant.market.starting')}
    {/if}
  </span>
</div>

<style>
  .tp {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 520px;
  }
  .bar {
    height: 4px;
    border-radius: var(--radius);
    background: var(--surface-2);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
</style>
