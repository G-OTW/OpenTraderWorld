<script>
  // Revenue by business segment and by region (stacked), plus company-specific KPIs.
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let s = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const r = await fundamentalsApi.tab(company.ticker, 'segments', force);
    s = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });

  const stacked = (list) => list.map((x) => ({ name: x.name, type: 'bar', stack: 'a', data: x.values }));
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

{#if s}
  <div class="fd-grid">
    <Section key="company.segProduct" title={$t('fundamentals.seg.product')} description={$t('fundamentals.seg.productDesc')}>
      <Chart categories={s.years} series={stacked(s.product)} yFormat={(v) => fmtBig(v)} height={300} />
    </Section>
    <Section key="company.segRegion" title={$t('fundamentals.seg.region')} description={$t('fundamentals.seg.regionDesc')}>
      <Chart categories={s.years} series={stacked(s.region)} yFormat={(v) => fmtBig(v)} height={300} />
    </Section>
  </div>
  {#if s.kpis?.length}
  <Section key="company.kpis" title={$t('fundamentals.seg.kpis')} description={$t('fundamentals.seg.kpisDesc')}>
    <div class="fd-scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>KPI</th>
            {#each s.years as y (y)}<th class="num">{y}</th>{/each}
          </tr>
        </thead>
        <tbody>
          {#each s.kpis as k (k.name)}
            <tr>
              <td>{k.name}</td>
              {#each k.values as v, i (i)}<td class="num">{fmtNum(v, 1)}</td>{/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </Section>
  {/if}
{/if}
