<script>
  // Compare tab: several saved backtests on their common dates. Rebased curves, how alike
  // their returns are, the deflated Sharpe of the best one (the others count as the trials it
  // was picked from), and the probability that picking the best in-sample overfits (PBO).
  import './panel.css';
  import QChart from './QChart.svelte';
  import DatasetChecklist from './DatasetChecklist.svelte';
  import CorrelationHeatmap from './CorrelationHeatmap.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, categoryAxis, timeAxis, tooltip, title, legend, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let runs = $state([]);
  let ids = $state([]);
  let partitions = $state(16);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    quantApi
      .backtestRuns()
      .then((r) => (runs = r))
      .catch((e) => (error = e.message));
  });

  async function run() {
    if (ids.length < 2) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.compare(ids, partitions);
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const c = $derived(data?.result);
  const bestIdx = $derived.by(() => {
    if (!c) return -1;
    let b = -1;
    c.summaries.forEach((s, i) => {
      if (s.sharpe != null && (b < 0 || s.sharpe > c.summaries[b].sharpe)) b = i;
    });
    return b;
  });

  const curveBuild = $derived.by(() => {
    if (!c) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.compare.curvesTitle')),
      legend: legend(p, { type: 'scroll', left: 140 }),
      grid: { left: 48, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (+v).toFixed(3) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { scale: true }),
      series: c.labels.map((l, i) => ({
        name: `#${i + 1} ${l}`,
        type: 'line',
        symbol: 'none',
        data: c.ts.map((ts, j) => [ts, c.curves[i][j]]),
        lineStyle: { width: i === bestIdx ? 2 : 1.2, color: p.series[i % p.series.length] },
        itemStyle: { color: p.series[i % p.series.length] }
      }))
    });
  });

  const logitBuild = $derived.by(() => {
    const h = c?.pbo?.logits;
    if (!h) return null;
    const mids = h.counts.map((_, i) => (h.edges[i] + h.edges[i + 1]) / 2);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.compare.logitTitle')),
      grid: { left: 40, right: 12, top: 32, bottom: 28 },
      tooltip: tooltip(p),
      xAxis: categoryAxis(p, mids.map((m) => m.toFixed(2))),
      yAxis: valueAxis(p),
      series: [{ type: 'bar', data: h.counts.map((v, i) => ({ value: v, itemStyle: { color: mids[i] <= 0 ? p.red : p.green } })), barCategoryGap: '8%' }]
    });
  });

  const degradeBuild = $derived.by(() => {
    const b = c?.pbo;
    if (!b?.pairs?.length) return null;
    const xs = b.pairs.map((q) => q[0]);
    const lo = Math.min(...xs);
    const hi = Math.max(...xs);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.compare.degradeTitle')),
      grid: { left: 44, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { trigger: 'item', formatter: (q) => (q.value ? `IS ${(+q.value[0]).toFixed(2)} · OOS ${(+q.value[1]).toFixed(2)}` : '') }),
      xAxis: valueAxis(p, { name: $t('quant.compare.isSharpe'), nameLocation: 'middle', nameGap: 22, scale: true }),
      yAxis: valueAxis(p, { name: $t('quant.compare.oosSharpe'), scale: true }),
      series: [
        { type: 'scatter', symbolSize: 4, itemStyle: { color: p.series[0], opacity: 0.5 }, data: b.pairs },
        { type: 'line', symbol: 'none', silent: true, lineStyle: { color: p.amber, width: 1.5 }, data: [[lo, b.intercept + b.slope * lo], [hi, b.intercept + b.slope * hi]] }
      ]
    });
  });

  const runLabel = (r) => `${r.name} · ${r.ticker} ${r.timeframe}`;
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field grow">
      <span class="qp-label">{$t('quant.compare.runs', { n: ids.length })}</span>
      <DatasetChecklist datasets={runs} bind:value={ids} max={20} labelOf={runLabel} subOf={(r) => `${fmtNum(r.stats?.trades ?? 0, 0)} ${$t('quant.mc.trades')}`} />
    </div>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.compare.partitions')}</span>
      <div class="qp-seg">
        {#each [8, 10, 12, 16] as s (s)}
          <button class:active={partitions === s} onclick={() => (partitions = s)}>{s}</button>
        {/each}
      </div>
    </div>
    <button class="primary" onclick={run} disabled={ids.length < 2 || busy}>
      {busy ? $t('quant.page.computing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if c}
    <p class="qp-hint">{$t('quant.compare.summary', { n: c.labels.length, p: c.observations, from: c.first_ts.slice(0, 10), to: c.last_ts.slice(0, 10) })}</p>
    <section class="qp-block">
      <QChart build={curveBuild} height={320} />
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr>
              <th>#</th><th>{$t('quant.compare.run')}</th><th class="num">{$t('quant.compare.total')}</th><th class="num">{$t('quant.compare.annReturn')}</th>
              <th class="num">{$t('quant.compare.annVol')}</th><th class="num">Sharpe</th><th class="num">{$t('quant.basket.maxDd')}</th>
            </tr>
          </thead>
          <tbody>
            {#each c.summaries as s, i (s.label)}
              <tr class:best={i === bestIdx}>
                <td class="qp-muted">{i + 1}</td>
                <td>{s.label}</td>
                <td class="num" class:qp-good={s.total_return > 0} class:qp-bad={s.total_return < 0}>{fmtPc(s.total_return, 1)}</td>
                <td class="num">{fmtPc(s.ann_return, 1)}</td>
                <td class="num">{fmtPc(s.ann_vol, 1)}</td>
                <td class="num">{fmtX(s.sharpe)}</td>
                <td class="num qp-bad">−{fmtPc(s.max_drawdown, 1)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.compare.correlation')}</h3>
        <p class="qp-hint">{$t('quant.compare.correlationHint')}</p>
        <CorrelationHeatmap corr={{ labels: c.labels.map((_, i) => `#${i + 1}`), matrix: c.correlation.matrix }} />
      </section>
      <section class="qp-block">
        <h3>{$t('quant.compare.selection')}</h3>
        <p class="qp-hint">{$t('quant.compare.selectionHint')}</p>
        {#if c.deflated}
          <div class="qp-cards">
            <div class="qp-card"><span class="k">{$t('quant.compare.bestSharpe')}</span><span class="v">{fmtX(c.deflated.best_sharpe)}</span><span class="s">{c.summaries[bestIdx]?.label}</span></div>
            <div class="qp-card"><span class="k">{$t('quant.compare.bar')}</span><span class="v">{fmtX(c.deflated.expected_max_sharpe)}</span><span class="s">{$t('quant.compare.barHint', { n: c.deflated.trials })}</span></div>
            <div class="qp-card"><span class="k">PSR</span><span class="v">{fmtPc(c.deflated.probabilistic_sharpe, 1)}</span><span class="s">{$t('quant.compare.psrHint')}</span></div>
            <div class="qp-card"><span class="k">DSR</span><span class="v" class:qp-good={c.deflated.deflated_sharpe >= 0.95} class:qp-bad={c.deflated.deflated_sharpe < 0.5}>{fmtPc(c.deflated.deflated_sharpe, 1)}</span><span class="s">{$t('quant.compare.dsrHint')}</span></div>
          </div>
        {/if}
        {#if c.pbo}
          <div class="qp-cards">
            <div class="qp-card"><span class="k">PBO</span><span class="v" class:qp-good={c.pbo.pbo < 0.2} class:qp-warn={c.pbo.pbo >= 0.2 && c.pbo.pbo < 0.5} class:qp-bad={c.pbo.pbo >= 0.5}>{fmtPc(c.pbo.pbo, 1)}</span><span class="s">{$t('quant.compare.pboHint')}</span></div>
            <div class="qp-card"><span class="k">{$t('quant.compare.oosLoss')}</span><span class="v">{fmtPc(c.pbo.prob_oos_loss, 1)}</span><span class="s">{$t('quant.compare.oosLossHint')}</span></div>
            <div class="qp-card"><span class="k">{$t('quant.compare.slope')}</span><span class="v">{fmtX(c.pbo.slope)}</span><span class="s">{$t('quant.compare.slopeHint')}</span></div>
            <div class="qp-card"><span class="k">{$t('quant.compare.splits')}</span><span class="v">{fmtNum(c.pbo.splits, 0)}</span><span class="s">{$t('quant.compare.splitsHint', { s: c.pbo.partitions, n: c.pbo.observations })}</span></div>
          </div>
        {:else}
          <p class="qp-hint">{$t('quant.compare.noPbo')}</p>
        {/if}
      </section>
    </div>

    {#if c.pbo}
      <div class="qp-grid2">
        <section class="qp-block"><QChart build={logitBuild} height={240} /><p class="qp-hint">{$t('quant.compare.logitHint')}</p></section>
        <section class="qp-block"><QChart build={degradeBuild} height={240} /><p class="qp-hint">{$t('quant.compare.degradeHint')}</p></section>
      </div>
    {/if}
  {:else if !busy}
    <p class="qp-hint">{$t('quant.compare.pickHint')}</p>
  {/if}
</div>

<style>
  .grow {
    flex: 1;
    min-width: 0;
    flex-basis: 320px;
    max-width: 640px;
  }
  tr.best td:nth-child(2) {
    font-weight: var(--fw-medium);
  }

</style>
