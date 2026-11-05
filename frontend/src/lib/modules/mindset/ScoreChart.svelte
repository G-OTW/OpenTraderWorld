<script>
  // Daily mindset score (0..100) over the chosen window, with the window's mean and its
  // ± 1 std band. Dots outside the band are the days worth opening: green above, red
  // below, the sign always doubled by position against the band. Click a dot to open
  // that day's check-in.
  import { t } from '$lib/i18n';
  import { roundOf } from './scoring.js';

  let { days = [], mean = null, std = 0, from = '', to = '', onpick = null } = $props();

  const W = 800;
  const H = 220;
  const L = 34;
  const R = 12;
  const T = 12;
  const B = 26;
  const plotW = W - L - R;
  const plotH = H - T - B;

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

  // Four date ticks across the axis, whatever the window length.
  const ticks = $derived.by(() => {
    const out = [];
    for (let i = 0; i <= 3; i++) {
      const tms = t0 + ((t1 - t0) * i) / 3;
      out.push(new Date(tms).toISOString().slice(0, 10));
    }
    return out;
  });
  const short = (iso) => iso.slice(5);
</script>

<svg viewBox="0 0 {W} {H}" role="img" aria-label={$t('mindset.history.scoreChart')}>
  {#each [0, 50, 100] as g (g)}
    <line x1={L} y1={y(g)} x2={L + plotW} y2={y(g)} class="grid" />
    <text x={L - 6} y={y(g) + 3} class="axis">{g}</text>
  {/each}
  {#if mean != null && std > 0}
    <rect x={L} y={y(Math.min(100, mean + std))} width={plotW} height={Math.max(0, y(Math.max(0, mean - std)) - y(Math.min(100, mean + std)))} class="band" />
  {/if}
  {#if mean != null}
    <line x1={L} y1={y(mean)} x2={L + plotW} y2={y(mean)} class="mean" />
    <text x={L + plotW} y={y(mean) - 4} class="axis right">{$t('mindset.history.meanLabel', { value: roundOf(mean) })}</text>
  {/if}
  {#if points.length > 1}
    <polyline points={line} fill="none" stroke="var(--chart-1)" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
  {/if}
  {#each points as p (p.date)}
    <!-- svelte-ignore a11y_no_static_element_interactions a11y_no_noninteractive_tabindex -->
    <g class="dot {p.dir}" class:clickable={!!onpick} role="button" tabindex="0" onclick={() => onpick?.(p.date)} onkeydown={(e) => e.key === 'Enter' && onpick?.(p.date)}>
      <circle cx={p.px} cy={p.py} r="9" class="hit" />
      <circle cx={p.px} cy={p.py} r="4" class="mark" vector-effect="non-scaling-stroke" />
      <title>{p.date}: {roundOf(p.score)}</title>
    </g>
  {/each}
  {#each ticks as tk, i (tk)}
    <text x={x(tk)} y={H - 8} class="axis x" text-anchor={i === 0 ? 'start' : i === 3 ? 'end' : 'middle'}>{short(tk)}</text>
  {/each}
</svg>

<style>
  svg {
    width: 100%;
    height: auto;
    display: block;
  }
  .grid {
    stroke: var(--grid-line);
    stroke-width: 0.5;
    vector-effect: non-scaling-stroke;
  }
  .band {
    fill: color-mix(in srgb, var(--chart-1) 10%, transparent);
  }
  .mean {
    stroke: var(--dim);
    stroke-width: 1;
    stroke-dasharray: 4 4;
    vector-effect: non-scaling-stroke;
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
  @media (max-width: 480px) {
    .axis {
      font-size: 14px;
    }
  }
</style>
