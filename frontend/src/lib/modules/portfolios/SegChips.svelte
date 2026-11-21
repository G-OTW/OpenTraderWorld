<script>
  // A compact segmented row (periods, granularity, grouping): one hairline frame, the
  // active option lifted by a tint and its text, never a filled accent block. One
  // definition for every such row in the module so they share a height and a rhythm.
  //
  //   <SegChips options={[{ key: '1m', label: '1M' }]} bind:value={period} mono />
  //
  // props: options [{ key, label, title? }], value ($bindable key), mono (figures font),
  //        ariaLabel, onpick(key)
  let { options = [], value = $bindable(null), mono = false, ariaLabel = '', onpick = null } = $props();
</script>

<div class="segchips" class:mono role="group" aria-label={ariaLabel || undefined}>
  {#each options as o (o.key)}
    <button
      type="button"
      class:active={value === o.key}
      aria-pressed={value === o.key}
      title={o.title || undefined}
      onclick={() => {
        value = o.key;
        onpick?.(o.key);
      }}>{o.label}</button
    >
  {/each}
</div>

<style>
  .segchips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 2px;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    max-width: 100%;
  }
  button {
    height: 22px;
    padding: 0 var(--space-2);
    border: 0;
    border-radius: calc(var(--radius-sm) - 2px);
    background: transparent;
    color: var(--muted);
    font-size: 11px;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background-color 160ms ease,
      color 160ms ease;
  }
  .mono button {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  button:hover {
    color: var(--text);
  }
  button.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 600;
  }
  button:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }
  @media (max-width: 767px) {
    button {
      min-height: 36px;
    }
  }
</style>
