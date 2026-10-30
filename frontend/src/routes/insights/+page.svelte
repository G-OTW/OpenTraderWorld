<script>
  // Shared route, not a registry module: the page reads the journal or the backtest module,
  // whichever the address names, and says so when that module is not installed.
  import InsightsPage from '$lib/insights/InsightsPage.svelte';
</script>

<InsightsPage />
