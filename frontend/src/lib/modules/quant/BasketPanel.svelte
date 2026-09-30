<script>
  // Basket tab: how many independent bets the basket really holds (PCA), which members move
  // together (dendrogram), a hierarchical risk parity allocation, a relative-strength league
  // table, and how a weighting would have come through past crises.
  import './panel.css';
  import QChart from './QChart.svelte';
  import DatasetChecklist from './DatasetChecklist.svelte';
  import WeightsBars from './WeightsBars.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, categoryAxis, tooltip, title, legend, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasets } = $props();

  let ids = $state([]);
  let linkage = $state('single');
  let weightMode = $state('equal'); // 'equal' | 'hrp' | 'custom'
  let custom = $state({}); // id → weight (percent)
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  const selected = $derived(ids.map((id) => datasets.find((d) => d.id === id)).filter(Boolean));

  async function run() {
    if (ids.length < 2) return;
    busy = true;
    error = '';
    try {
      let weights = null;
      if (weightMode === 'custom') weights = ids.map((id) => Number(custom[id] ?? 0));
      let res = await quantApi.basket(ids, { linkage, weights });
      // Stress the HRP allocation itself: it is only known after a first pass.
      if (weightMode === 'hrp') res = await quantApi.basket(ids, { linkage, weights: res.hrp.weights });
      data = res;
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const pcaBuild = $derived.by(() => {
    const c = data?.pca?.components;
    if (!c) return null;
    const x = c.map((_, i) => `PC${i + 1}`);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.basket.pcaTitle')),
      legend: legend(p, { data: [$t('quant.basket.explained'), $t('quant.basket.cumulative')] }),
      grid: { left: 44, right: 12, top: 32, bottom: 24 },
      tooltip: tooltip(p, { valueFormatter: (v) => `${v}%` }),
      xAxis: categoryAxis(p, x),
      yAxis: valueAxis(p, { max: 100, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' } }),
      series: [
        { name: $t('quant.basket.explained'), type: 'bar', data: c.map((q) => +(q.explained * 100).toFixed(2)), itemStyle: { color: p.series[0] } },
        { name: $t('quant.basket.cumulative'), type: 'line', data: c.map((q) => +(q.cumulative * 100).toFixed(2)), itemStyle: { color: p.amber }, lineStyle: { color: p.amber } }
      ]
    });
  });

  // Dendrogram drawn from the merges: leaves spaced along x in tree order, each merge a
  // bracket at its joining distance.
  const dendroBuild = $derived.by(() => {
    const c = data?.clustering;
    if (!c?.merges?.length) return null;
    const n = c.labels.length;
    const pos = {};
    c.order.forEach((leaf, i) => (pos[leaf] = { x: i, y: 0 }));
    const segs = [];
    c.merges.forEach((m, i) => {
      const a = pos[m.a];
      const b = pos[m.b];
      segs.push({ coords: [[a.x, a.y], [a.x, m.distance], [b.x, m.distance], [b.x, b.y]] });
      pos[n + i] = { x: (a.x + b.x) / 2, y: m.distance };
    });
    const names = c.order.map((i) => c.labels[i]);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.basket.dendroTitle')),
      grid: { left: 44, right: 12, top: 32, bottom: 40 },
      xAxis: valueAxis(p, {
        min: -0.5,
        max: n - 0.5,
        interval: 1,
        splitLine: { show: false },
        axisLabel: { color: p.text, fontSize: 10, rotate: n > 8 ? 45 : 0, formatter: (v) => names[Math.round(v)] ?? '' }
      }),
      yAxis: valueAxis(p),
      series: [{ type: 'lines', coordinateSystem: 'cartesian2d', polyline: true, data: segs, lineStyle: { color: p.series[0], width: 1.5 }, silent: true }]
    });
  });

  const pcaTop = $derived(data?.pca?.components?.slice(0, 3) ?? []);
  const cell = (v) => `background: color-mix(in srgb, var(${v >= 0 ? '--green' : '--red'}) ${Math.round(Math.min(1, Math.abs(v)) * 45)}%, transparent)`;
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field">
      <span class="qp-label">{$t('quant.basket.assets', { n: ids.length })}</span>
      <DatasetChecklist {datasets} bind:value={ids} />
    </div>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.basket.linkage')}</span>
      <div class="qp-seg">
        {#each ['single', 'average', 'complete', 'ward'] as m (m)}
          <button class:active={linkage === m} onclick={() => (linkage = m)}>{$t(`quant.basket.link.${m}`)}</button>
        {/each}
      </div>
      <span class="qp-label">{$t('quant.basket.stressWeights')}</span>
      <div class="qp-seg">
        {#each ['equal', 'hrp', 'custom'] as m (m)}
          <button class:active={weightMode === m} onclick={() => (weightMode = m)}>{$t(`quant.basket.weights.${m}`)}</button>
        {/each}
      </div>
      {#if weightMode === 'custom'}
        <div class="custom">
          {#each selected as d (d.id)}
            <label><span>{d.ticker}</span><input type="number" min="0" step="1" bind:value={custom[d.id]} placeholder="%" /></label>
          {/each}
        </div>
      {/if}
    </div>
    <button class="primary" onclick={run} disabled={ids.length < 2 || busy}>
      {busy ? $t('quant.page.computing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if data}
    <p class="qp-hint">{$t('quant.basket.summary', { n: data.labels.length, p: data.periods, tf: data.measured_at })}</p>

    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.basket.pca')}</h3>
        <p class="qp-hint">{$t('quant.basket.pcaHint')}</p>
        <QChart build={pcaBuild} height={220} />
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.basket.loadings')}</th>{#each pcaTop as _, i (i)}<th class="num">PC{i + 1}</th>{/each}</tr></thead>
            <tbody>
              {#each data.pca.labels as l, r (l)}
                <tr><td>{l}</td>{#each pcaTop as c, i (i)}<td class="num" style={cell(c.loadings[r])}>{fmtX(c.loadings[r], 2)}</td>{/each}</tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
      <section class="qp-block">
        <h3>{$t('quant.basket.clusters')}</h3>
        <p class="qp-hint">{$t('quant.basket.clustersHint')}</p>
        <QChart build={dendroBuild} height={300} />
      </section>
    </div>

    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.basket.hrp')}</h3>
        <p class="qp-hint">{$t('quant.basket.hrpHint', { v: fmtPc(data.hrp.vol, 1) })}</p>
        <WeightsBars labels={data.hrp.labels} weights={data.hrp.weights} title={$t('quant.basket.hrpWeights')} />
      </section>
      <section class="qp-block">
        <h3>{$t('quant.basket.strength')}</h3>
        <p class="qp-hint">{$t('quant.basket.strengthHint')}</p>
        <div class="qp-scroll">
          <table class="qp-table">
            <thead>
              <tr>
                <th>#</th><th></th>
                {#each data.strength.lookbacks as lb, i (lb)}<th class="num">{['1M', '3M', '6M', '12M'][i]}</th>{/each}
                <th class="num">12-1</th><th class="num">{$t('quant.basket.volShort')}</th><th class="num">{$t('quant.basket.retPerVol')}</th>
              </tr>
            </thead>
            <tbody>
              {#each data.strength.rows as r (r.label)}
                <tr>
                  <td>{r.rank}</td>
                  <td>{r.label}</td>
                  {#each r.returns as v, i (i)}<td class="num" class:qp-good={v > 0} class:qp-bad={v < 0}>{fmtPc(v, 1)}</td>{/each}
                  <td class="num">{fmtPc(r.momentum_12_1, 1)}</td>
                  <td class="num">{fmtPc(r.vol, 1)}</td>
                  <td class="num">{fmtX(r.risk_adjusted)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    </div>

    <section class="qp-block">
      <h3>{$t('quant.basket.stress')}</h3>
      <p class="qp-hint">
        {$t('quant.basket.stressHint')}
        {data.stress.labels.map((l, i) => `${l} ${fmtPc(data.stress.weights[i], 0)}`).join(' · ')}
      </p>
      {#if data.stress.windows.length}
        <div class="qp-scroll">
          <table class="qp-table">
            <thead>
              <tr>
                <th>{$t('quant.basket.crisis')}</th><th>{$t('quant.basket.window')}</th>
                <th class="num">{$t('quant.basket.portfolio')}</th><th class="num">{$t('quant.basket.maxDd')}</th>
                {#each data.stress.labels as l (l)}<th class="num">{l}</th>{/each}
              </tr>
            </thead>
            <tbody>
              {#each data.stress.windows as w (w.key)}
                <tr>
                  <td>{$t(`quant.basket.crises.${w.key}`)}</td>
                  <td class="qp-muted">{w.from.slice(0, 10)} → {w.to.slice(0, 10)}</td>
                  <td class="num" class:qp-good={w.ret > 0} class:qp-bad={w.ret < 0}>{fmtPc(w.ret, 1)}</td>
                  <td class="num qp-bad">−{fmtPc(w.max_drawdown, 1)}</td>
                  {#each w.asset_returns as v, i (i)}<td class="num" class:qp-good={v > 0} class:qp-bad={v < 0}>{fmtPc(v, 1)}</td>{/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="qp-hint">{$t('quant.basket.noCrisis')}</p>
      {/if}
      <p class="qp-hint">
        {$t('quant.basket.worst')}
        {data.stress.worst_periods.slice(0, 5).map(([ts, r]) => `${ts.slice(0, 10)} ${fmtPc(r, 1)}`).join(' · ')}
      </p>
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.basket.pickHint')}</p>
  {/if}
</div>

<style>
  .custom {
    display: grid;
    grid-template-columns: repeat(2, auto);
    gap: var(--space-1) var(--space-3);
  }
  .custom label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .custom input {
    width: 70px;
  }
</style>
