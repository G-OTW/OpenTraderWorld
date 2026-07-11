<script>
  import Icon from './Icon.svelte';

  // The one button. Wraps the global `.btn` layer (theme/components.css) so the
  // look stays shared with bare <button class="btn"> markup that hasn't migrated
  // yet; this component adds the states that layer can't express: `loading`
  // (spinner + inert) and a real `:active` press.
  //
  //   <Button variant="primary" onclick={save}>Save</Button>
  //   <Button variant="danger" icon="trash" size="sm" loading={busy}>Delete</Button>
  //
  // Only `primary` wears --accent. One primary per screen — that is what makes
  // it read as the action.
  // props: variant ('primary'|'secondary'|'ghost'|'danger'), size ('sm'|'md'),
  //        icon (Icon name), loading, disabled, type, onclick, children
  let {
    variant = 'secondary',
    size = 'md',
    icon = '',
    loading = false,
    disabled = false,
    type = 'button',
    onclick = undefined,
    children,
    ...rest
  } = $props();

  // `secondary` is the neutral `.btn` base; the others stack a variant class.
  const variantClass = $derived(variant === 'secondary' ? '' : variant);
  const inert = $derived(disabled || loading);
</script>

<button
  {type}
  class="btn {variantClass} {size === 'sm' ? 'sm' : ''}"
  class:loading
  disabled={inert}
  aria-busy={loading}
  {onclick}
  {...rest}
>
  {#if loading}
    <span class="spinner" aria-hidden="true"></span>
  {:else if icon}
    <Icon name={icon} size={size === 'sm' ? 13 : 15} />
  {/if}
  {@render children?.()}
</button>

<style>
  /* Press feedback the global layer lacks: the button gives, then returns. */
  .btn:active:not(:disabled) {
    transform: translateY(1px);
  }

  /* Loading keeps the label (no width jump) and swaps the icon for a spinner. */
  .btn.loading {
    cursor: progress;
    opacity: 1; /* override the :disabled dim — it's busy, not unavailable */
  }

  .spinner {
    width: 13px;
    height: 13px;
    flex: none;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    opacity: 0.7;
    animation: spin 600ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
