<script>
  // `projection`: a goal (value by a date), progress toward it, the monthly contribution it
  // needs at the book's measured return, and a cone of 1000 paths drawn from the measured
  // return and volatility. A book too short to annualize is refused, not extrapolated.
  //
  // The cone is a band (p10 to p90) with the median dashed in the value chart's colour:
  // a projection is never drawn solid, measured history is. The goal is a dashed line too.
  import { t } from '$lib/i18n';
  import { dateKey } from '$lib/format.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import Unavailable from './Unavailable.svelte';
  import Panel from './Panel.svelte';
  import { portfoliosApi, fmtMoney, fmtCompact, na } from '../api.js';

  let { portfolioId, currency = 'USD', onfix = null } = $props();

  let data = $state(null);
  let error = $state(null);
  let saving = $state(false);
  let form = $state({ target_value: '', target_date: '', monthly_contribution: '' });
  const c = $derived(chartColors());

  function load() {
    portfoliosApi
      .projection(portfolioId)
      .then((r) => {
        data = r;
        if (r.goal) {
          form = {
            target_value: r.goal.target_value,
            target_date: r.goal.target_date,
            monthly_contribution: r.goal.monthly_contribution
          };
        }
      })
      .catch((e) => (error = e.message));
  }

  $effect(() => {
    if (portfolioId) load();
  });

  async function save() {
    saving = true;
    error = null;
    try {
      await portfoliosApi.saveGoal(portfolioId, {
        target_value: Number(form.target_value),
        target_date: form.target_date,
        monthly_contribution: Number(form.monthly_contribution) || 0
      });
      load();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  const pct = (v) => (v == null ? na() : `${(v * 100).toFixed(1)}%`);
  const money = (v) => fmtMoney(v, currency);

  // Month n of the cone, as a calendar date from today: the axis reads in dates, not in
  // "month 14".
  function monthAt(n) {
    const d = new Date();
    d.setDate(1);
    d.setMonth(d.getMonth() + n);
    return dateKey(d);
  }
  const cone = $derived(data?.status === 'ok' ? (data.cone ?? []) : []);
  const pts = (key) => cone.map((p) => [monthAt(p.month), p[key]]);
</script>

<div class="pa-stack">
  <section class="goal">
    <Input label={$t('portfolios.goal.target', { ccy: currency })} type="number" step="any" min="0" bind:value={form.target_value} />
    <Input label={$t('portfolios.goal.date')} type="date" bind:value={form.target_date} />
    <Input label={$t('portfolios.goal.monthly', { ccy: currency })} type="number" step="any" min="0" bind:value={form.monthly_contribution} />
    <div class="save">
      <Button variant="primary" onclick={save} loading={saving} disabled={!(Number(form.target_value) > 0) || !form.target_date}>
        {$t('portfolios.goal.save')}
      </Button>
    </div>
  </section>

  <ErrorText {error} />

  {#if !data}
    <div class="pa-stats">
      {#each Array(4) as _, i (i)}<Skeleton height="64px" />{/each}
    </div>
    <Skeleton height="280px" />
  {:else if data.status !== 'ok'}
    <Unavailable missing={data.missing ?? []} {onfix} />
  {:else}
    <div class="pa-stats">
      <StatCard label={$t('portfolios.goal.progress')} value={`${data.progress_pct.toFixed(1)}%`} hint={money(data.value)} />
      <StatCard
        label={$t('portfolios.goal.required')}
        value={money(data.required_monthly)}
        hint={$t('portfolios.goal.requiredHint', { r: pct(data.measured.annual_return) })}
      />
      <StatCard
        label={$t('portfolios.goal.measured')}
        value={`${pct(data.measured.annual_return)} / ${pct(data.measured.annual_vol)}`}
        hint={$t('portfolios.goal.measuredHint', { years: data.measured.years.toFixed(1) })}
      />
      {#if data.end}
        <StatCard
          label={$t('portfolios.goal.median')}
          value={money(data.end.p50)}
          hint={$t('portfolios.goal.range', { lo: money(data.end.p10), hi: money(data.end.p90) })}
        />
      {/if}
    </div>

    <Panel
      title={$t('portfolios.goal.coneTitle')}
      subtitle={$t('portfolios.goal.coneSub', { n: data.paths })}
      height={260}
      unavailable={cone.length < 2 ? { reason: $t('portfolios.analytics.missing.too_short'), icon: 'trending-up' } : null}
    >
      <TimeSeries
        series={[
          { key: 'p50', label: $t('portfolios.goal.p50'), points: pts('p50'), color: c.accent, dashed: true },
          {
            key: 'target',
            label: $t('portfolios.goal.targetLine'),
            points: cone.map((p) => [monthAt(p.month), data.goal.target_value]),
            color: c.dim,
            dashed: true,
            width: 1
          }
        ]}
        bands={[{ key: 'cone', label: $t('portfolios.goal.band'), lower: pts('p10'), upper: pts('p90'), color: c.accent }]}
        height={260}
        change="none"
        format={money}
        xAxisFormat={(sec) => new Date(sec * 1000).toLocaleDateString(undefined, { month: 'short', year: '2-digit' })}
        axisFormat={(v) => fmtCompact(v, currency)}
      />
    </Panel>
  {/if}
</div>

<style>
  .goal {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr)) auto;
    gap: var(--space-3);
    align-items: end;
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
  }
  @media (max-width: 767px) {
    .goal {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
