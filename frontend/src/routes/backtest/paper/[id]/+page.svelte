<script>
  import { page } from '$app/stores';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import PaperDashboard from '$lib/modules/backtest/PaperDashboard.svelte';
  import PaperModal from '$lib/modules/backtest/PaperModal.svelte';

  let dashboard;
  let editing = $state(false);
  let draft = $state(null);
  let tickers = $state([]);

  function edit(session, instruments) {
    draft = structuredClone($state.snapshot(session));
    tickers = instruments;
    editing = true;
  }
</script>

<RequireModule module="backtest">
  {#key $page.params.id}
    <PaperDashboard bind:this={dashboard} sessionId={$page.params.id} onedit={edit} />
  {/key}
  <PaperModal bind:open={editing} bind:session={draft} {tickers} onsaved={() => dashboard?.reload()} />
</RequireModule>
