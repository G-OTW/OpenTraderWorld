<script>
  // Segmented chips: one choice among a few, as one control. Borderless on purpose: the
  // tinted track is the frame, the picked segment lifts to the raised surface.
  // props: options [{ value, label }], value ($bindable), ariaLabel, size ('sm' | 'md')
  let { options = [], value = $bindable(''), ariaLabel = '', size = 'sm' } = $props();
</script>

<div class="seg {size}" role="group" aria-label={ariaLabel || undefined}>
  {#each options as o (o.value)}
    <button type="button" class:on={value === o.value} aria-pressed={value === o.value} onclick={() => (value = o.value)}>
      {o.label}
    </button>
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    flex: none;
    align-self: flex-start;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    max-width: 100%;
    overflow-x: auto;
  }
  button {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 var(--space-3);
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background-color 160ms ease,
      color 160ms ease;
  }
  .md button {
    height: 28px;
  }
  button:hover {
    color: var(--text);
  }
  button.on {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
    color: var(--text);
    font-weight: 500;
  }
  button:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  @media (max-width: 767px) {
    button {
      min-height: 44px;
    }
  }
</style>
