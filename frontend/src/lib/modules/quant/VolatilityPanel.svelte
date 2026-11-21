<script>
  // Volatility tab: five estimators from the same bars (close-to-close ignores the intrabar
  // range, the range estimators use it), their rolling paths, the volatility cone that says
  // whether today's reading is high or low for its horizon, and a GARCH(1,1) forecast.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, timeAxis, categoryAxis, tooltip, title, legend, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasetId, win } = $props();

  let winLen = $state(21);
  let horizon = $state(21);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    void datasetId;
    void win;
    data = null;
  });

  async function run() {
    if (!datasetId) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.volatility(datasetId, { ...win, window: Number(winLen) || 21, horizon: Number(horizon) || 21 });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const r = $derived(data?.result);
  const pc = (v) => (v == null ? null : +(v * 100).toFixed(2));

  const estimators = $derived(
    r
      ? [
          ['close_to_close', $t('quant.vol.closeToClose'), $t('quant.vol.closeToCloseHint')],
          ['parkinson', 'Parkinson', $t('quant.vol.parkinsonHint')],
          ['garman_klass', 'Garman-Klass', $t('quant.vol.garmanKlassHint')],
          ['rogers_satchell', 'Rogers-Satchell', $t('quant.vol.rogersSatchellHint')],
          ['yang_zhang', 'Yang-Zhang', $t('quant.vol.yangZhangHint')]
        ]
      : []
  );

  const rollingBuild = $derived.by(() => {
    if (!r) return null;
    const names = [$t('quant.vol.closeToClose'), 'Parkinson', 'Yang-Zhang'];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.vol.rollingTitle', { n: r.window })),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (v == null ? '−' : `${v}%`) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' } }),
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: r.rolling.map((x) => [x.ts, pc(x.cc)]), lineStyle: { width: 1.2, color: p.series[0] }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', symbol: 'none', data: r.rolling.map((x) => [x.ts, pc(x.parkinson)]), lineStyle: { width: 1.2, color: p.series[1] }, itemStyle: { color: p.series[1] } },
        { name: names[2], type: 'line', symbol: 'none', data: r.rolling.map((x) => [x.ts, pc(x.yang_zhang)]), lineStyle: { width: 1.2, color: p.series[4] }, itemStyle: { color: p.series[4] } }
      ]
    });
  });

  const coneBuild = $derived.by(() => {
    if (!r?.cones?.length) return null;
    const x = r.cones.map((c) => String(c.window));
    const band = (k) => r.cones.map((c) => pc(c[k]));
    const line = (name, k, color, dash) => ({ name, type: 'line', data: band(k), symbol: 'none', lineStyle: { color, width: 1, type: dash ? 'dashed' : 'solid' }, itemStyle: { color } });
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.vol.coneTitle')),
      legend: legend(p),
      grid: { left: 48, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => (v == null ? '−' : `${v}%`) }),
      xAxis: categoryAxis(p, x, { name: $t('quant.vol.windowBars'), nameLocation: 'middle', nameGap: 22, nameTextStyle: { color: p.dim, fontSize: 10 } }),
      yAxis: valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' } }),
      series: [
        line($t('quant.vol.max'), 'max', p.red, true),
        line('P90', 'p90', p.amber, false),
        line($t('quant.vol.median'), 'median', p.muted, false),
        line('P10', 'p10', p.green, false),
        line($t('quant.vol.min'), 'min', p.green, true),
        { name: $t('quant.vol.current'), type: 'line', data: band('current'), symbolSize: 7, lineStyle: { color: p.accent, width: 2 }, itemStyle: { color: p.accent } }
      ]
    });
  });

  const garchBuild = $derived.by(() => {
    const g = r?.garch;
    if (!g) return null;
    const hs = g.forecast.map((_, i) => String(i + 1));
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.vol.garchForecast')),
      grid: { left: 48, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => `${v}%` }),
      xAxis: categoryAxis(p, hs, { name: $t('quant.vol.barsAhead'), nameLocation: 'middle', nameGap: 22, nameTextStyle: { color: p.dim, fontSize: 10 } }),
      yAxis: valueAxis(p, { scale: true, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' } }),
      series: [
        {
          type: 'line',
          data: g.forecast.map(pc),
          symbol: 'none',
          lineStyle: { color: p.series[0], width: 1.5 },
          markLine: g.long_run_vol != null
            ? { symbol: 'none', silent: true, data: [{ yAxis: pc(g.long_run_vol), label: { formatter: $t('quant.vol.longRun'), color: p.muted }, lineStyle: { color: p.muted, type: 'dashed' } }] }
            : undefined
        }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <label class="qp-field">
      <span class="qp-label">{$t('quant.vol.window')}</span>
      <input type="number" min="5" max="500" bind:value={winLen} />
    </label>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.vol.horizon')}</span>
      <input type="number" min="1" max="252" bind:value={horizon} />
    </label>
    <button class="primary" onclick={run} disabled={!datasetId || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if r}
    <section class="qp-block">
      <h3>{$t('quant.vol.estimators')}</h3>
      <p class="qp-hint">{$t('quant.vol.estimatorsHint')}</p>
      <div class="qp-cards">
        {#each estimators as [k, label, hint] (k)}
          <div class="qp-card"><span class="k">{label}</span><span class="v">{fmtPc(r.annual[k], 1)}</span><span class="s">{hint}</span></div>
        {/each}
      </div>
      <QChart build={rollingBuild} height={260} />
    </section>

    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.vol.cone')}</h3>
        <p class="qp-hint">{$t('quant.vol.coneHint')}</p>
        <QChart build={coneBuild} height={260} />
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.vol.windowBars')}</th><th class="num">{$t('quant.vol.current')}</th><th class="num">{$t('quant.vol.median')}</th><th class="num">{$t('quant.vol.rank')}</th></tr></thead>
            <tbody>
              {#each r.cones as c (c.window)}
                <tr>
                  <td>{c.window}</td>
                  <td class="num">{fmtPc(c.current, 1)}</td>
                  <td class="num">{fmtPc(c.median, 1)}</td>
                  <td class="num" class:qp-bad={c.current_rank > 0.9} class:qp-good={c.current_rank < 0.1}>{fmtPc(c.current_rank, 0)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>

      <section class="qp-block">
        <h3>GARCH(1,1)</h3>
        <p class="qp-hint">{$t('quant.vol.garchHint')}</p>
        {#if r.garch}
          {@const g = r.garch}
          <div class="qp-cards">
            <div class="qp-card"><span class="k raw">α</span><span class="v">{fmtX(g.alpha, 3)}</span><span class="s">{$t('quant.vol.alphaHint')}</span></div>
            <div class="qp-card"><span class="k raw">β</span><span class="v">{fmtX(g.beta, 3)}</span><span class="s">{$t('quant.vol.betaHint')}</span></div>
            <div class="qp-card"><span class="k raw">α + β</span><span class="v">{fmtX(g.persistence, 3)}</span><span class="s">{$t('quant.vol.halfLifeBars', { n: fmtX(g.half_life, 1) })}</span></div>
            <div class="qp-card"><span class="k">{$t('quant.vol.longRun')}</span><span class="v">{fmtPc(g.long_run_vol, 1)}</span><span class="s">{$t('quant.vol.nextBar', { v: fmtPc(g.forecast[0], 1) })}</span></div>
          </div>
          <QChart build={garchBuild} height={220} />
        {:else}
          <p class="qp-hint">{$t('quant.vol.garchTooShort')}</p>
        {/if}
      </section>
    </div>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.vol.pickHint')}</p>
  {/if}
</div>
