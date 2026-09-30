<script>
  // Volatility targeting: hold target vol / estimated vol of the asset, capped by a maximum
  // leverage and sized on the estimate known before each bar. Shows the exposure to hold now
  // and how the scaled track compares with holding the asset flat.
  import './panel.css';
  import QChart from './QChart.svelte';
  import DatasetPicker from './DatasetPicker.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, timeAxis, tooltip, title, legend, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasets } = $props();

  let datasetId = $state(null);
  let target = $state(15);
  let winLen = $state(20);
  let estimator = $state('rolling');
  let maxLev = $state(2);
  let equity = $state(100000);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  async function run() {
    if (!datasetId) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.voltarget({
        dataset_id: datasetId,
        target: Number(target) / 100,
        window: Math.round(Number(winLen)) || 20,
        estimator,
        max_leverage: Number(maxLev),
        equity: Number(equity) > 0 ? Number(equity) : null
      });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const r = $derived(data?.result);

  const eqBuild = $derived.by(() => {
    if (!r) return null;
    const names = [data.ticker, $t('quant.voltarget.scaled')];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.voltarget.equityTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (+v).toFixed(3) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { type: 'log', scale: true }),
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: r.points.map((q) => [q[0], q[3]]), lineStyle: { color: p.muted, width: 1.2 }, itemStyle: { color: p.muted } },
        { name: names[1], type: 'line', symbol: 'none', data: r.points.map((q) => [q[0], q[4]]), lineStyle: { color: p.series[0], width: 1.5 }, itemStyle: { color: p.series[0] } }
      ]
    });
  });

  const wBuild = $derived.by(() => {
    if (!r) return null;
    const names = [$t('quant.voltarget.weight'), $t('quant.voltarget.estVol')];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.voltarget.weightTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 48, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 1) }),
      xAxis: timeAxis(p),
      yAxis: [
        valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => `${(v * 100).toFixed(0)}%` } }),
        valueAxis(p, { splitLine: { show: false }, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => `${(v * 100).toFixed(0)}%` } })
      ],
      series: [
        { name: names[0], type: 'line', symbol: 'none', step: 'end', data: r.points.map((q) => [q[0], q[1]]), lineStyle: { color: p.series[0], width: 1.2 }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', symbol: 'none', yAxisIndex: 1, data: r.points.map((q) => [q[0], q[2]]), lineStyle: { color: p.amber, width: 1 }, itemStyle: { color: p.amber } }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field">
      <span class="qp-label">{$t('quant.voltarget.asset')}</span>
      <DatasetPicker bind:value={datasetId} {datasets} />
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.voltarget.target')}</span><input type="number" min="1" max="200" step="1" bind:value={target} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.vol.window')}</span><input type="number" min="2" max="1000" bind:value={winLen} /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.voltarget.estimator')}</span>
      <div class="qp-seg">
        {#each ['rolling', 'ewma'] as e (e)}
          <button class:active={estimator === e} onclick={() => (estimator = e)}>{$t(`quant.voltarget.${e}`)}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.voltarget.maxLev')}</span><input type="number" min="0" max="20" step="0.1" bind:value={maxLev} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.voltarget.equity')}</span><input type="number" min="0" step="1000" bind:value={equity} /></label>
    <button class="primary" onclick={run} disabled={!datasetId || busy}>{busy ? $t('quant.page.computing') : $t('quant.page.compute')}</button>
  </div>

  <ErrorText {error} />

  {#if r}
    <section class="qp-block">
      <h3>{$t('quant.voltarget.now', { a: data.ticker })}</h3>
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.voltarget.estVol')}</span><span class="v">{fmtPc(r.current_vol, 1)}</span><span class="s">{$t(`quant.voltarget.${estimator}`)} · {winLen}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.voltarget.exposure')}</span><span class="v">{fmtPc(r.current_weight, 0)}</span><span class="s">{r.current_weight >= r.max_leverage - 1e-9 ? $t('quant.voltarget.capped') : $t('quant.voltarget.exposureHint', { t: fmtPc(r.target, 0) })}</span></div>
        {#if r.units != null}
          <div class="qp-card"><span class="k">{$t('quant.voltarget.units')}</span><span class="v">{fmtNum(r.units, r.units < 10 ? 4 : 2)}</span><span class="s">{$t('quant.voltarget.unitsHint', { n: fmtNum(r.notional, 0), p: fmtNum(data.last_close, 2) })}</span></div>
        {/if}
        <div class="qp-card"><span class="k">{$t('quant.voltarget.avgWeight')}</span><span class="v">{fmtPc(r.avg_weight, 0)}</span></div>
      </div>
    </section>
    <section class="qp-block">
      <h3>{$t('quant.voltarget.track')}</h3>
      <p class="qp-hint">{$t('quant.voltarget.trackHint', { n: r.observations })}</p>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead><tr><th></th><th class="num">{$t('quant.compare.annVol')}</th><th class="num">{$t('quant.compare.annReturn')}</th><th class="num">Sharpe</th><th class="num">{$t('quant.basket.maxDd')}</th></tr></thead>
          <tbody>
            <tr><td>{data.ticker}</td><td class="num">{fmtPc(r.asset_vol, 1)}</td><td class="num">{fmtPc(r.asset_return, 1)}</td><td class="num">{fmtX(r.asset_sharpe)}</td><td class="num qp-bad">−{fmtPc(r.asset_max_dd, 1)}</td></tr>
            <tr><td>{$t('quant.voltarget.scaled')}</td><td class="num">{fmtPc(r.scaled_vol, 1)}</td><td class="num">{fmtPc(r.scaled_return, 1)}</td><td class="num">{fmtX(r.scaled_sharpe)}</td><td class="num qp-bad">−{fmtPc(r.scaled_max_dd, 1)}</td></tr>
          </tbody>
        </table>
      </div>
      <div class="qp-grid2">
        <QChart build={eqBuild} height={260} />
        <QChart build={wBuild} height={260} />
      </div>
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.voltarget.pickHint')}</p>
  {/if}
</div>
