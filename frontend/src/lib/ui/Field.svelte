<script>
  // Shared label/hint/error scaffolding for form controls. Not used directly by
  // pages — Input and Select wrap it so both carry identical spacing, identical
  // error semantics, and identical a11y wiring (aria-describedby, aria-invalid).
  //
  // Error state colors the border and the hint. Never a red fill: a filled input
  // is unreadable and reads as an alarm, not a correction.
  // props: id, label, hint, error, required, children
  let { id, label = '', hint = '', error = '', required = false, children } = $props();

  const message = $derived(error || hint);
</script>

<div class="field" class:has-error={!!error}>
  {#if label}
    <label for={id}>
      {label}
      {#if required}<span class="req" aria-hidden="true">*</span>{/if}
    </label>
  {/if}

  {@render children?.()}

  {#if message}
    <p class="msg" id="{id}-msg" role={error ? 'alert' : undefined}>{message}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  label {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
  }
  .req {
    color: var(--red);
    margin-left: 2px;
  }

  .msg {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: var(--lh-tight);
  }
  .has-error .msg {
    color: var(--red);
  }

  /* The control is a child slot, so reach it from the error state. */
  .has-error :global(input),
  .has-error :global(select),
  .has-error :global(textarea) {
    border-color: var(--red);
  }
  .has-error :global(input:focus-visible),
  .has-error :global(select:focus-visible),
  .has-error :global(textarea:focus-visible) {
    outline-color: var(--red);
  }
</style>
