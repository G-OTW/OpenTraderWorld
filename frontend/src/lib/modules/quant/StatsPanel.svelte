<script>
  // Statistics tab: what kind of series is this? Distribution shape against the normal,
  // serial dependence (returns and absolute returns), mean reversion versus trend (Hurst,
  // variance ratios, half-life), stationarity (ADF and KPSS read together), and whether the
  // Sharpe ratio is distinguishable from luck.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, tooltip, title, legend, fmtP, fmtX, fmtPc, stars } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasetId, win } = $props();

  let lags = $state(20);
  let riskFree = $state(0);
  let benchmark = $state(0);
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
      data = await quantApi.stats(datasetId, {
        ...win,
        lags: Number(lags) || 20,
        risk_free: (Number(riskFree) || 0) / 100,
        benchmark_sharpe: Number(benchmark) || 0
      });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const ppy = $derived(data?.periods_per_year ?? 252);

  // Histogram of log returns with the fitted normal density scaled to counts.
  const histBuild = $derived.by(() => {
    if (!data) return null;
    const { edges, counts } = data.histogram;
    const m = data.moments;
    const w = edges[1] - edges[0];
    const n = counts.reduce((a, b) => a + b, 0);
    const bars = counts.map((c, i) => [((edges[i] + edges[i + 1]) / 2) * 100, c]);
    const normal = [];
    for (let i = 0; i <= 120; i++) {
      const x = edges[0] + ((edges[edges.length - 1] - edges[0]) * i) / 120;
      const z = (x - m.mean) / m.stdev;
      normal.push([x * 100, (n * w * Math.exp(-0.5 * z * z)) / (m.stdev * Math.sqrt(2 * Math.PI))]);
    }
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.stats.histTitle')),
      legend: legend(p, { data: [$t('quant.stats.observed'), $t('quant.stats.normal')] }),
      grid: { left: 48, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { axisPointer: { type: 'line', snap: false } }),
      xAxis: valueAxis(p, { splitLine: { show: false }, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' } }),
      yAxis: valueAxis(p),
      series: [
        { name: $t('quant.stats.observed'), type: 'bar', data: bars, barWidth: '95%', itemStyle: { color: p.series[0], opacity: 0.65 } },
        { name: $t('quant.stats.normal'), type: 'line', data: normal, symbol: 'none', lineStyle: { color: p.amber, width: 1.5 } }
      ]
    });
  });

  const qqBuild = $derived.by(() => {
    if (!data) return null;
    const pts = data.qq;
    const lo = Math.min(...pts.map((q) => q[0]));
    const hi = Math.max(...pts.map((q) => q[0]));
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.stats.qqTitle')),
      grid: { left: 40, right: 12, top: 32, bottom: 28 },
      tooltip: { ...tooltip(p), trigger: 'item', formatter: (c) => `${c.value[0].toFixed(2)} → ${c.value[1].toFixed(2)}` },
      xAxis: valueAxis(p, { name: $t('quant.stats.theoretical'), nameLocation: 'middle', nameGap: 20, nameTextStyle: { color: p.dim, fontSize: 10 } }),
      yAxis: valueAxis(p),
      series: [
        { type: 'scatter', data: pts, symbolSize: 3, itemStyle: { color: p.series[0] } },
        { type: 'line', data: [[lo, lo], [hi, hi]], symbol: 'none', lineStyle: { color: p.amber, type: 'dashed', width: 1 }, silent: true }
      ]
    });
  });

  // ACF/PACF bars with the white-noise band.
  function corrBuild(c, head) {
    const lagsArr = c.acf.map((_, i) => i).slice(1);
    const bandLine = (v) => ({ yAxis: v, lineStyle: { color: 'inherit', type: 'dashed' } });
    return (p) => ({
      animation: false,
      title: title(p, head),
      legend: legend(p, { data: ['ACF', 'PACF'] }),
      grid: { left: 44, right: 12, top: 32, bottom: 24 },
      tooltip: tooltip(p),
      xAxis: { type: 'category', data: lagsArr, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false }, axisLabel: { color: p.dim, fontSize: 10 } },
      yAxis: valueAxis(p),
      series: [
        {
          name: 'ACF',
          type: 'bar',
          data: c.acf.slice(1).map((v) => +v.toFixed(4)),
          itemStyle: { color: p.series[0] },
          markLine: { symbol: 'none', silent: true, label: { show: false }, lineStyle: { color: p.muted }, data: [bandLine(c.band), bandLine(-c.band)] }
        },
        { name: 'PACF', type: 'bar', data: c.pacf.slice(1).map((v) => +v.toFixed(4)), itemStyle: { color: p.series[1] } }
      ]
    });
  }
  const acfBuild = $derived(data ? corrBuild(data.correlogram, $t('quant.stats.acfReturns')) : null);
  const acfAbsBuild = $derived(data ? corrBuild(data.correlogram_abs, $t('quant.stats.acfAbs')) : null);

  const hurstVerdict = (h) =>
    h == null ? '' : h < 0.45 ? $t('quant.stats.meanReverting') : h > 0.55 ? $t('quant.stats.persistent') : $t('quant.stats.randomWalk');

  // ADF rejects a unit root; KPSS rejects stationarity. Read together, they give four cases.
  function stationarity(adf, kpss) {
    if (!adf || !kpss) return '';
    const adfRej = adf.pvalue < 0.05;
    const kpssRej = kpss.pvalue <= 0.05;
    if (adfRej && !kpssRej) return $t('quant.stats.stationary');
    if (!adfRej && kpssRej) return $t('quant.stats.unitRoot');
    if (adfRej && kpssRej) return $t('quant.stats.differenceStationary');
    return $t('quant.stats.inconclusive');
  }
</script>

<div class="qp">
  <div class="qp-params">
    <label class="qp-field">
      <span class="qp-label">{$t('quant.stats.lags')}</span>
      <input type="number" min="5" max="100" bind:value={lags} />
    </label>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.stats.riskFree')}</span>
      <input type="number" step="0.1" bind:value={riskFree} />
    </label>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.stats.benchmarkSharpe')}</span>
      <input type="number" step="0.1" bind:value={benchmark} />
    </label>
    <button class="primary" onclick={run} disabled={!datasetId || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if data}
    {@const m = data.moments}
    {@const s = data.sharpe}
    <section class="qp-block">
      <h3>{$t('quant.stats.distribution')}</h3>
      <p class="qp-hint">{$t('quant.stats.distributionHint')}</p>
      {#if m}
        <div class="qp-cards">
          <div class="qp-card"><span class="k">{$t('quant.stats.meanAnnual')}</span><span class="v">{fmtPc(m.mean * ppy)}</span><span class="s">{$t('quant.stats.logReturns')}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.stats.volAnnual')}</span><span class="v">{fmtPc(m.stdev * Math.sqrt(ppy))}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.stats.skew')}</span><span class="v">{fmtX(m.skew)}</span><span class="s">{m.skew < 0 ? $t('quant.stats.leftTail') : $t('quant.stats.rightTail')}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.stats.excessKurtosis')}</span><span class="v">{fmtX(m.excess_kurtosis)}</span><span class="s">{$t('quant.stats.kurtosisHint')}</span></div>
          <div class="qp-card"><span class="k">Jarque-Bera</span><span class="v">{fmtX(m.jarque_bera, 1)}</span><span class="s">p {fmtP(m.jb_pvalue)} · {m.jb_pvalue < 0.05 ? $t('quant.stats.notNormal') : $t('quant.stats.normalOk')}</span></div>
        </div>
      {/if}
      <div class="qp-grid2">
        <QChart build={histBuild} height={260} />
        <QChart build={qqBuild} height={260} />
      </div>
    </section>

    <section class="qp-block">
      <h3>{$t('quant.stats.dependence')}</h3>
      <p class="qp-hint">{$t('quant.stats.dependenceHint')}</p>
      <div class="qp-grid2">
        <QChart build={acfBuild} height={220} />
        <QChart build={acfAbsBuild} height={220} />
      </div>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr><th>Ljung-Box</th>{#each data.correlogram.ljung_box as lb (lb.lag)}<th class="num">{$t('quant.stats.lagN', { n: lb.lag })}</th>{/each}</tr>
          </thead>
          <tbody>
            <tr>
              <td>{$t('quant.stats.returnsP')}</td>
              {#each data.correlogram.ljung_box as lb (lb.lag)}<td class="num">{fmtP(lb.pvalue)} {stars(lb.pvalue)}</td>{/each}
            </tr>
            <tr>
              <td>{$t('quant.stats.absReturnsP')}</td>
              {#each data.correlogram_abs.ljung_box as lb (lb.lag)}<td class="num">{fmtP(lb.pvalue)} {stars(lb.pvalue)}</td>{/each}
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <section class="qp-block">
      <h3>{$t('quant.stats.memory')}</h3>
      <p class="qp-hint">{$t('quant.stats.memoryHint')}</p>
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.stats.hurstRs')}</span><span class="v">{fmtX(data.hurst.rs, 3)}</span><span class="s">{hurstVerdict(data.hurst.rs)}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.stats.hurstDfa')}</span><span class="v">{fmtX(data.hurst.dfa, 3)}</span><span class="s">{hurstVerdict(data.hurst.dfa)}</span></div>
        <div class="qp-card">
          <span class="k">{$t('quant.stats.halfLife')}</span>
          <!-- A half-life longer than the sample is no reversion this history can show. -->
          <span class="v">{data.half_life?.half_life != null && data.half_life.half_life <= m.n ? fmtX(data.half_life.half_life, 1) : '∞'}</span>
          <span class="s">{$t('quant.stats.halfLifeHint')}</span>
        </div>
      </div>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr><th>{$t('quant.stats.horizonQ')}</th><th class="num">{$t('quant.stats.varianceRatio')}</th><th class="num">z</th><th class="num">p</th><th>{$t('quant.stats.reading')}</th></tr>
          </thead>
          <tbody>
            {#each data.variance_ratio as v (v.lags)}
              <tr>
                <td>{v.lags}</td>
                <td class="num">{fmtX(v.vr, 3)}</td>
                <td class="num">{fmtX(v.stat)}</td>
                <td class="num">{fmtP(v.pvalue)} {stars(v.pvalue)}</td>
                <td class:qp-muted={v.pvalue >= 0.05}>
                  {v.pvalue >= 0.05 ? $t('quant.stats.randomWalk') : v.vr < 1 ? $t('quant.stats.meanReverting') : $t('quant.stats.persistent')}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <section class="qp-block">
      <h3>{$t('quant.stats.stationarity')}</h3>
      <p class="qp-hint">{$t('quant.stats.stationarityHint')}</p>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr><th></th><th class="num">ADF</th><th class="num">p</th><th class="num">{$t('quant.stats.lagsUsed')}</th><th class="num">KPSS</th><th class="num">p</th><th>{$t('quant.stats.verdict')}</th></tr>
          </thead>
          <tbody>
            {#each [[$t('quant.stats.logPrice'), data.adf_price, data.kpss_price], [$t('quant.stats.logReturns'), data.adf_returns, data.kpss_returns]] as [label, adf, kpss] (label)}
              <tr>
                <td>{label}</td>
                <td class="num">{fmtX(adf?.stat)}</td>
                <td class="num">{fmtP(adf?.pvalue)}</td>
                <td class="num">{adf?.used_lag ?? '−'}</td>
                <td class="num">{fmtX(kpss?.stat, 3)}</td>
                <td class="num">{kpss ? (kpss.pvalue <= 0.01 ? '≤ 0.01' : kpss.pvalue >= 0.1 ? '≥ 0.10' : fmtP(kpss.pvalue)) : '−'}</td>
                <td>{stationarity(adf, kpss)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    {#if s}
      <section class="qp-block">
        <h3>{$t('quant.stats.sharpe')}</h3>
        <p class="qp-hint">{$t('quant.stats.sharpeHint')}</p>
        <div class="qp-cards">
          <div class="qp-card"><span class="k">{$t('quant.stats.sharpeAnnual')}</span><span class="v">{fmtX(s.sharpe_annual)}</span><span class="s">95% [{fmtX(s.ci_low)}, {fmtX(s.ci_high)}]</span></div>
          <div class="qp-card"><span class="k">t / p</span><span class="v">{fmtX(s.t_stat)}</span><span class="s">p {fmtP(s.p_value)} {stars(s.p_value)}</span></div>
          <div class="qp-card"><span class="k">PSR</span><span class="v">{fmtPc(s.psr, 1)}</span><span class="s">{$t('quant.stats.psrHint', { b: fmtX(s.benchmark) })}</span></div>
          <div class="qp-card">
            <span class="k">{$t('quant.stats.minTrl')}</span>
            <span class="v">{s.min_trl_years != null ? `${fmtX(s.min_trl_years, 1)} ${$t('quant.stats.years')}` : '∞'}</span>
            <span class="s">{$t('quant.stats.minTrlHint', { have: fmtX(s.observations / s.periods_per_year, 1) })}</span>
          </div>
          <div class="qp-card"><span class="k">{$t('quant.stats.sharpeLo')}</span><span class="v">{fmtX(s.sharpe_annual_lo)}</span><span class="s">{$t('quant.stats.sharpeLoHint')}</span></div>
        </div>
      </section>
    {/if}
  {:else if !busy}
    <p class="qp-hint">{$t('quant.stats.pickHint')}</p>
  {/if}
</div>
