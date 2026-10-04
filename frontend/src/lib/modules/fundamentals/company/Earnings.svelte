<script>
  // Earnings history: EPS estimate vs actual, surprise, revenue beat and next-day move.
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum, fmtPct } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let e = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const r = await fundamentalsApi.tab(company.ticker, 'earnings', force);
    e = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });

  const beats = $derived(e ? e.history.filter((h) => h.surprise_pct > 0).length : 0);
  const timing = (time) => (time === 'AMC' ? $t('fundamentals.earn.amc') : time === 'BMO' ? $t('fundamentals.earn.bmo') : '');
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

{#if e}
  <div class="fd-stats">
    <StatCard label={$t('fundamentals.earn.next')} value={e.next?.date ?? '·'} hint={timing(e.next?.time)} />
    <StatCard label={$t('fundamentals.earn.nextEps')} value={fmtNum(e.next?.eps_estimate)} />
    <StatCard label={$t('fundamentals.earn.beatRate')} value={`${beats} / ${e.history.length}`} hint={$t('fundamentals.earn.beatHint')} />
  </div>

  <Section key="company.epsChart" title={$t('fundamentals.earn.chart')} description={$t('fundamentals.earn.chartDesc')}>
    <Chart
      categories={e.history.map((h) => h.period)}
      series={[
        { name: $t('fundamentals.earn.estimate'), type: 'bar', data: e.history.map((h) => h.eps_estimate) },
        { name: $t('fundamentals.earn.actual'), type: 'bar', data: e.history.map((h) => h.eps_actual) }
      ]}
      height={260}
    />
  </Section>

  <Section key="company.epsTable" title={$t('fundamentals.earn.history')} description={$t('fundamentals.earn.historyDesc')}>
    <div class="fd-scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('fundamentals.period')}</th>
            <th>{$t('fundamentals.date')}</th>
            <th class="num">{$t('fundamentals.earn.estimate')}</th>
            <th class="num">{$t('fundamentals.earn.actual')}</th>
            <th class="num">{$t('fundamentals.earn.surprise')}</th>
            <th class="num">{$t('fundamentals.earn.revEst')}</th>
            <th class="num">{$t('fundamentals.earn.revActual')}</th>
            <th class="num">{$t('fundamentals.earn.move')}</th>
          </tr>
        </thead>
        <tbody>
          {#each e.history.toReversed() as h, i (i)}
            <tr>
              <td>{h.period}</td>
              <td>{h.date}</td>
              <td class="num">{fmtNum(h.eps_estimate)}</td>
              <td class="num">{fmtNum(h.eps_actual)}</td>
              <td class="num" class:fd-up={h.surprise_pct > 0} class:fd-down={h.surprise_pct < 0}>{fmtPct(h.surprise_pct, 1, true)}</td>
              <td class="num">{fmtBig(h.revenue_estimate)}</td>
              <td class="num">{fmtBig(h.revenue_actual)}</td>
              <td class="num" class:fd-up={h.move_next_day > 0} class:fd-down={h.move_next_day < 0}>{fmtPct(h.move_next_day, 1, true)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </Section>
{/if}
