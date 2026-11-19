<script>
  // One field of an order ticket: a single frame around a borderless input, the small buttons
  // at its ends (`.swap`) and a muted note beside the value (`.aside`). The box owns the hairline
  // and the focus color, so a field is one control, never a border inside a border.
  let { off = false, readonly = false, title = undefined, children } = $props();
</script>

<div class="box" class:off class:ro={readonly} {title}>
  {@render children?.()}
</div>

<style>
  .box {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--control-h, 30px);
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    transition: border-color var(--dur-fast) var(--ease);
  }
  .box:focus-within {
    border-color: var(--accent);
  }
  .box.off {
    opacity: 0.55;
  }
  /* Read from the venue, never typed: the box shows it, nothing to focus. */
  .box.ro {
    background: var(--surface-2);
    cursor: default;
  }
  .box :global(input) {
    flex: 1 1 0;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    font-family: var(--mono);
  }
  .box :global(input.qty) {
    text-align: center;
  }
  .box :global(.swap) {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .box :global(.swap:hover:not(:disabled)) {
    background: var(--surface-2);
    color: var(--text);
  }
  /* Sized to its text, so the input takes the rest of the box and the buttons sit at its ends. */
  .box :global(.aside) {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
    color: var(--muted);
    font-family: var(--mono);
  }
</style>
