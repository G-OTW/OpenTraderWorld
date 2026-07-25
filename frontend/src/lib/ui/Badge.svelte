<script>
  import Icon from './Icon.svelte';

  // Non-interactive status pill. The `tone` prop is semantic, not chromatic:
  // pages say what a thing *is* ("danger"), never what color to paint it. That
  // indirection is what lets a theme swap without touching a page.
  //
  //   <Badge tone="success">Filled</Badge>
  //   <Badge tone="danger" icon="alert-triangle">Failed</Badge>
  //
  // Colors come from the global `.badge` layer (theme/components.css).
  // props: tone ('neutral'|'success'|'warn'|'danger'|'accent'), icon, children
  let { tone = 'neutral', icon = '', children, ...rest } = $props();

  // Semantic tone → the layer's color class. `neutral` is the bare .badge.
  const TONE_CLASS = {
    neutral: '',
    success: 'green',
    warn: 'amber',
    danger: 'red',
    accent: 'accent'
  };
  const toneClass = $derived(TONE_CLASS[tone] ?? '');
</script>

<span class="badge {toneClass}" {...rest}>
  {#if icon}<Icon name={icon} size={11} />{/if}
  {@render children?.()}
</span>
