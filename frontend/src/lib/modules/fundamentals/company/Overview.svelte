<script>
  // Company overview: the user's chosen metric tiles grouped by theme, the five-year
  // price line and the profile.
  import StatCard from '$lib/ui/StatCard.svelte';
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import DataNote from '../DataNote.svelte';
  import { fmtBig, fmtNum, fmtPct } from '../api.js';
  import { prefs, METRIC_GROUPS } from '../prefs.svelte.js';
  import { t } from '$lib/i18n';

  // props: company, price ({ snapshot, error }: the page loads it, since the header's
  //        market ratios depend on it too)
  let { company, price = null } = $props();

  const px = $derived(price?.snapshot?.data?.series ?? []);
  const priceNote = $derived(
    price
      ? [{ dataset: 'price', provider: price.snapshot?.provider_label ?? '', fetched_at: price.snapshot?.fetched_at ?? null, error: price.error ?? '' }]
      : []
  );

  const m = $derived(company.metrics);
  const values = $derived({
    marketCap: fmtBig(m.market_cap),
    ev: fmtBig(m.enterprise_value),
    pe: fmtNum(m.pe, 1),
    forwardPe: fmtNum(m.forward_pe, 1),
    evEbitda: fmtNum(m.ev_ebitda, 1),
    ps: fmtNum(m.ps, 1),
    pb: fmtNum(m.pb, 1),
    fcfYield: fmtPct(m.fcf_yield, 2),
    grossMargin: fmtPct(m.gross_margin),
    opMargin: fmtPct(m.operating_margin),
    netMargin: fmtPct(m.net_margin),
    roe: fmtPct(m.roe),
    roic: fmtPct(m.roic),
    debtEquity: fmtNum(m.debt_equity, 2),
    currentRatio: fmtNum(m.current_ratio, 2),
    beta: fmtNum(m.beta, 2),
    divYield: fmtPct(m.dividend_yield, 2),
    sharesOut: fmtBig(m.shares_out)
  });

  const groups = $derived(
    Object.entries(METRIC_GROUPS)
      .map(([g, keys]) => [g, keys.filter((k) => prefs.company.tiles.includes(k))])
      .filter(([, keys]) => keys.length)
  );
</script>

<Section
  key="company.metrics"
  title={$t('fundamentals.company.metrics')}
  description={m.basis ? `${$t('fundamentals.company.metricsDesc')} ${$t('fundamentals.company.basis', { basis: m.basis })}` : $t('fundamentals.company.metricsDesc')}
  source="SEC XBRL"
>
  {#if groups.length}
    {#each groups as [g, keys] (g)}
      <div class="fd-group">
        <h4>{$t(`fundamentals.metricGroup.${g}`)}</h4>
        <div class="fd-stats">
          {#each keys as k (k)}
            <StatCard label={$t(`fundamentals.metric.${k}`)} value={values[k]} />
          {/each}
        </div>
      </div>
    {/each}
  {:else}
    <p class="fd-muted">{$t('fundamentals.company.noTiles')}</p>
  {/if}
</Section>

<div class="fd-grid">
  <Section key="company.price" title={$t('fundamentals.company.price5y')} description={$t('fundamentals.company.priceDesc')}>
    <DataNote notes={priceNote} />
    {#if px.length}<Chart series={[{ name: company.ticker, data: px, area: true }]} legend={false} height={280} />{/if}
  </Section>
  <Section key="company.profile" title={$t('fundamentals.company.profile')} description={company.description} source="SEC EDGAR">
    <dl class="fd-kv">
      <dt>{$t('fundamentals.company.sector')}</dt><dd>{company.sector}</dd>
      <dt>{$t('fundamentals.company.industry')}</dt><dd>{company.industry}</dd>
      <dt>{$t('fundamentals.company.exchange')}</dt><dd>{company.exchange}</dd>
      <dt>{$t('fundamentals.company.country')}</dt><dd>{company.country}</dd>
      <dt>{$t('fundamentals.company.currency')}</dt><dd>{company.currency}</dd>
      <dt>CIK</dt><dd>{company.cik}</dd>
      <dt>SIC</dt><dd>{company.sic || '·'}</dd>
      <dt>{$t('fundamentals.company.fye')}</dt><dd>{company.fiscal_year_end}</dd>
      <dt>{$t('fundamentals.company.website')}</dt><dd>{company.website || '·'}</dd>
    </dl>
  </Section>
</div>
