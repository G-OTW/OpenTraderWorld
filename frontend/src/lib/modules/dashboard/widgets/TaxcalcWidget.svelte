<script>
  // Tax-calculator widgets — the cards of the Taxcalc mockup, each a `variant`. Reads the
  // module's saved scenarios (each carries its last computed result) and its profiles; no
  // recomputation happens here, so the figures are exactly the ones the module last stored.
  //
  // Config: { variant, year, profileId, limit }.
  import { taxcalcApi, fmtMoney, fmtPct } from '$lib/modules/taxcalc/api.js';
  import { fmtSignedPct } from '$lib/format';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import Donut from './parts/Donut.svelte';
  import TrendChart from './parts/TrendChart.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'total');
  const limit = $derived(Math.max(1, Math.min(20, item.config?.limit ?? 8)));
  const profileId = $derived(item.config?.profileId || '');
  // The year the headline totals cover; unset means the newest year with a result.
  const year = $derived(item.config?.year ?? null);

  let scenarios = $state(null);
  let profiles = $state([]);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    let alive = true;
    Promise.all([taxcalcApi.scenarios(), taxcalcApi.profiles()])
      .then(([s, p]) => { if (alive) { scenarios = s; profiles = p; } })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const computed = $derived(
    (scenarios ?? []).filter((s) => s.result && (!profileId || s.profile_id === profileId))
  );
  const years = $derived([...new Set(computed.map((s) => s.tax_year))].sort((a, b) => a - b));
  const shownYear = $derived(year ?? years.at(-1) ?? null);
  const ofYear = $derived(computed.filter((s) => s.tax_year === shownYear));

  const ccy = $derived(ofYear[0]?.result?.currency ?? computed[0]?.result?.currency ?? 'USD');
  const totalTax = $derived(ofYear.reduce((sum, s) => sum + (s.result?.total_tax ?? 0), 0));

  // Previous year's total, for the mockup's "vs previous estimate".
  const prevYear = $derived(years[years.indexOf(shownYear) - 1] ?? null);
  const prevTotal = $derived(
    prevYear == null
      ? null
      : computed.filter((s) => s.tax_year === prevYear).reduce((sum, s) => sum + (s.result?.total_tax ?? 0), 0)
  );
  const deltaPct = $derived(
    prevTotal ? ((totalTax - prevTotal) / prevTotal) * 100 : null
  );

  // The engine's own per-line split of the year: what the tax is made of.
  const lines = $derived.by(() => {
    const sums = new Map();
    for (const s of ofYear) for (const l of s.result?.lines ?? []) {
      sums.set(l.label, (sums.get(l.label) ?? 0) + (l.tax ?? 0));
    }
    return [...sums]
      .map(([label, value]) => ({ label, value }))
      .filter((r) => r.value > 0)
      .sort((a, b) => b.value - a.value);
  });

  // One row per year: base, tax, effective rate, and the change on the year before.
  const byYear = $derived.by(() =>
    years.map((y, i) => {
      const rows = computed.filter((s) => s.tax_year === y);
      const tax = rows.reduce((sum, s) => sum + (s.result?.total_tax ?? 0), 0);
      const base = rows.reduce((sum, s) => sum + (s.result?.total_base ?? 0), 0);
      const rate = base ? (tax / base) * 100 : null;
      const prev = i > 0
        ? computed.filter((s) => s.tax_year === years[i - 1])
            .reduce((sum, s) => sum + (s.result?.total_tax ?? 0), 0)
        : null;
      return { year: y, tax, base, rate, yoy: prev ? ((tax - prev) / prev) * 100 : null };
    })
  );
  const rateSeries = $derived(byYear.map((r) => r.rate ?? 0));
  const profileName = $derived(
    profiles.find((p) => p.id === (profileId || ofYear[0]?.profile_id))?.name ?? ''
  );
</script>

<WidgetState
  {editing}
  error={err}
  loading={scenarios === null}
  empty={computed.length === 0}
  preview={$t('dashboard.widgets.taxcalc.preview')}
  emptyText={$t('dashboard.widgets.taxcalc.empty')}
  rows={4}
>
  {#if variant === 'rate'}
    <div class="w-body">
      <span class="w-eyebrow">{$t('dashboard.widgets.taxcalc.effectiveRate')}{profileName ? ` · ${profileName}` : ''}</span>
      <Stat
        value={byYear.at(-1)?.rate == null ? '—' : fmtPct(byYear.at(-1).rate)}
        size="sm"
        delta={byYear.length > 1 && byYear[0].rate != null && byYear.at(-1).rate != null
          ? fmtSignedPct(byYear.at(-1).rate - byYear[0].rate, 1)
          : ''}
        deltaTone={byYear.length > 1 && byYear.at(-1).rate > byYear[0].rate ? 'neg' : 'pos'}
        note={byYear.length > 1 ? $t('dashboard.widgets.taxcalc.vsYear', { year: byYear[0].year }) : ''}
      />
      {#if rateSeries.length > 1}
        <!-- The Stat above owns the headline; the chart carries the grid and the year axis. -->
        <TrendChart
          points={byYear.map((r) => ({ value: r.rate, label: String(r.year) }))}
          tone="accent"
          format={(v) => fmtPct(v)}
          axisFormat={(v) => fmtPct(v, 0)}
          showValue={false}
          showPill={false}
          label={$t('dashboard.widgets.taxcalc.effectiveRate')}
        />
      {/if}
    </div>
  {:else if variant === 'compare'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('dashboard.widgets.taxcalc.year')}</th>
          <th class="num">{$t('dashboard.widgets.taxcalc.base')}</th>
          <th class="num">{$t('dashboard.widgets.taxcalc.tax')}</th>
          <th class="num">{$t('dashboard.widgets.taxcalc.rate')}</th>
          <th class="num">{$t('dashboard.widgets.taxcalc.yoy')}</th>
        </tr>
      </thead>
      <tbody>
        {#each byYear.slice(-limit) as r (r.year)}
          <tr>
            <td>{r.year}</td>
            <td class="num">{fmtMoney(r.base, ccy)}</td>
            <td class="num">{fmtMoney(r.tax, ccy)}</td>
            <td class="num">{r.rate == null ? '—' : fmtPct(r.rate)}</td>
            <td class="num" class:w-neg={(r.yoy ?? 0) > 0} class:w-pos={(r.yoy ?? 0) < 0}>
              {r.yoy == null ? '—' : fmtSignedPct(r.yoy, 1)}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="w-body">
      <Stat
        value={fmtMoney(totalTax, ccy)}
        delta={deltaPct == null ? '' : fmtSignedPct(deltaPct, 0)}
        deltaTone={deltaPct == null ? '' : deltaPct > 0 ? 'neg' : 'pos'}
        note={shownYear == null ? '' : $t('dashboard.widgets.taxcalc.allScenarios', { year: shownYear })}
      />
      {#if lines.length}
        <Donut segments={lines} size={92} valueFormat={(v) => fmtMoney(v, ccy)} />
      {/if}
    </div>
  {/if}
</WidgetState>


