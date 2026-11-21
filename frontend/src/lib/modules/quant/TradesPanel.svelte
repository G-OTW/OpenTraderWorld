<script>
  // Trades tab: what a saved backtest's trades are worth per unit of risk. Expectancy in
  // currency and in R, the R-multiple distribution, SQN, and the MAE/MFE scatter that shows
  // how much heat winners took and how far losers ran first.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, categoryAxis, tooltip, title, legend, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let runs = $state([]);
  let runId = $state(null);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    quantApi
      .backtestRuns()
      .then((r) => {
        runs = r;
        if (!runId && r.length) runId = r[0].id;
      })
      .catch((e) => (error = e.message));
  });

  async function run() {
    if (!runId) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.trades(runId);
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const a = $derived(data?.result);
  // Van Tharp's reading of SQN (on at most 100 trades).
  const sqnGrade = (v) =>
    v < 1.6 ? 'poor' : v < 2 ? 'belowAvg' : v < 2.5 ? 'average' : v < 3 ? 'good' : v < 5 ? 'excellent' : v < 7 ? 'superb' : 'grail';

  const histBuild = $derived.by(() => {
    const h = a?.r_histogram;
    if (!h) return null;
    const mids = h.counts.map((_, i) => (h.edges[i] + h.edges[i + 1]) / 2);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.trades.rDist')),
      grid: { left: 40, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p),
      xAxis: categoryAxis(p, mids.map((m) => `${m.toFixed(1)}R`)),
      yAxis: valueAxis(p),
      series: [{ type: 'bar', data: h.counts.map((c, i) => ({ value: c, itemStyle: { color: mids[i] >= 0 ? p.green : p.red } })), barCategoryGap: '8%' }]
    });
  });

  const scatter = (key, name) =>
    a
      ? (p) => ({
          animation: false,
          title: title(p, name),
          legend: legend(p, { data: [$t('quant.trades.winners'), $t('quant.trades.losers')] }),
          grid: { left: 44, right: 12, top: 32, bottom: 36 },
          tooltip: tooltip(p, { trigger: 'item', formatter: (q) => `${key.toUpperCase().slice(0, 3)} ${q.value[0].toFixed(2)}R · ${q.value[1].toFixed(2)}R` }),
          xAxis: valueAxis(p, { name: `${key === 'mae_r' ? 'MAE' : 'MFE'} (R)`, nameLocation: 'middle', nameGap: 22 }),
          yAxis: valueAxis(p, { name: 'R' }),
          series: [true, false].map((w) => ({
            name: w ? $t('quant.trades.winners') : $t('quant.trades.losers'),
            type: 'scatter',
            symbolSize: 5,
            itemStyle: { color: w ? p.green : p.red, opacity: 0.7 },
            data: a.points.filter((q) => q.win === w).map((q) => [q[key], q.r])
          }))
        })
      : null;
  const maeBuild = $derived(scatter('mae_r', $t('quant.trades.maeTitle')));
  const mfeBuild = $derived(scatter('mfe_r', $t('quant.trades.mfeTitle')));

  const curveBuild = $derived.by(() => {
    if (!a?.r_curve?.length) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.trades.rCurve')),
      grid: { left: 44, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => `${(+v).toFixed(2)}R` }),
      xAxis: { type: 'category', data: a.r_curve.map((q, i) => i + 1), axisLabel: { color: p.dim, fontSize: 10 }, axisLine: { lineStyle: { color: p.border } } },
      yAxis: valueAxis(p),
      series: [{ type: 'line', symbol: 'none', data: a.r_curve.map((q) => q[1]), lineStyle: { color: p.series[0], width: 1.2 }, areaStyle: { color: p.series[0], opacity: 0.08 } }]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field grow">
      <span class="qp-label">{$t('quant.mc.sourceRun')}</span>
      {#if runs.length === 0}
        <p class="qp-hint">{@html $t('quant.mc.noRunsHint')}</p>
      {:else}
        <Dropdown
          bind:value={runId}
          ariaLabel={$t('quant.mc.sourceRun')}
          options={runs.map((r) => ({ value: r.id, label: `${r.name} · ${r.ticker} ${r.timeframe} · ${fmtNum(r.stats?.trades ?? 0, 0)} ${$t('quant.mc.trades')}` }))}
        />
      {/if}
    </div>
    <button class="primary" onclick={run} disabled={!runId || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if a}
    <p class="qp-hint">
      {$t('quant.trades.summary', { name: data.name, n: a.trades })}
      {a.r_basis === 'stop' ? $t('quant.trades.basisStop') : $t('quant.trades.basisAvgLoss', { v: fmtNum(a.r_unit, 2) })}
    </p>
    <section class="qp-block">
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.trades.winRate')}</span><span class="v">{fmtPc(a.win_rate, 1)}</span><span class="s">{a.wins} / {a.losses}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.payoff')}</span><span class="v">{fmtX(a.payoff)}</span><span class="s">{fmtNum(a.avg_win, 2)} / {fmtNum(a.avg_loss, 2)}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.expectancy')}</span><span class="v" class:qp-good={a.expectancy > 0} class:qp-bad={a.expectancy < 0}>{fmtNum(a.expectancy, 2)}</span><span class="s">{$t('quant.trades.perTrade')}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.expectancyR')}</span><span class="v" class:qp-good={a.expectancy_r > 0} class:qp-bad={a.expectancy_r < 0}>{fmtX(a.expectancy_r)}R</span><span class="s">σ {fmtX(a.sd_r)}R</span></div>
        <div class="qp-card"><span class="k">SQN</span><span class="v">{fmtX(a.sqn_100)}</span><span class="s">{$t(`quant.trades.sqn.${sqnGrade(a.sqn_100)}`)} · {$t('quant.trades.sqnAll', { v: fmtX(a.sqn) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.edgeRatio')}</span><span class="v">{fmtX(a.edge_ratio)}</span><span class="s">{$t('quant.trades.edgeRatioHint')}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.bestWorst')}</span><span class="v">{fmtX(a.best_r, 1)}R / {fmtX(a.worst_r, 1)}R</span><span class="s">≥ 2R {fmtPc(a.r_ge_2, 0)} · ≤ −1R {fmtPc(a.r_le_minus_1, 0)}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.winnersHeat')}</span><span class="v">{a.winners_mae_p95_r == null ? '−' : `${fmtX(a.winners_mae_p95_r)}R`}</span><span class="s">{$t('quant.trades.winnersHeatHint')}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.trades.losersRan')}</span><span class="v">{a.losers_reached_1r == null ? '−' : fmtPc(a.losers_reached_1r, 0)}</span><span class="s">{$t('quant.trades.losersRanHint')}</span></div>
      </div>
      <p class="qp-hint">{$t('quant.trades.sqnHint')}</p>
    </section>
    <div class="qp-grid2">
      <section class="qp-block"><QChart build={histBuild} height={260} /></section>
      <section class="qp-block"><QChart build={curveBuild} height={260} /></section>
    </div>
    {#if !a.has_excursions}
      <p class="qp-hint">{$t('quant.trades.noExcursions')}</p>
    {:else}
      <div class="qp-grid2">
        <section class="qp-block">
          <QChart build={maeBuild} height={300} />
          <p class="qp-hint">{$t('quant.trades.maeHint')}</p>
        </section>
        <section class="qp-block">
          <QChart build={mfeBuild} height={300} />
          <p class="qp-hint">{$t('quant.trades.mfeHint')}</p>
        </section>
      </div>
    {/if}
  {:else if !busy}
    <p class="qp-hint">{$t('quant.trades.pickHint')}</p>
  {/if}
</div>

<style>
  .grow {
    flex: 1;
    min-width: 0;
    flex-basis: 280px;
    max-width: 640px;
  }

</style>
