<script>
  // Two pictures of the same trade list: how the results are spread (histogram of per-trade
  // returns, losers red / winners green, with the two averages marked) and how many landed
  // on each side (donut). Both are read-only derivations — no state of their own.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { t } from '$lib/i18n';
  import { returnBuckets } from './trades.js';

  let { trades = [], s = {} } = $props();

  const dist = $derived(returnBuckets(trades));

  let histEl;
  let donutEl;
  let hist;
  let donut;

  function cssVar(name, fb) {
    if (typeof window === 'undefined') return fb;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fb;
  }
  const palette = () => ({
    green: cssVar('--green', '#15866a'),
    red: cssVar('--red', '#c94b54'),
    amber: cssVar('--amber', '#b87516'),
    dim: cssVar('--dim', '#7b838d'),
    line: cssVar('--grid-line', '#e3e0d9'),
    surface: cssVar('--surface', '#fdfdfe'),
    text: cssVar('--text', '#17191c'),
    mono: cssVar('--mono', 'monospace')
  });

  onMount(() => {
    hist = echarts.init(histEl, null, { renderer: 'canvas' });
    donut = echarts.init(donutEl, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => {
      hist?.resize();
      donut?.resize();
    });
    ro.observe(histEl);
    ro.observe(donutEl);
    return () => ro.disconnect();
  });
  onDestroy(() => {
    hist?.dispose();
    donut?.dispose();
  });

  const pct = (v) => `${v >= 0 ? '+' : ''}${v.toFixed(2)}%`;

  function histOption(d) {
    const p = palette();
    const labels = d.buckets.map((b) => `${b.from.toFixed(Math.abs(d.step) < 1 ? 1 : 0)}%`);
    const mark = (v, color, label) =>
      v == null
        ? null
        : {
            // Bucket index the average falls in, kept fractional so the line sits where the
            // value really is instead of snapping to a bar.
            xAxis: (v - d.buckets[0].from) / d.step - 0.5,
            lineStyle: { color, type: 'dashed', width: 1 },
            label: { show: false },
            name: label
          };
    const marks = [mark(d.avgLossPct, p.red), mark(d.avgWinPct, p.green)].filter(Boolean);
    const bar = (name, data, color) => ({
      name,
      type: 'bar',
      stack: 'trades',
      data,
      barMaxWidth: 26,
      itemStyle: { color, borderRadius: [2, 2, 0, 0] }
    });
    return {
      animation: false,
      backgroundColor: 'transparent',
      grid: { left: 34, right: 8, top: 12, bottom: 22 },
      tooltip: {
        trigger: 'axis',
        axisPointer: { type: 'shadow' },
        backgroundColor: p.surface,
        borderColor: p.line,
        textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 }
      },
      xAxis: {
        type: 'category',
        data: labels,
        axisLine: { lineStyle: { color: p.line } },
        axisTick: { show: false },
        axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, hideOverlap: true }
      },
      yAxis: {
        type: 'value',
        minInterval: 1,
        axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
        splitLine: { lineStyle: { color: p.line, opacity: 0.5 } }
      },
      series: [
        {
          ...bar($t('analysis.losers'), d.buckets.map((b) => b.losers || null), p.red),
          markLine: { silent: true, symbol: 'none', data: marks }
        },
        bar($t('analysis.winners'), d.buckets.map((b) => b.winners || null), p.green)
      ]
    };
  }

  function donutOption(st) {
    const p = palette();
    const total = st.trades ?? 0;
    return {
      animation: false,
      backgroundColor: 'transparent',
      tooltip: {
        trigger: 'item',
        backgroundColor: p.surface,
        borderColor: p.line,
        textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 }
      },
      series: [
        {
          type: 'pie',
          radius: ['62%', '86%'],
          center: ['50%', '50%'],
          padAngle: 1,
          label: {
            show: true,
            position: 'center',
            formatter: () => `{n|${total}}\n{l|${$t('analysis.totalTrades')}}`,
            rich: {
              n: { color: p.text, fontFamily: p.mono, fontSize: 20, fontWeight: 500 },
              l: { color: p.dim, fontSize: 10, padding: [4, 0, 0, 0] }
            }
          },
          labelLine: { show: false },
          data: [
            { value: st.wins ?? 0, name: $t('analysis.winners'), itemStyle: { color: p.green } },
            { value: st.losses ?? 0, name: $t('analysis.losers'), itemStyle: { color: p.red } },
            { value: st.breakevens ?? 0, name: $t('analysis.breakevens'), itemStyle: { color: p.amber } }
          ].filter((x) => x.value > 0)
        }
      ]
    };
  }

  $effect(() => {
    const d = dist;
    if (hist) hist.setOption(histOption(d), { replaceMerge: ['series'] });
  });
  $effect(() => {
    const st = s;
    if (donut) donut.setOption(donutOption(st), { replaceMerge: ['series'] });
  });

  const legend = $derived([
    { label: $t('analysis.winners'), color: 'var(--green)', n: s.wins ?? 0 },
    { label: $t('analysis.losers'), color: 'var(--red)', n: s.losses ?? 0 },
    { label: $t('analysis.breakevens'), color: 'var(--amber)', n: s.breakevens ?? 0 }
  ]);
</script>

<div class="dist">
  <section>
    <h4>{$t('analysis.returnsDist')}</h4>
    <div class="ec" bind:this={histEl}></div>
    <div class="foot">
      <span><i class="sq" style:background="var(--red)"></i>{$t('analysis.losers')}</span>
      <span><i class="sq" style:background="var(--green)"></i>{$t('analysis.winners')}</span>
      <span class="avg red">{$t('analysis.avgLoss')} <b>{dist.avgLossPct == null ? '—' : pct(dist.avgLossPct)}</b></span>
      <span class="avg green">{$t('analysis.avgWin')} <b>{dist.avgWinPct == null ? '—' : pct(dist.avgWinPct)}</b></span>
    </div>
  </section>

  <section>
    <h4>{$t('analysis.tradesDist')}</h4>
    <div class="donut-row">
      <div class="ec donut" bind:this={donutEl}></div>
      <ul class="keys">
        {#each legend as k (k.label)}
          <li>
            <i class="sq" style:background={k.color}></i>
            <span class="k">{k.label}</span>
            <b>{k.n}</b>
            <span class="p">{s.trades ? ((k.n / s.trades) * 100).toFixed(2) : '0.00'}%</span>
          </li>
        {/each}
      </ul>
    </div>
  </section>
</div>

<style>
  .dist {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-4);
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .ec {
    height: 190px;
    width: 100%;
  }
  .donut-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  .donut {
    flex: 0 0 190px;
  }
  .keys {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 0;
  }
  .keys li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .keys .k {
    min-width: 74px;
  }
  .keys b {
    font-family: var(--mono);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .keys .p {
    font-family: var(--mono);
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .foot b {
    font-family: var(--mono);
    color: var(--text);
  }
  .avg.red {
    border-left: 2px dashed var(--red);
    padding-left: var(--space-1);
  }
  .avg.green {
    border-left: 2px dashed var(--green);
    padding-left: var(--space-1);
  }
  .sq {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 4px;
  }
</style>
