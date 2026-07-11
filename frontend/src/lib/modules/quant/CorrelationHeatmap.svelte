<script>
  // Correlation matrix as a heatmap. Diverging color scale: --diverge-neg (−1) → a neutral
  // midpoint → --diverge-pos (+1). Answers "am I diversified or buying the same thing twice?".
  //
  // No number is drawn in the cell. A diverging ramp necessarily passes through a
  // mid-lightness band where neither a dark nor a light ink reaches 4.5:1 (measured: it
  // tops out around 4.2:1 whatever the arm colors), so the value lives in the hover
  // tooltip and the visualMap legend decodes the scale.
  import { onMount, onDestroy } from 'svelte';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import * as echarts from 'echarts';

  let { corr } = $props(); // { labels, matrix }

  let el;
  let chart;

  // Read inside the effect below, so a theme flip repaints the canvas.
  const colors = $derived(chartColors());

  function build(c, p) {
    const data = [];
    for (let i = 0; i < c.labels.length; i++) {
      for (let j = 0; j < c.labels.length; j++) {
        data.push([j, i, +c.matrix[i][j].toFixed(2)]);
      }
    }
    return {
      animation: false,
      tooltip: {
        formatter: (cell) => `${c.labels[cell.value[1]]} ↔ ${c.labels[cell.value[0]]}: ${cell.value[2]}`,
        backgroundColor: p.surface,
        borderColor: p.border,
        textStyle: { color: p.text, fontSize: 11 }
      },
      grid: { left: 90, right: 20, top: 20, bottom: 60 },
      xAxis: { type: 'category', data: c.labels, axisLabel: { color: p.muted, rotate: 30 }, splitArea: { show: true } },
      yAxis: { type: 'category', data: c.labels, axisLabel: { color: p.muted }, splitArea: { show: true } },
      visualMap: {
        min: -1,
        max: 1,
        calculable: true,
        orient: 'horizontal',
        left: 'center',
        bottom: 0,
        inRange: { color: p.diverge },
        textStyle: { color: p.muted }
      },
      series: [
        {
          type: 'heatmap',
          data,
          // A surface-colored hairline between cells: the 2px spacer that keeps two
          // adjacent fills from reading as one block.
          itemStyle: { borderColor: p.surface, borderWidth: 2 },
          emphasis: { itemStyle: { borderColor: p.text, borderWidth: 2 } }
        }
      ]
    };
  }

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  $effect(() => {
    if (chart && corr) chart.setOption(build(corr, colors), true);
  });
</script>

<div class="chart" bind:this={el}></div>

<style>
  .chart {
    width: 100%;
    height: 360px;
  }
</style>
