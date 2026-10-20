<script>
  // ESG scores and executive pay (latest proxy year). Credit ratings are licensed data
  // with no personal API, so they are not shown.
  import Section from '../Section.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let r = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const res = await fundamentalsApi.tab(company.ticker, 'ratings', force);
    r = res.data;
    notes = res.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

{#if r}
  {#if r.esg}
  <Section key="company.esg" title={$t('fundamentals.rat.esg')} description={$t('fundamentals.rat.esgDesc')} source={r.esg.provider}>
    <div class="fd-stats">
      <StatCard label={$t('fundamentals.rat.total')} value={fmtNum(r.esg.total, 1)} />
      <StatCard label={$t('fundamentals.rat.env')} value={fmtNum(r.esg.environment, 1)} />
      <StatCard label={$t('fundamentals.rat.social')} value={fmtNum(r.esg.social, 1)} />
      <StatCard label={$t('fundamentals.rat.gov')} value={fmtNum(r.esg.governance, 1)} />
    </div>
  </Section>
  {/if}
  <Section key="company.management" title={$t('fundamentals.rat.management')} description={$t('fundamentals.rat.managementDesc')} source="DEF 14A">
    <table class="tbl">
      <thead><tr><th>{$t('fundamentals.name')}</th><th class="num">{$t('fundamentals.rat.year')}</th><th class="num">{$t('fundamentals.rat.pay')}</th></tr></thead>
      <tbody>
        {#each r.management as m, i (i)}
          <tr><td>{m.name}</td><td class="num">{m.since ?? '·'}</td><td class="num">{fmtBig(m.pay)}</td></tr>
        {:else}
          <tr><td colspan="3" class="fd-muted">{$t('fundamentals.none')}</td></tr>
        {/each}
      </tbody>
    </table>
  </Section>
{/if}
