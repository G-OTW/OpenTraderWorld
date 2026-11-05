<script>
  import Icon from './Icon.svelte';

  // A single figure, presented so it reads as the answer to a question. The jump
  // from the 12px label to the 28px value is what makes the page look like a
  // product rather than a form.
  //
  //   <StatCard label="Net PnL" value="1 240,50 €" delta={3.2} />
  //   <StatCard label="Open positions" value={7} />
  //
  // `delta` is a signed number: positive → green, negative → red, zero → muted.
  // The sign is ALWAYS carried by a glyph (+ / −) and an arrow, never by color
  // alone — roughly 1 in 12 men cannot separate the two hues.
  // props: label, value, delta, deltaSuffix, hint
  let { label = '', value = '', delta = null, deltaSuffix = '%', hint = '' } = $props();

  const dir = $derived(delta == null || delta === 0 ? 'flat' : delta > 0 ? 'up' : 'down');
  // Minus sign U+2212, not a hyphen: it aligns with digits in a tabular font.
  const sign = $derived(dir === 'up' ? '+' : dir === 'down' ? '−' : '');
  const magnitude = $derived(delta == null ? '' : Math.abs(delta).toString());
</script>

<!-- `statcard`, not `stat`: the global .stat layer (theme/components.css) still
     dresses the pages that haven't migrated, and two rules on one class is how a
     restyle turns into a debugging session. -->
<div class="statcard">
  <span class="label">{label}</span>
  <span class="value num">{value}</span>

  {#if delta != null}
    <span class="delta {dir}">
      {#if dir !== 'flat'}
        <Icon name={dir === 'up' ? 'arrow-up' : 'arrow-down'} size={12} />
      {/if}
      <span class="num">{sign}{magnitude}{deltaSuffix}</span>
    </span>
  {:else if hint}
    <span class="hint">{hint}</span>
  {/if}
</div>

<style>
  .statcard {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    border-radius: 0;
    padding: var(--pad-metric);
    min-width: 0;
  }

  .label {
    color: var(--dim);
    font-size: 10px;
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .value {
    color: var(--text);
    font-size: 15px;
    font-weight: var(--fw-normal);
    font-family: var(--mono);
    line-height: var(--lh-tight);
    overflow-wrap: anywhere;
  }

  .delta {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    font-family: var(--mono);
    line-height: var(--lh-tight);
  }
  .delta.up {
    color: var(--green);
  }
  .delta.down {
    color: var(--red);
  }
  .delta.flat {
    color: var(--dim);
  }

  .hint {
    color: var(--dim);
    font-size: var(--text-xs);
    line-height: var(--lh-tight);
  }
</style>
