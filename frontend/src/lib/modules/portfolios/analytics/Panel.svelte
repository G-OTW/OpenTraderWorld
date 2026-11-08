<script>
  // One analysis panel: the module's card around the shared ChartFrame, so every chart,
  // table and empty state in the analysis tab sits on the same frame (13px title, one-line
  // subtitle, legend when 2+, actions on the right). Unavailable and loading states hold
  // `height`, so a panel never jumps when its answer arrives or goes away.
  //
  //   <Panel title="Underwater" subtitle="Drawdown from the running peak" height={200}>
  //     <TimeSeries ... height={200} />
  //   </Panel>
  //
  // props: title, subtitle, legend, height, loading, unavailable, flush (no inner
  //        padding, for a table that runs edge to edge), actions (snippet), children
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';

  let {
    title = '',
    subtitle = '',
    legend = [],
    height = null,
    loading = false,
    unavailable = null,
    flush = false,
    actions,
    children
  } = $props();
</script>

<section class="pcard" class:flush>
  <ChartFrame {title} {subtitle} {legend} {height} {loading} {unavailable} {actions}>
    {@render children?.()}
  </ChartFrame>
</section>

<style>
  .pcard {
    min-width: 0;
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
  }
  /* A table panel keeps the title padded and lets the rows run to the frame's edge. */
  .pcard.flush {
    padding: var(--space-4) 0 var(--space-1);
  }
  .pcard.flush :global(.cf-head) {
    padding: 0 var(--space-4);
  }
</style>
