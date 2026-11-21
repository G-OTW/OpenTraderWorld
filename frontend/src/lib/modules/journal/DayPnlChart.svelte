<script>
  // Cumulative realized P&L across one trading day: a step line from flat, one step per
  // closed position at its exit time. The area above zero is tinted green and below red,
  // and each step carries a dot in its trade's own colour, so the sign is always doubled by
  // position or glyph. Inline SVG on the theme tokens, like the mindset charts.
  import { t } from '$lib/i18n';

  // `fmt` labels a dot's tooltip (full precision); `axis` labels the Y ticks (compact).
  let { curve = { steps: [], low: 0, high: 0 }, fmt = (n) => String(n), axis = fmt } = $props();

  // Drawn at the container's own pixel width (no viewBox scaling), so text stays at its
  // real size and hairlines stay one pixel.
  let width = $state(0);
  const W = $derived(Math.max(320, Math.round(width || 800)));
  const H = 260;
  const L = 84;
  const R = 16;
  const T = 12;
  const B = 28;
  const plotW = $derived(W - L - R);
  const plotH = H - T - B;

  const steps = $derived(curve.steps ?? []);
  // A lone trade still needs a span: half an hour either side.
  const t0 = $derived(steps.length ? steps[0].at - (steps.length === 1 ? 1800000 : 0) : 0);
  const t1 = $derived(steps.length ? steps[steps.length - 1].at + (steps.length === 1 ? 1800000 : 0) : 1);
  // Room before the first step so the opening move reads as a step from flat.
  const lead = $derived(Math.max(60000, (t1 - t0) * 0.04));
  const xMin = $derived(t0 - lead);
  const xMax = $derived(t1 + lead);
  // Round ticks (1, 2, 2.5 or 5 times a power of ten) spanning the curve and zero, so the
  // axis reads in whole steps instead of the day's exact low and high.
  function niceStep(span, n) {
    const raw = span / n;
    const p = 10 ** Math.floor(Math.log10(raw));
    const m = raw / p;
    return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 2.5 ? 2.5 : m <= 5 ? 5 : 10) * p;
  }
  const scale = $derived.by(() => {
    const lo = Math.min(0, curve.low ?? 0);
    const hi = Math.max(0, curve.high ?? 0);
    // About one gridline every 36 px of plot height.
    const step = niceStep(hi - lo || 1, Math.max(3, Math.floor(plotH / 36)));
    const from = Math.floor(lo / step) * step;
    const to = Math.ceil(hi / step) * step;
    const ticks = [];
    for (let v = from; v <= to + step / 2; v += step) ticks.push(Math.abs(v) < step / 1e6 ? 0 : v);
    return { lo: from, hi: to, ticks };
  });

  const x = (ms) => L + ((ms - xMin) / Math.max(1, xMax - xMin)) * plotW;
  const y = (v) => T + (1 - (v - scale.lo) / (scale.hi - scale.lo || 1)) * plotH;
  // Gridlines on the half pixel: a 1px line on a whole coordinate smears over two rows.
  const crisp = (v) => Math.round(v) + 0.5;

  const path = $derived.by(() => {
    if (!steps.length) return '';
    let d = `M${x(xMin)},${y(0)} H${x(steps[0].at)}`;
    for (const s of steps) d += ` V${y(s.cum)} H${x(s.at)}`;
    // Hold the close of the day to the right edge.
    d += ` H${x(xMax)}`;
    return d;
  });
  // Same outline closed on the zero line: clipped above for the gain tint, below for the loss.
  const area = $derived(path ? `${path} V${y(0)} Z` : '');
  const zero = $derived(y(0));

  const hhmm = (ms) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  // Time ticks on round clock times (5 min to 4 h), about one every 110 px.
  const TIME_STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 180, 240, 360].map((m) => m * 60000);
  const ticks = $derived.by(() => {
    const want = Math.max(2, Math.floor(plotW / 110));
    const span = xMax - xMin;
    const step = TIME_STEPS.find((s) => span / s <= want) ?? TIME_STEPS[TIME_STEPS.length - 1];
    // Align on local clock time, not on the epoch.
    const tz = new Date(xMin).getTimezoneOffset() * 60000;
    const out = [];
    for (let v = Math.ceil((xMin - tz) / step) * step + tz; v <= xMax; v += step) out.push(v);
    return out;
  });
  const uid = `dpc${Math.random().toString(36).slice(2, 8)}`;

  // ── Hover: the step nearest the pointer, read anywhere over the plot ──
  let svgEl = $state(null);
  let hi = $state(-1);
  const hov = $derived(hi >= 0 && hi < steps.length ? steps[hi] : null);
  const hx = $derived(hov ? x(hov.at) : 0);
  const hy = $derived(hov ? y(hov.cum) : 0);
  // The card sits beside the point, on the side with room, vertically centred and kept
  // inside the frame.
  const tipStyle = $derived.by(() => {
    if (!hov) return '';
    const left = (hx / W) * 100;
    const top = Math.min(78, Math.max(22, (hy / H) * 100));
    const side = hx > W * 0.55 ? 'translate(calc(-100% - 14px), -50%)' : 'translate(14px, -50%)';
    return `left:${left}%;top:${top}%;transform:${side};`;
  });

  function onMove(e) {
    if (!svgEl || !steps.length) return;
    const r = svgEl.getBoundingClientRect();
    const vx = ((e.clientX - r.left) / r.width) * W;
    let best = 0;
    let dist = Infinity;
    for (const [i, s] of steps.entries()) {
      const d = Math.abs(x(s.at) - vx);
      if (d < dist) {
        dist = d;
        best = i;
      }
    }
    hi = best;
  }
  const hhmmss = (ms) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
</script>

<div class="wrap" bind:clientWidth={width}>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<svg
  bind:this={svgEl}
  viewBox="0 0 {W} {H}"
  width={W}
  height={H}
  role="img"
  aria-label={$t('journal.day.chart.aria')}
  onpointermove={onMove}
  onpointerleave={() => (hi = -1)}
>
  <defs>
    <clipPath id="{uid}-up"><rect x={L} y={0} width={plotW} height={zero} /></clipPath>
    <clipPath id="{uid}-dn"><rect x={L} y={zero} width={plotW} height={Math.max(0, H - zero)} /></clipPath>
  </defs>

  {#each scale.ticks as g (g)}
    {#if g !== 0}
      <line x1={L} y1={crisp(y(g))} x2={L + plotW} y2={crisp(y(g))} class="grid" />
    {/if}
    <text x={L - 8} y={y(g) + 4} class="axis">{g === 0 ? '0' : axis(g)}</text>
  {/each}
  {#each ticks as tk (tk)}
    <line x1={crisp(x(tk))} y1={T} x2={crisp(x(tk))} y2={T + plotH} class="grid v" />
  {/each}
  <line x1={L} y1={crisp(zero)} x2={L + plotW} y2={crisp(zero)} class="zero" />

  {#if area}
    <path d={area} class="fill up" clip-path="url(#{uid}-up)" />
    <path d={area} class="fill dn" clip-path="url(#{uid}-dn)" />
    <path d={path} class="line" />
  {/if}

  {#if hov}
    <!-- Crosshair to both axes: the time below, the running P&L on the left. -->
    <line x1={hx} y1={T} x2={hx} y2={T + plotH} class="cross" />
    <line x1={L} y1={hy} x2={hx} y2={hy} class="cross" />
  {/if}

  {#each steps as s, i (s.id)}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <g
      class="dot"
      class:pos={s.net > 0}
      class:neg={s.net < 0}
      class:on={i === hi}
      tabindex="0"
      role="img"
      aria-label="{hhmm(s.at)} {s.ticker} {fmt(s.net)} {fmt(s.cum)}"
      onfocus={() => (hi = i)}
      onblur={() => (hi = -1)}
    >
      <circle cx={x(s.at)} cy={y(s.cum)} r="9" class="halo" />
      <circle cx={x(s.at)} cy={y(s.cum)} r="4" class="mark" />
    </g>
  {/each}

  {#each ticks as tk (tk)}
    {#if x(tk) > L + 18 && x(tk) < L + plotW - 18}
      <text x={x(tk)} y={H - 9} class="axis x" class:faded={!!hov} text-anchor="middle">{hhmm(tk)}</text>
    {/if}
  {/each}

  {#if hov}
    <!-- Axis read-outs: pills over the tick labels, in the line's colour. -->
    <g class="pill">
      <rect x={Math.min(Math.max(hx - 30, L), L + plotW - 60)} y={H - 22} width="60" height="18" rx="2" />
      <text x={Math.min(Math.max(hx, L + 30), L + plotW - 30)} y={H - 9} text-anchor="middle">{hhmm(hov.at)}</text>
    </g>
    <g class="pill">
      <rect x={2} y={hy - 9} width={L - 6} height="18" rx="2" />
      <text x={L - 8} y={hy + 4} text-anchor="end">{axis(hov.cum)}</text>
    </g>
  {/if}
</svg>

{#if hov}
  <div class="tip" style={tipStyle} role="tooltip">
    <div class="tip-head">
      <span class="tip-tk">{hov.ticker}</span>
      {#if hov.side}<span class="tip-side {hov.side}">{$t(`journal.side.${hov.side}`)}</span>{/if}
      <span class="tip-time">{hhmmss(hov.at)}</span>
    </div>
    <div class="tip-row">
      <span>{$t('journal.day.chart.trade')}</span>
      <b class:pos={hov.net > 0} class:neg={hov.net < 0}>{fmt(hov.net)}</b>
    </div>
    <div class="tip-row">
      <span>{$t('journal.day.chart.cum')}</span>
      <b class:pos={hov.cum > 0} class:neg={hov.cum < 0}>{fmt(hov.cum)}</b>
    </div>
    <div class="tip-foot">{$t('journal.day.chart.nth', { n: hi + 1, total: steps.length })}</div>
  </div>
{/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  svg {
    width: 100%;
    display: block;
    cursor: crosshair;
    touch-action: pan-y;
  }
  .cross {
    stroke: var(--dim);
    stroke-width: 1;
    stroke-dasharray: 3 3;
    vector-effect: non-scaling-stroke;
    pointer-events: none;
  }
  .halo {
    fill: transparent;
    transition: fill var(--dur-fast) var(--ease);
  }
  .dot.on .halo {
    fill: color-mix(in srgb, var(--chart-1) 22%, transparent);
  }
  .dot.pos.on .halo {
    fill: color-mix(in srgb, var(--green) 25%, transparent);
  }
  .dot.neg.on .halo {
    fill: color-mix(in srgb, var(--red) 25%, transparent);
  }
  .dot:focus {
    outline: none;
  }
  .axis.faded {
    opacity: 0.35;
  }
  .pill rect {
    fill: var(--chart-1);
  }
  .pill text {
    fill: var(--bg);
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  /* Hover card: a small sheet beside the point, never under the pointer. */
  .tip {
    position: absolute;
    z-index: 3;
    min-width: 11.5rem;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    box-shadow: var(--shadow-2);
    pointer-events: none;
    font-size: var(--text-sm);
  }
  .tip-head {
    display: flex;
    align-items: baseline;
    gap: 6px;
    padding-bottom: 4px;
    margin-bottom: 2px;
    border-bottom: 0.5px solid var(--border);
  }
  .tip-tk {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .tip-side {
    font-size: var(--text-xs);
  }
  .tip-side.long {
    color: var(--green);
  }
  .tip-side.short {
    color: var(--red);
  }
  .tip-time {
    margin-left: auto;
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }
  .tip-row {
    display: flex;
    justify-content: space-between;
    gap: var(--space-4);
    color: var(--muted);
  }
  .tip-row b {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .tip-row b.pos {
    color: var(--green);
  }
  .tip-row b.neg {
    color: var(--red);
  }
  .tip-foot {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .grid {
    stroke: var(--grid-line);
    stroke-width: 1;
    shape-rendering: crispEdges;
  }
  .grid.v {
    opacity: 0.6;
  }
  .zero {
    stroke: var(--dim);
    stroke-width: 1;
    shape-rendering: crispEdges;
  }
  .fill.up {
    fill: color-mix(in srgb, var(--green) 14%, transparent);
  }
  .fill.dn {
    fill: color-mix(in srgb, var(--red) 14%, transparent);
  }
  .line {
    fill: none;
    stroke: var(--chart-1);
    stroke-width: 1.5;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  .axis {
    fill: var(--dim);
    font-family: var(--mono);
    font-size: 11px;
    text-anchor: end;
    font-variant-numeric: tabular-nums;
  }
  .mark {
    fill: var(--bg);
    stroke: var(--chart-1);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .dot.pos .mark {
    fill: var(--green);
    stroke: var(--green);
  }
  .dot.neg .mark {
    fill: var(--red);
    stroke: var(--red);
  }
  .dot.on .mark {
    stroke-width: 3;
  }
</style>
