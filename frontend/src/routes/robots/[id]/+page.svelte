<script>
  import { page } from '$app/state';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import RobotDashboard from '$lib/modules/robots/RobotDashboard.svelte';
  import RobotEditor from '$lib/modules/robots/RobotEditor.svelte';

  let dashboard = $state();
  let editing = $state(false);
  let draft = $state(null);
  let tickers = $state([]);

  function edit(session, instruments) {
    draft = structuredClone($state.snapshot(session));
    tickers = instruments;
    editing = true;
  }
</script>

<RequireModule module="robots">
  {#key page.params.id}
    <RobotDashboard bind:this={dashboard} sessionId={page.params.id} onedit={edit} />
  {/key}
  <RobotEditor bind:open={editing} bind:session={draft} {tickers} onsaved={() => dashboard?.reload()} />
</RequireModule>
