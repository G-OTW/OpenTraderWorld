<script>
  // Kept only as an import path for Scanner's grid (scanner/GridView.svelte imports it):
  // it renders the shared sparkline, so both modules draw the same mark. Watchlists itself
  // imports $lib/ui/charts/Sparkline.svelte directly. Drop this file once Scanner does too.
  import SharedSparkline from '$lib/ui/charts/Sparkline.svelte';

  let { values = [], width = 120, height = 30 } = $props();
</script>

<SharedSparkline {values} {width} {height} />
