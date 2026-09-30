<script>
  // Why a block has no answer, in the user's words.
  //
  // Never a zero-filled table: a reader cannot tell a measured zero from a missing one, and
  // both occur here. Each reason carries what to do about it, because "no history" and "no
  // benchmark" are fixed in completely different places.
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';

  let { missing = [], sample = null, onfix = null } = $props();

  // Reasons that have a button behind them. The rest are stated and left alone.
  const FIXABLE = { no_history: 'history', no_bars: 'history', no_benchmark: 'benchmark', no_targets: 'targets' };
  const first = $derived(missing[0] ?? 'unknown');
  const action = $derived(FIXABLE[first] ?? null);
</script>

<div class="unavail">
  <Icon name="info" size={16} />
  <div class="body">
    <p class="why">{$t(`portfolios.analytics.missing.${first}`)}</p>
    <p class="how">{$t(`portfolios.analytics.missingHow.${first}`)}</p>
    {#if sample?.rows}
      <p class="sample">{$t('portfolios.analytics.sampleRows', { n: sample.rows })}</p>
    {/if}
  </div>
  {#if action && onfix}
    <button class="btn sm" onclick={() => onfix(action)}>
      {$t(`portfolios.analytics.fix.${action}`)}
    </button>
  {/if}
</div>

<style>
  .unavail {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-4);
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .why {
    margin: 0;
    color: var(--text);
    font-weight: 600;
  }
  .how,
  .sample {
    margin: var(--space-1) 0 0;
    font-size: 12px;
  }
</style>
