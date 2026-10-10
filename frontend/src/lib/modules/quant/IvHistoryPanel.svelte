<script>
  // Implied against realized volatility: Interactive Brokers' 30-day implied volatility of an
  // underlying next to the close-to-close volatility before and after each day. The premium
  // implied volatility carries over what followed, where today sits in its own range, and how
  // well implied volatility forecast realized volatility.
  import './panel.css';
  import QChart from './QChart.svelte';
  import MarketSource from './MarketSource.svelte';
  import TaskProgress from './TaskProgress.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, timeAxis, tooltip, title, legend, fmtPc, fmtX } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let source = $state(null);
  let underlying = $state('SPY');
  let assetType = $state('etf');
  let years = $state(3);
  let rvWindow = $state(21);
  let lookback = $state(252);
  let data = $state(null);
  let busy = $state(false);
  let progress = $state(null);
  let error = $state('');

  async function run() {
    if (!source) return;
    busy = true;
    error = '';
    progress = null;
    try {
      data = await quantApi.ivHistory(
        { connector_id: source, underlying: underlying.trim(), asset_type: assetType, years: Number(years), window: Math.round(Number(rvWindow)), lookback: Math.round(Number(lookback)) },
        (p) => (progress = p)
      );
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const h = $derived(data?.result);
  const s = $derived(h?.summary);
  const pctAxis = (p, d = 0) => ({ color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => fmtPc(v, d) });

  const lineBuild = $derived.by(() => {
    if (!h?.rows?.length) return null;
    const names = [$t('quant.ivh.iv'), $t('quant.ivh.rv', { n: s.window }), $t('quant.ivh.rvFwd', { n: s.window })];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.ivh.lineTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 16, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (v == null ? '−' : fmtPc(+v, 2)) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { axisLabel: pctAxis(p) }),
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: h.rows.map((r) => [r.date, r.iv]), lineStyle: { color: p.series[0], width: 1.4 }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', symbol: 'none', data: h.rows.map((r) => [r.date, r.rv]), lineStyle: { color: p.muted, width: 1 }, itemStyle: { color: p.muted } },
        { name: names[2], type: 'line', symbol: 'none', data: h.rows.map((r) => [r.date, r.rv_forward]), lineStyle: { color: p.amber, width: 1 }, itemStyle: { color: p.amber } }
      ]
    });
  });

  const vrpBuild = $derived.by(() => {
    const rows = h?.rows?.filter((r) => r.rv_forward != null);
    if (!rows?.length) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.ivh.vrpTitle')),
      grid: { left: 48, right: 16, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 2) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { axisLabel: pctAxis(p) }),
      series: [
        {
          type: 'bar',
          barCategoryGap: 0,
          data: rows.map((r) => ({ value: [r.date, r.iv - r.rv_forward], itemStyle: { color: r.iv >= r.rv_forward ? p.green : p.red } }))
        }
      ]
    });
  });

  const fitBuild = $derived.by(() => {
    const rows = h?.rows?.filter((r) => r.rv_forward != null);
    if (!rows?.length || s.fwd_beta == null) return null;
    const xs = rows.map((r) => r.iv);
    const lo = Math.min(...xs);
    const hi = Math.max(...xs);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.ivh.fitTitle')),
      grid: { left: 48, right: 16, top: 32, bottom: 36 },
      tooltip: tooltip(p, { trigger: 'item', formatter: (q) => (q.value ? `IV ${fmtPc(q.value[0], 1)} · RV ${fmtPc(q.value[1], 1)}` : '') }),
      xAxis: valueAxis(p, { name: $t('quant.ivh.iv'), nameLocation: 'middle', nameGap: 22, scale: true, axisLabel: pctAxis(p) }),
      yAxis: valueAxis(p, { name: $t('quant.ivh.rvFwdShort'), scale: true, axisLabel: pctAxis(p) }),
      series: [
        { type: 'scatter', symbolSize: 3, itemStyle: { color: p.series[0], opacity: 0.45 }, data: rows.map((r) => [r.iv, r.rv_forward]) },
        { type: 'line', symbol: 'none', silent: true, lineStyle: { color: p.amber, width: 1.5 }, data: [[lo, s.fwd_alpha + s.fwd_beta * lo], [hi, s.fwd_alpha + s.fwd_beta * hi]] },
        { type: 'line', symbol: 'none', silent: true, lineStyle: { color: p.muted, width: 1, type: 'dashed' }, data: [[lo, lo], [hi, hi]] }
      ]
    });
  });

  const rankBuild = $derived.by(() => {
    const rows = h?.rows?.filter((r) => r.rank != null);
    if (!rows?.length) return null;
    const names = [$t('quant.ivh.rank'), $t('quant.ivh.percentile')];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.ivh.rankTitle', { n: s.lookback })),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 16, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 0) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { min: 0, max: 1, axisLabel: pctAxis(p) }),
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: rows.map((r) => [r.date, r.rank]), lineStyle: { color: p.series[0], width: 1.2 }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', symbol: 'none', data: rows.map((r) => [r.date, r.percentile]), lineStyle: { color: p.series[1], width: 1 }, itemStyle: { color: p.series[1] } }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <MarketSource bind:value={source} providers={['ibkr']} />
    <label class="qp-field"><span class="qp-label">{$t('quant.surf.underlying')}</span><input type="text" bind:value={underlying} placeholder="SPY" /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.fut.spotType')}</span>
      <div class="qp-seg">
        {#each ['etf', 'equity', 'index'] as k (k)}
          <button class:active={assetType === k} onclick={() => (assetType = k)}>{$t(`quant.market.type.${k}`)}</button>
        {/each}
      </div>
    </div>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.fut.years')}</span>
      <div class="qp-seg">
        {#each [1, 2, 3, 5, 10] as y (y)}
          <button class:active={years === y} onclick={() => (years = y)}>{y}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.ivh.window')}</span><input type="number" min="5" max="126" step="1" bind:value={rvWindow} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.ivh.lookback')}</span><input type="number" min="20" max="756" step="1" bind:value={lookback} /></label>
    <button class="primary" onclick={run} disabled={!source || busy || !underlying.trim()}>{busy ? $t('quant.page.computing') : $t('quant.page.compute')}</button>
  </div>

  {#if busy}
    <TaskProgress {progress} />
  {/if}
  <ErrorText {error} />

  {#if h}
    <p class="qp-hint">{$t('quant.ivh.summary', { u: data.underlying, n: s.observations, from: h.rows[0].date, to: h.rows.at(-1).date })}</p>
    <section class="qp-block">
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.ivh.iv')}</span><span class="v">{fmtPc(s.current_iv, 2)}</span><span class="s">{$t('quant.ivh.mean', { v: fmtPc(s.mean_iv, 1) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.ivh.rv', { n: s.window })}</span><span class="v">{fmtPc(s.current_rv, 2)}</span><span class="s">{$t('quant.ivh.ibHv', { v: fmtPc(s.current_hv, 2) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.ivh.rank')}</span><span class="v">{fmtPc(s.rank, 0)}</span><span class="s">{$t('quant.ivh.percentileOf', { v: fmtPc(s.percentile, 0), n: s.lookback })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.ivh.vrp')}</span><span class="v" class:qp-good={s.mean_vrp > 0} class:qp-bad={s.mean_vrp < 0}>{fmtPc(s.mean_vrp, 2)}</span><span class="s">{$t('quant.ivh.above', { v: fmtPc(s.iv_above_share, 0) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.ivh.forecast')}</span><span class="v">β {fmtX(s.fwd_beta)}</span><span class="s">R² {fmtNum(s.fwd_r2, 2)} · α {fmtPc(s.fwd_alpha, 1)} · {$t('quant.ivh.pairs', { n: s.forecast_pairs })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.ivh.leverage')}</span><span class="v">{fmtNum(s.iv_return_corr, 2)}</span><span class="s">{$t('quant.ivh.leverageHint')}</span></div>
      </div>
      <p class="qp-hint">{$t('quant.ivh.hint')}</p>
    </section>
    <section class="qp-block"><QChart build={lineBuild} height={300} /></section>
    <div class="qp-grid2">
      <section class="qp-block"><QChart build={vrpBuild} height={260} /><p class="qp-hint">{$t('quant.ivh.vrpHint')}</p></section>
      <section class="qp-block"><QChart build={fitBuild} height={260} /><p class="qp-hint">{$t('quant.ivh.fitHint')}</p></section>
    </div>
    <section class="qp-block"><QChart build={rankBuild} height={240} /><p class="qp-hint">{$t('quant.ivh.rankHint')}</p></section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.ivh.pickHint')}</p>
  {/if}
</div>
