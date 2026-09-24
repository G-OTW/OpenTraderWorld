<script>
  // One inline-SVG line, no chart library, matching ValueChart's conventions.
  //
  // Used for the equity curve and the underwater curve. `baseline` draws a reference line
  // (1.0 for an equity index, 0 for a drawdown) so the eye has something to read the shape
  // against; `fill` shades to it.
  import { fmtDate } from '$lib/format.js';

  let {
    values = [],
    labels = [],
    baseline = null,
    // 'up-down' colors by whether the series ended above or below its baseline; 'loss'
    // is always red (a drawdown curve is never good news).
    tone = 'up-down',
    height = 160,
    format = (v) => v.toFixed(2)
  } = $props();

  const W = 900;
  const L = 8;
  const R = 8;
  const T = 8;
  const B = 4;
  const plotW = W - L - R;
  const plotH = $derived(height - T - B);

  const lo = $derived(Math.min(...values, baseline ?? Infinity));
  const hi = $derived(Math.max(...values, baseline ?? -Infinity));
  // A flat series has no range to divide by; give it a hairline of headroom so the line
  // lands in the middle instead of on an edge.
  const span = $derived(hi - lo || Math.abs(hi) || 1);
  const x = (i) => L + (values.length < 2 ? plotW / 2 : (i / (values.length - 1)) * plotW);
  const y = (v) => T + plotH - ((v - lo) / span) * plotH;

  const path = $derived(
    values.length ? values.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ') : ''
  );
  const area = $derived(
    values.length && baseline != null
      ? `${path} L${x(values.length - 1).toFixed(1)},${y(baseline).toFixed(1)} L${x(0).toFixed(1)},${y(baseline).toFixed(1)} Z`
      : ''
  );
  const positive = $derived(
    tone === 'loss' ? false : values.length > 0 && values[values.length - 1] >= (baseline ?? values[0])
  );
  const stroke = $derived(tone === 'loss' ? 'var(--red)' : positive ? 'var(--green)' : 'var(--red)');

  // Above the baseline is a gain and below it a loss, whatever the curve ends at, so the
  // two halves are drawn as one path clipped twice rather than as one signed color.
  const split = $derived(tone !== 'loss' && baseline != null && values.length > 1);
  const uid = $props.id();
  const baseY = $derived(baseline != null ? y(baseline) : 0);

  // Hover readout: nearest point, printed in the corner rather than in a floating box.
  let hover = $state(null);
  function onmove(e) {
    if (!values.length) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const rel = ((e.clientX - rect.left) / rect.width) * W;
    const i = Math.round(((rel - L) / plotW) * (values.length - 1));
    hover = Math.max(0, Math.min(values.length - 1, i));
  }
  const hoverStroke = $derived(
    hover == null ? stroke : split ? (values[hover] >= baseline ? 'var(--green)' : 'var(--red)') : stroke
  );
</script>

<div class="chart">
  {#if values.length}
    <svg
      viewBox="0 0 {W} {height}"
      preserveAspectRatio="none"
      role="img"
      onmousemove={onmove}
      onmouseleave={() => (hover = null)}
    >
      {#if split}
        <defs>
          <clipPath id="{uid}-up">
            <rect x="0" y={T} width={W} height={Math.max(0, baseY - T)} />
          </clipPath>
          <clipPath id="{uid}-down">
            <rect x="0" y={baseY} width={W} height={Math.max(0, T + plotH - baseY)} />
          </clipPath>
        </defs>
        <line class="base" x1={L} x2={W - R} y1={baseY} y2={baseY} />
        <g clip-path="url(#{uid}-up)">
          <path class="area" d={area} fill="var(--green)" />
          <path class="line" d={path} stroke="var(--green)" />
        </g>
        <g clip-path="url(#{uid}-down)">
          <path class="area" d={area} fill="var(--red)" />
          <path class="line" d={path} stroke="var(--red)" />
        </g>
      {:else}
        {#if baseline != null}
          <line class="base" x1={L} x2={W - R} y1={baseY} y2={baseY} />
          <path class="area" d={area} fill={stroke} />
        {/if}
        <path class="line" d={path} stroke={stroke} />
      {/if}
      {#if hover != null}
        <line class="cross" x1={x(hover)} x2={x(hover)} y1={T} y2={T + plotH} />
        <circle cx={x(hover)} cy={y(values[hover])} r="4" fill={hoverStroke} />
      {/if}
    </svg>
    <div class="readout num">
      {#if hover != null}
        <b>{format(values[hover])}</b>
        {#if labels[hover]}<span>{fmtDate(labels[hover])}</span>{/if}
      {:else}
        <b>{format(values[values.length - 1])}</b>
        {#if labels.length}<span>{fmtDate(labels[labels.length - 1])}</span>{/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .chart {
    position: relative;
  }
  svg {
    width: 100%;
    display: block;
  }
  .line {
    fill: none;
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .area {
    opacity: 0.12;
    stroke: none;
  }
  .base,
  .cross {
    stroke: var(--border);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .base {
    stroke-dasharray: 3 3;
  }
  .readout {
    position: absolute;
    top: 0;
    right: 0;
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    font-size: 12px;
    color: var(--muted);
    pointer-events: none;
  }
  .readout b {
    color: var(--text);
    font-size: 14px;
  }
</style>
