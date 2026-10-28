<script>
  // Fundamentals: ETF. Fund profile, top holdings, sector and country exposure, flows,
  // from the first connected ETF provider (FMP, Alpha Vantage, EODHD). Each block can be
  // hidden, folded or reordered.
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import Section from '$lib/modules/fundamentals/Section.svelte';
  import Chart from '$lib/modules/fundamentals/Chart.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import DataNote from '$lib/modules/fundamentals/DataNote.svelte';
  import SymbolPicker from '$lib/modules/fundamentals/SymbolPicker.svelte';
  import { fundamentalsApi, SUGGESTED_ETFS, fmtBig, fmtPct } from '$lib/modules/fundamentals/api.js';
  import { prefs, visibleSections } from '$lib/modules/fundamentals/prefs.svelte.js';
  import { t } from '$lib/i18n';

  const SECTIONS = ['profile', 'top', 'flows', 'sectors', 'countries'];
  let e = $state(null);
  // A failed load, as a DataNote line so Refresh stays at hand.
  let failure = $state(null);
  let busy = $state(false);
  let etfs = $state([]);

  async function load(force = false) {
    const tk = prefs.etf.ticker;
    busy = true;
    failure = null;
    try {
      e = await fundamentalsApi.etf(tk, force);
      // Its data is stored: keep it, at the front of the recent ones.
      fundamentalsApi.openedEtf(tk).then(loadEtfs).catch(() => {});
    } catch (err) {
      e = null;
      failure = { dataset: 'etf', error: err.deferred ? '' : err.message, deferred: err.deferred ?? [] };
    }
    busy = false;
  }

  $effect(() => {
    load();
  });

  const loadEtfs = () => fundamentalsApi.etfs().then((r) => (etfs = r)).catch(() => {});
  $effect(() => {
    loadEtfs();
  });

  async function follow(tk, followed) {
    await fundamentalsApi.followEtf(tk, followed);
    loadEtfs();
  }

  const sections = $derived(visibleSections('etf', SECTIONS));
</script>

{#snippet exposure(k, rows)}
  <Section key="etf.{k}" title={$t(`fundamentals.etf.${k}`)} description={$t(`fundamentals.etf.${k}Desc`)}>
    <table class="tbl">
      <tbody>
        {#each rows as r, i (i)}
          <tr>
            <td>{r.name}</td>
            <td class="bar"><span style="width: {r.weight}%"></span></td>
            <td class="num">{fmtPct(r.weight)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </Section>
{/snippet}

<Shell
  title={$t('fundamentals.etf.title')}
  subtitle={$t('fundamentals.etf.subtitle')}
  page="etf"
  sections={SECTIONS.map((id) => ({ id, label: $t(`fundamentals.etf.${id}`) }))}
>
  <SymbolPicker items={etfs} value={prefs.etf.ticker} placeholder={$t('fundamentals.etf.search')} fallback={SUGGESTED_ETFS} onpick={(tk) => (prefs.etf.ticker = tk)} onfollow={follow} />
  {#if e?.note}<DataNote notes={[e.note]} {busy} onrefresh={() => load(true)} />{/if}

  {#if failure}
    <DataNote notes={[failure]} {busy} onrefresh={() => load(true)} />
  {:else if e}
    <div class="fd-grid">
      {#each sections as id (id)}
        <div class:wide={id === 'profile'}>
          {#if id === 'profile'}
            <Section key="etf.profile" title={e.name} description={$t('fundamentals.etf.profileDesc')} source={e.issuer}>
              <div class="fd-stats">
                <StatCard label={$t('fundamentals.etf.aum')} value={fmtBig(e.aum)} />
                <StatCard label={$t('fundamentals.etf.expense')} value={fmtPct(e.expense, 2)} />
                <StatCard label={$t('fundamentals.etf.holdings')} value={e.holdings ?? '·'} />
                <StatCard label={$t('fundamentals.etf.index')} value={e.index || '·'} />
                <StatCard label={$t('fundamentals.etf.inception')} value={e.inception} />
              </div>
            </Section>
          {:else if id === 'top'}
            <Section key="etf.top" title={$t('fundamentals.etf.top')} description={$t('fundamentals.etf.topDesc')}>
              <Chart
                categories={e.top_holdings.map((h) => h.name)}
                series={[{ name: $t('fundamentals.etf.weight'), type: 'bar', data: e.top_holdings.map((h) => h.weight) }]}
                legend={false}
                yFormat={(v) => `${v}%`}
                height={280}
              />
            </Section>
          {:else if id === 'flows'}
            <Section key="etf.flows" title={$t('fundamentals.etf.flows')} description={$t('fundamentals.etf.flowsDesc')}>
              {#if !e.flows?.length}<p class="fd-muted">{$t('fundamentals.etf.noFlows')}</p>{:else}
              <Chart
                series={[
                  { name: $t('fundamentals.etf.monthlyFlow'), type: 'bar', data: e.flows.map(([ts, f]) => [ts, f]) },
                  { name: $t('fundamentals.etf.cumFlow'), data: e.flows.map(([ts, , c]) => [ts, c]), yAxisIndex: 1 }
                ]}
                dualAxis
                yFormat={(v) => `${Number(v).toFixed(1)}B`}
                height={280}
              />
              {/if}
            </Section>
          {:else if id === 'sectors'}
            {@render exposure('sectors', e.sectors)}
          {:else}
            {@render exposure('countries', e.countries)}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</Shell>

<style>
  .wide {
    grid-column: 1 / -1;
  }
  .bar {
    width: 45%;
  }
  .bar span {
    display: block;
    height: 6px;
    border-radius: 3px;
    background: var(--accent);
    opacity: 0.7;
  }
</style>
