<script>
  // Capital returns: dividends per share (provider), cash spent on buybacks and
  // dividends (EDGAR cash-flow statements), splits and recent ex-dates.
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let c = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const r = await fundamentalsApi.tab(company.ticker, 'capital', force);
    c = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

{#if c}
  <div class="fd-grid">
    <Section key="company.dividends" title={$t('fundamentals.cap.dividends')} description={$t('fundamentals.cap.dividendsDesc')}>
      {#if !c.dividends.length}<p class="fd-muted">{$t('fundamentals.none')}</p>{/if}
      <Chart
        categories={c.dividends.map((d) => d.period)}
        series={[{ name: $t('fundamentals.cap.dps'), type: 'bar', data: c.dividends.map((d) => d.amount) }]}
        legend={false}
        height={240}
      />
    </Section>
    <Section key="company.buybacks" title={$t('fundamentals.cap.buybacks')} description={$t('fundamentals.cap.buybacksDesc')} source="SEC XBRL">
      <Chart
        categories={c.buybacks.map((b) => b.year)}
        series={[
          { name: $t('fundamentals.cap.buybacks'), type: 'bar', data: c.buybacks.map((b) => b.amount ?? null) },
          { name: $t('fundamentals.line.dividends_paid'), type: 'bar', data: c.buybacks.map((b) => b.dividends ?? null) }
        ]}
        yFormat={(v) => fmtBig(v)}
        height={240}
      />
    </Section>
  </div>
  <div class="fd-grid">
    <Section key="company.splits" title={$t('fundamentals.cap.splits')}>
      {#if c.splits.length}
        <table class="tbl">
          <thead><tr><th>{$t('fundamentals.date')}</th><th class="num">{$t('fundamentals.cap.ratio')}</th></tr></thead>
          <tbody>
            {#each c.splits as s, i (i)}
              <tr><td>{s.date}</td><td class="num">{s.ratio}</td></tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="fd-muted">{$t('fundamentals.none')}</p>
      {/if}
    </Section>
    <Section key="company.corpActions" title={$t('fundamentals.cap.actions')} description={$t('fundamentals.cap.actionsDesc')}>
      <table class="tbl">
        <thead><tr><th>{$t('fundamentals.date')}</th><th>{$t('fundamentals.type')}</th><th>{$t('fundamentals.alt.description')}</th></tr></thead>
        <tbody>
          {#each c.actions as a, i (i)}
            <tr><td>{a.date}</td><td>{a.type}</td><td class="fd-muted">{a.detail}</td></tr>
          {/each}
          {#each c.dividends.slice(-4).toReversed() as d, i (i)}
            <tr><td>{d.ex_date}</td><td>{$t('fundamentals.cap.exDate')}</td><td class="num">{fmtNum(d.amount, 3)}</td></tr>
          {/each}
        </tbody>
      </table>
    </Section>
  </div>
{/if}
