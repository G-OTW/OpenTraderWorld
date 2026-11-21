<script>
  // A short choice as one framed row of buttons: two to four options, all in view.
  let {
    value = $bindable(),
    /** [{ value, label }] */
    options = [],
    ariaLabel = '',
    disabled = false,
    onpick
  } = $props();

  function pick(v) {
    if (disabled || v === value) return;
    value = v;
    onpick?.(v);
  }
</script>

<div class="seg" role="radiogroup" aria-label={ariaLabel} class:off={disabled}>
  {#each options as o (o.value)}
    <button type="button" role="radio" aria-checked={o.value === value} class:on={o.value === value} {disabled} onclick={() => pick(o.value)}>
      {o.label}
    </button>
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    width: fit-content;
    max-width: 100%;
    height: var(--control-h, 34px);
    border: var(--hairline, 1px) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    overflow: hidden;
  }
  .seg.off {
    opacity: 0.5;
  }
  button {
    display: inline-flex;
    align-items: center;
    height: 100%;
    padding: 0 12px;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  button + button {
    border-left: var(--hairline, 1px) solid var(--border);
  }
  button:hover:not(:disabled) {
    color: var(--text);
  }
  button.on {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-weight: var(--fw-medium);
  }
  button:disabled {
    cursor: default;
  }
</style>
