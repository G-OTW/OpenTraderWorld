<script>
  // Trend of one 1–5 scale prompt across recent check-ins. Inline SVG line + dots on a fixed
  // 1..5 Y axis, oldest→newest. `series` = [{ date, value }].
  import { t } from '$lib/i18n';

  let { series = [], label = '' } = $props();

  const W = 420;
  const H = 110;
  const L = 16;
  const R = 8;
  const T = 10;
  const B = 22;
  const plotW = W - L - R;
  const plotH = H - T - B;

  let points = $derived.by(() => {
    if (series.length === 0) return [];
    const stepX = series.length > 1 ? plotW / (series.length - 1) : 0;
    return series.map((s, i) => ({
      x: L + (series.length > 1 ? i * stepX : plotW / 2),
      y: T + (1 - (s.value - 1) / 4) * plotH,
      ...s
    }));
  });
  let line = $derived(points.map((p) => `${p.x},${p.y}`).join(' '));

  function short(d) {
    return d.slice(5); // MM-DD
  }
</script>

<div class="trend">
  <span class="lbl">{label}</span>
  {#if points.length === 0}
    <p class="muted small">{$t('mindset.trendChart.noData')}</p>
  {:else}
    <svg viewBox="0 0 {W} {H}" role="img" aria-label={$t('mindset.trendChart.trendLabel', { label })}>
      {#each [1, 3, 5] as g (g)}
        <line
          x1={L}
          y1={T + (1 - (g - 1) / 4) * plotH}
          x2={L + plotW}
          y2={T + (1 - (g - 1) / 4) * plotH}
          class="grid"
        />
        <text x={L - 4} y={T + (1 - (g - 1) / 4) * plotH + 3} class="axis">{g}</text>
      {/each}
      {#if points.length > 1}
        <polyline points={line} fill="none" stroke="var(--chart-1)" stroke-width="1.75" vector-effect="non-scaling-stroke" />
      {/if}
      {#each points as p (p.date)}
        <circle cx={p.x} cy={p.y} r="3" fill="var(--chart-1)">
          <title>{p.date}: {p.value}</title>
        </circle>
      {/each}
      <text x={points[0].x} y={H - 6} class="axis x" text-anchor="start">{short(points[0].date)}</text>
      {#if points.length > 1}
        <text x={points[points.length - 1].x} y={H - 6} class="axis x" text-anchor="end">
          {short(points[points.length - 1].date)}
        </text>
      {/if}
    </svg>
  {/if}
</div>

<style>
  .trend {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .lbl {
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
  }
  svg {
    width: 100%;
    height: auto;
    display: block;
  }
  .grid {
    stroke: var(--border);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .axis {
    fill: var(--muted);
    font-size: 9px;
    text-anchor: end;
  }
  .axis.x {
    font-size: 9px;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
</style>
