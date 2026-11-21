<script>
  // Pairs tab: are two series tied together in the long run (cointegration), how far apart are
  // they now (spread z-score, half-life), how does their co-movement drift (rolling correlation
  // and beta), and does one move first (lead-lag, Granger).
  import './panel.css';
  import QChart from './QChart.svelte';
  import DatasetPicker from './DatasetPicker.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, timeAxis, categoryAxis, tooltip, title, legend, fmtP, fmtX, fmtPc, stars } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasets } = $props();

  let yId = $state(null);
  let xId = $state(null);
  let winLen = $state(60);
  let maxLag = $state(5);
  let useLog = $state(true);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  async function run() {
    if (!yId || !xId) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.pairs([yId, xId], { window: Number(winLen) || 60, max_lag: Number(maxLag) || 5, log: useLog });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const yl = $derived(data?.labels?.[0] ?? 'Y');
  const xl = $derived(data?.labels?.[1] ?? 'X');
  const verdict = (p) => (p == null ? '' : p < 0.05 ? $t('quant.pairs.cointegrated') : $t('quant.pairs.notCointegrated'));

  const zBuild = $derived.by(() => {
    const sp = data?.spread;
    if (!sp) return null;
    const names = [$t('quant.pairs.zFull'), $t('quant.pairs.zRolling', { n: sp.window })];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.pairs.zTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 40, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (v == null ? '−' : (+v).toFixed(2)) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p),
      series: [
        {
          name: names[0],
          type: 'line',
          symbol: 'none',
          data: sp.points.map((q) => [q.ts, +q.z.toFixed(3)]),
          lineStyle: { color: p.series[0], width: 1.2 },
          itemStyle: { color: p.series[0] },
          markLine: {
            symbol: 'none',
            silent: true,
            label: { show: false },
            lineStyle: { color: p.muted, type: 'dashed' },
            data: [{ yAxis: 2 }, { yAxis: -2 }, { yAxis: 0 }]
          }
        },
        {
          name: names[1],
          type: 'line',
          symbol: 'none',
          data: sp.points.map((q) => [q.ts, q.z_rolling == null ? null : +q.z_rolling.toFixed(3)]),
          lineStyle: { color: p.series[4], width: 1 },
          itemStyle: { color: p.series[4] }
        }
      ]
    });
  });

  const rollBuild = $derived.by(() => {
    if (!data?.rolling?.length) return null;
    const names = [$t('quant.pairs.rollingCorr'), $t('quant.pairs.rollingBeta', { y: yl, x: xl })];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.pairs.rollingTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 40, right: 44, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (v == null ? '−' : (+v).toFixed(3)) }),
      xAxis: timeAxis(p),
      yAxis: [valueAxis(p, { min: -1, max: 1 }), valueAxis(p, { splitLine: { show: false } })],
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: data.rolling.map((q) => [q.ts, q.corr]), lineStyle: { color: p.series[0], width: 1.2 }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', symbol: 'none', yAxisIndex: 1, data: data.rolling.map((q) => [q.ts, q.beta]), lineStyle: { color: p.series[1], width: 1.2 }, itemStyle: { color: p.series[1] } }
      ]
    });
  });

  const lagBuild = $derived.by(() => {
    if (!data?.lead_lag?.length) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.pairs.leadLagTitle', { y: yl, x: xl })),
      grid: { left: 44, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => (+v).toFixed(4) }),
      xAxis: categoryAxis(p, data.lead_lag.map((q) => String(q.lag)), { name: $t('quant.pairs.lagBars'), nameLocation: 'middle', nameGap: 22, nameTextStyle: { color: p.dim, fontSize: 10 } }),
      yAxis: valueAxis(p),
      series: [{ type: 'bar', data: data.lead_lag.map((q) => +q.corr.toFixed(4)), itemStyle: { color: p.series[0] } }]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field">
      <span class="qp-label">{$t('quant.pairs.first')}</span>
      <DatasetPicker bind:value={yId} {datasets} />
    </div>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.pairs.second')}</span>
      <DatasetPicker bind:value={xId} {datasets} />
    </div>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.vol.window')}</span>
      <input type="number" min="10" max="1000" bind:value={winLen} />
    </label>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.pairs.maxLag')}</span>
      <input type="number" min="1" max="20" bind:value={maxLag} />
    </label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.pairs.prices')}</span>
      <div class="qp-seg">
        <button class:active={useLog} onclick={() => (useLog = true)}>{$t('quant.pairs.log')}</button>
        <button class:active={!useLog} onclick={() => (useLog = false)}>{$t('quant.pairs.raw')}</button>
      </div>
    </div>
    <button class="primary" onclick={run} disabled={!yId || !xId || yId === xId || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if data}
    <p class="qp-hint">
      {$t('quant.pairs.summary', { y: yl, x: xl, n: data.periods, tf: data.measured_at, c: fmtX(data.correlation, 3) })}
    </p>

    <section class="qp-block">
      <h3>{$t('quant.pairs.cointegration')}</h3>
      <p class="qp-hint">{$t('quant.pairs.cointegrationHint')}</p>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr><th>Engle-Granger</th><th class="num raw">β</th><th class="num raw">τ</th><th class="num">p</th><th class="num">{$t('quant.pairs.crit5')}</th><th>{$t('quant.stats.verdict')}</th></tr>
          </thead>
          <tbody>
            {#each [[`${yl} ~ ${xl}`, data.engle_granger_yx], [`${xl} ~ ${yl}`, data.engle_granger_xy]] as [label, eg] (label)}
              <tr>
                <td>{label}</td>
                <td class="num">{fmtX(eg?.beta, 3)}</td>
                <td class="num">{fmtX(eg?.stat)}</td>
                <td class="num">{fmtP(eg?.pvalue)} {stars(eg?.pvalue)}</td>
                <td class="num">{fmtX(eg?.crit?.[1])}</td>
                <td>{verdict(eg?.pvalue)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if data.johansen}
        <div class="qp-scroll">
          <table class="qp-table">
            <thead>
              <tr><th>Johansen H0</th><th class="num">{$t('quant.pairs.eigenvalue')}</th><th class="num">{$t('quant.pairs.trace')}</th><th class="num">95%</th><th class="num">{$t('quant.pairs.maxEig')}</th><th class="num">95%</th></tr>
            </thead>
            <tbody>
              {#each data.johansen.ranks as r (r.r)}
                <tr>
                  <td>r ≤ {r.r}</td>
                  <td class="num">{fmtX(r.eigenvalue, 4)}</td>
                  <td class="num" class:qp-good={r.trace > r.trace_crit[1]}>{fmtX(r.trace)}</td>
                  <td class="num qp-muted">{fmtX(r.trace_crit[1])}</td>
                  <td class="num" class:qp-good={r.max_eig > r.max_eig_crit[1]}>{fmtX(r.max_eig)}</td>
                  <td class="num qp-muted">{fmtX(r.max_eig_crit[1])}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="qp-hint">{$t('quant.pairs.johansenRank', { r: data.johansen.rank_95 })}</p>
      {/if}
    </section>

    {#if data.spread}
      {@const sp = data.spread}
      <section class="qp-block">
        <h3>{$t('quant.pairs.spread')}</h3>
        <p class="qp-hint">{$t('quant.pairs.spreadHint', { y: yl, x: xl, b: fmtX(sp.hedge_ratio, 3) })}</p>
        <div class="qp-cards">
          <div class="qp-card"><span class="k">{$t('quant.pairs.hedgeRatio')}</span><span class="v">{fmtX(sp.hedge_ratio, 3)}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.pairs.zNow')}</span><span class="v" class:qp-bad={Math.abs(sp.z_now) > 2}>{fmtX(sp.z_now)}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.pairs.zRollingNow')}</span><span class="v" class:qp-bad={Math.abs(sp.z_rolling_now ?? 0) > 2}>{fmtX(sp.z_rolling_now)}</span></div>
          <div class="qp-card">
            <span class="k">{$t('quant.pairs.halfLife')}</span>
            <span class="v">{sp.half_life?.half_life != null ? fmtX(sp.half_life.half_life, 1) : '∞'}</span>
          </div>
          <div class="qp-card"><span class="k raw">{$t('quant.pairs.beyond2')}</span><span class="v">{fmtPc(sp.beyond_2, 1)}</span></div>
        </div>
        <QChart build={zBuild} height={260} />
      </section>
    {/if}

    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.pairs.comovement')}</h3>
        <QChart build={rollBuild} height={240} />
      </section>
      <section class="qp-block">
        <h3>{$t('quant.pairs.leadLag')}</h3>
        <p class="qp-hint">{$t('quant.pairs.leadLagHint', { y: yl, x: xl })}</p>
        <QChart build={lagBuild} height={200} />
        <div class="qp-scroll">
          <table class="qp-table">
            <thead>
              <tr><th>{$t('quant.pairs.grangerLag')}</th><th class="num">{xl} → {yl} (p)</th><th class="num">{yl} → {xl} (p)</th></tr>
            </thead>
            <tbody>
              {#each data.granger_x_to_y as g, i (g.lag)}
                <tr>
                  <td>{g.lag}</td>
                  <td class="num">{fmtP(g.pvalue)} {stars(g.pvalue)}</td>
                  <td class="num">{fmtP(data.granger_y_to_x[i]?.pvalue)} {stars(data.granger_y_to_x[i]?.pvalue)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.pairs.pickHint')}</p>
  {/if}
</div>
