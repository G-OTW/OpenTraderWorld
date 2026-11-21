<script>
  // Daily mindset score (0..100) over the chosen window, with the window's mean and its
  // ± 1 std band. Dots outside the band are the days worth opening: green above, red
  // below, the sign always doubled by position against the band. Click a dot to open
  // that day's check-in.
  import { t } from '$lib/i18n';
  import { roundOf } from './scoring.js';

  let { days = [], mean = null, std = 0, from = '', to = '', onpick = null } = $props();

  // Drawn at the container's own pixel width (no viewBox scaling), so text keeps its real
  // size and hairlines stay one pixel.
  let width = $state(0);
  const W = $derived(Math.max(320, Math.round(width || 800)));
  const H = 240;
  const L = 34;
  const R = 16;
  const T = 12;
  const B = 28;
  const plotW = $derived(W - L - R);
  const plotH = H - T - B;
  // Gridlines on the half pixel: a 1px line on a whole coordinate smears over two rows.
  const crisp = (v) => Math.round(v) + 0.5;

  const t0 = $derived(new Date(`${from}T00:00:00Z`).getTime());
  const t1 = $derived(new Date(`${to}T00:00:00Z`).getTime());
  const x = (iso) => {
    const span = Math.max(1, t1 - t0);
    return L + ((new Date(`${iso}T00:00:00Z`).getTime() - t0) / span) * plotW;
  };
  const y = (v) => T + (1 - v / 100) * plotH;

  const points = $derived(
    days.map((d) => ({
      ...d,
      px: x(d.date),
      py: y(d.score),
      dir: mean == null || std === 0 ? 'in' : d.score >= mean + std ? 'high' : d.score <= mean - std ? 'low' : 'in'
    }))
  );
  const line = $derived(points.map((p) => `${p.px},${p.py}`).join(' '));

  // Date ticks on round day steps (1 day to a quarter), about one every 90 px.
  const DAY = 86400000;
  const DAY_STEPS = [1, 2, 3, 7, 14, 30, 61, 91];
  const ticks = $derived.by(() => {
    const want = Math.max(2, Math.floor(plotW / 90));
    const span = Math.max(DAY, t1 - t0);
    const step = (DAY_STEPS.find((d) => span / (d * DAY) <= want) ?? 91) * DAY;
    const out = [];
    for (let v = t0; v <= t1; v += step) out.push(new Date(v).toISOString().slice(0, 10));
    return out;
  });
  const short = (iso) => iso.slice(5);
</script>

<div class="wrap" bind:clientWidth={width}>
<svg viewBox="0 0 {W} {H}" width={W} height={H} role="img" aria-label={$t('mindset.history.scoreChart')}>
  {#each [0, 25, 50, 75, 100] as g (g)}
    <line x1={L} y1={crisp(y(g))} x2={L + plotW} y2={crisp(y(g))} class="grid" />
    <text x={L - 8} y={y(g) + 4} class="axis">{g}</text>
  {/each}
  {#each ticks as tk (tk)}
    <line x1={crisp(x(tk))} y1={T} x2={crisp(x(tk))} y2={T + plotH} class="grid v" />
  {/each}
  {#if mean != null && std > 0}
    <rect x={L} y={y(Math.min(100, mean + std))} width={plotW} height={Math.max(0, y(Math.max(0, mean - std)) - y(Math.min(100, mean + std)))} class="band" />
  {/if}
  {#if mean != null}
    <line x1={L} y1={y(mean)} x2={L + plotW} y2={y(mean)} class="mean" />
    <text x={L + plotW} y={y(mean) - 4} class="axis right">{$t('mindset.history.meanLabel', { value: roundOf(mean) })}</text>
  {/if}
  {#if points.length > 1}
    <polyline points={line} fill="none" stroke="var(--chart-1)" stroke-width="1.5" stroke-linejoin="round" />
  {/if}
  {#each points as p (p.date)}
    <!-- svelte-ignore a11y_no_static_element_interactions a11y_no_noninteractive_tabindex -->
    <g class="dot {p.dir}" class:clickable={!!onpick} role="button" tabindex="0" onclick={() => onpick?.(p.date)} onkeydown={(e) => e.key === 'Enter' && onpick?.(p.date)}>
      <circle cx={p.px} cy={p.py} r="9" class="hit" />
      <circle cx={p.px} cy={p.py} r="4" class="mark" />
      <title>{p.date}: {roundOf(p.score)}</title>
    </g>
  {/each}
  {#each ticks as tk (tk)}
    {#if x(tk) > L + 16 && x(tk) < L + plotW - 16}
      <text x={x(tk)} y={H - 9} class="axis x" text-anchor="middle">{short(tk)}</text>
    {/if}
  {/each}
</svg>
</div>

<style>
  .wrap {
    min-width: 0;
  }
  svg {
    width: 100%;
    display: block;
  }
  .grid {
    stroke: var(--grid-line);
    stroke-width: 1;
    shape-rendering: crispEdges;
  }
  .grid.v {
    opacity: 0.6;
  }
  .band {
    fill: color-mix(in srgb, var(--chart-1) 10%, transparent);
  }
  .mean {
    stroke: var(--dim);
    stroke-width: 1;
    stroke-dasharray: 4 4;
    shape-rendering: crispEdges;
  }
  .axis {
    fill: var(--dim);
    font-family: var(--mono);
    font-size: 11px;
    text-anchor: end;
  }
  .axis.right {
    text-anchor: end;
  }
  .hit {
    fill: transparent;
  }
  .mark {
    fill: var(--bg);
    stroke: var(--chart-1);
    stroke-width: 2;
  }
  .dot.high .mark {
    fill: var(--green);
    stroke: var(--green);
  }
  .dot.low .mark {
    fill: var(--red);
    stroke: var(--red);
  }
  .dot.clickable {
    cursor: pointer;
  }
  .dot.clickable:hover .mark,
  .dot:focus-visible .mark {
    stroke-width: 3;
  }
</style>
