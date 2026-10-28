<script>
  // Regression tab: what an asset's returns are made of. Alpha (what is left after the
  // factors), each factor's beta with its t-stat, how much the factors explain (R²), tracking
  // error and information ratio, and up/down capture against the first factor.
  import './panel.css';
  import QChart from './QChart.svelte';
  import DatasetPicker from './DatasetPicker.svelte';
  import DatasetChecklist from './DatasetChecklist.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, timeAxis, tooltip, title, fmtP, fmtX, fmtPc, stars } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasets } = $props();

  let assetId = $state(null);
  let factorIds = $state([]);
  let riskFree = $state(0);
  let winLen = $state(60);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  async function run() {
    if (!assetId || !factorIds.length) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.regression(assetId, factorIds, { risk_free: (Number(riskFree) || 0) / 100, window: Number(winLen) || 60 });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const g = $derived(data?.regression);
  const betaBuild = $derived.by(() => {
    if (!data?.rolling?.length || !g) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.regression.rollingTitle', { f: g.betas[0].label })),
      grid: { left: 40, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (v == null ? '−' : (+v).toFixed(3)) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p),
      series: [
        {
          type: 'line',
          symbol: 'none',
          data: data.rolling.map((q) => [q.ts, q.beta]),
          lineStyle: { color: p.series[0], width: 1.2 },
          markLine: { symbol: 'none', silent: true, label: { formatter: $t('quant.regression.fullSample'), color: p.muted, position: 'insideEndTop' }, lineStyle: { color: p.muted, type: 'dashed' }, data: [{ yAxis: +g.betas[0].beta.toFixed(4) }] }
        }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field">
      <span class="qp-label">{$t('quant.regression.asset')}</span>
      <DatasetPicker bind:value={assetId} {datasets} />
    </div>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.regression.factors', { n: factorIds.length })}</span>
      <DatasetChecklist {datasets} bind:value={factorIds} exclude={assetId} max={10} />
    </div>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.stats.riskFree')}</span>
      <input type="number" step="0.1" bind:value={riskFree} />
    </label>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.vol.window')}</span>
      <input type="number" min="10" max="1000" bind:value={winLen} />
    </label>
    <button class="primary" onclick={run} disabled={!assetId || !factorIds.length || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if g}
    <p class="qp-hint">{$t('quant.regression.summary', { a: g.asset, n: g.observations, tf: data.measured_at })}</p>
    <section class="qp-block">
      <h3>{$t('quant.regression.title')}</h3>
      <p class="qp-hint">{$t('quant.regression.hint')}</p>
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.regression.alpha')}</span><span class="v" class:qp-good={g.alpha_annual > 0} class:qp-bad={g.alpha_annual < 0}>{fmtPc(g.alpha_annual)}</span><span class="s">t {fmtX(g.alpha_t)} · p {fmtP(g.alpha_p)} {stars(g.alpha_p)}</span></div>
        <div class="qp-card"><span class="k">R²</span><span class="v">{fmtPc(g.r2, 1)}</span><span class="s">{$t('quant.regression.adjR2', { v: fmtPc(g.adj_r2, 1) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.regression.trackingError')}</span><span class="v">{fmtPc(g.tracking_error, 1)}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.regression.ir')}</span><span class="v">{fmtX(g.information_ratio)}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.regression.capture', { f: g.betas[0].label })}</span><span class="v">{fmtPc(g.up_capture, 0)} / {fmtPc(g.down_capture, 0)}</span><span class="s">{$t('quant.regression.captureHint')}</span></div>
      </div>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead><tr><th>{$t('quant.regression.factor')}</th><th class="num raw">β</th><th class="num">t</th><th class="num">p</th></tr></thead>
          <tbody>
            {#each g.betas as b (b.label)}
              <tr><td>{b.label}</td><td class="num">{fmtX(b.beta, 3)}</td><td class="num">{fmtX(b.t)}</td><td class="num">{fmtP(b.p)} {stars(b.p)}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
      <QChart build={betaBuild} height={240} />
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.regression.pickHint')}</p>
  {/if}
</div>
