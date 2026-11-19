<script>
  // The headline figure of a widget: a big mono number, an optional signed delta beside
  // it, a note under it, and an optional progress track. Every "one number" card in the
  // mockups (Average Progress, Net PnL, Win Rate, Profit Factor…) is this component.
  import Icon from '$lib/ui/Icon.svelte';

  let {
    value,                 // already-formatted string
    tone = '',             // '' | 'pos' | 'neg' | 'warn' — colours the value
    delta = '',            // already-formatted signed string
    deltaTone = '',        // '' | 'pos' | 'neg'
    note = '',             // one line under the figure
    percent = null,        // 0..100 draws a progress track
    barTone = '',          // '' | 'pos' | 'warn'
    size = 'lg',           // 'lg' | 'sm'
    children
  } = $props();
</script>

<div class="stat">
  <div class="line">
    <span class="w-metric-value" class:sm={size === 'sm'} class:w-pos={tone === 'pos'}
      class:w-neg={tone === 'neg'} class:w-warn={tone === 'warn'}>{value}</span>
    {#if delta}
      <span class="delta" class:w-pos={deltaTone === 'pos'} class:w-neg={deltaTone === 'neg'}>
        {#if deltaTone}<Icon name={deltaTone === 'neg' ? 'arrow-down' : 'arrow-up'} size={13} />{/if}
        {delta}
      </span>
    {/if}
  </div>
  {#if note}<p class="w-metric-note">{note}</p>{/if}
  {#if percent !== null}
    <div class="w-bar" class:pos={barTone === 'pos'} class:warn={barTone === 'warn'}
      role="progressbar" aria-valuenow={Math.round(percent)} aria-valuemin="0" aria-valuemax="100">
      <span style:width={`${Math.max(0, Math.min(100, percent))}%`}></span>
    </div>
  {/if}
  {@render children?.()}
</div>

<style>
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
    min-width: 0;
  }
  .delta {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .delta :global(svg) {
    flex-shrink: 0;
  }
  .w-metric-note {
    margin: 0;
  }
</style>
