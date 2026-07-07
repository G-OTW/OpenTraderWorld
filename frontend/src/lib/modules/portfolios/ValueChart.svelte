<script>
  // Value-over-time chart of a portfolio's daily valuation snapshots. Inline SVG, no chart lib.
  // Y axis = market value (gridlines + labels), X axis = date, bucketable by day/week/month/year
  // (buckets keep the last snapshot in each period). A mouse crosshair draws both axes to the
  // nearest point and prints the value (right, on the Y line) and date (bottom, on the X line) —
  // no floating tooltip box. Line/fill turn green over the range when value ended up, red when down.
  import { fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  let { snapshots = [], currency = 'USD' } = $props();

  // Plot geometry. viewBox units; SVG scales to container width. Left/bottom gutters hold axis labels.
  const W = 900;
  const H = 300;
  const L = 64; // left gutter (Y labels)
  const R = 12; // right gutter
  const T = 12; // top gutter
  const B = 28; // bottom gutter (X labels)
  const plotW = W - L - R;
  const plotH = H - T - B;

  const GRANS = [
    { key: 'day', labelKey: 'portfolios.valueChart.day' },
    { key: 'week', labelKey: 'portfolios.valueChart.week' },
    { key: 'month', labelKey: 'portfolios.valueChart.month' },
    { key: 'year', labelKey: 'portfolios.valueChart.year' }
  ];
  let gran = $state('day');

  // Period key for a YYYY-MM-DD date string, per granularity. Bucketing keeps one point per period.
  function periodKey(dateStr, g) {
    const d = new Date(dateStr + 'T00:00:00');
    if (g === 'day') return dateStr;
    if (g === 'year') return String(d.getFullYear());
    if (g === 'month') return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
    // week: ISO-ish — Monday-anchored week start date.
    const day = (d.getDay() + 6) % 7;
    const monday = new Date(d);
    monday.setDate(d.getDate() - day);
    return monday.toISOString().slice(0, 10);
  }

  // Snapshots collapsed to one entry per period (the last snapshot in each), oldest→newest.
  let series = $derived.by(() => {
    const byKey = new Map();
    for (const s of snapshots) byKey.set(periodKey(s.snap_date, gran), s); // later wins
    return [...byKey.values()].sort((a, b) => a.snap_date.localeCompare(b.snap_date));
  });

  let up = $derived(
    series.length >= 2 ? series[series.length - 1].market_value >= series[0].market_value : true
  );
  let color = $derived(up ? 'var(--green)' : 'var(--red)');

  let bounds = $derived.by(() => {
    if (series.length < 2) return null;
    const vals = series.map((s) => s.market_value);
    let min = Math.min(...vals);
    let max = Math.max(...vals);
    if (min === max) { min -= 1; max += 1; }
    // pad 6% so the line isn't glued to the frame
    const pad = (max - min) * 0.06;
    return { min: min - pad, max: max + pad };
  });

  let points = $derived.by(() => {
    if (!bounds) return [];
    const span = bounds.max - bounds.min || 1;
    const stepX = series.length > 1 ? plotW / (series.length - 1) : 0;
    return series.map((s, i) => ({
      x: L + i * stepX,
      y: T + (1 - (s.market_value - bounds.min) / span) * plotH,
      snap: s
    }));
  });

  let line = $derived(points.map((p) => `${p.x},${p.y}`).join(' '));
  let area = $derived(
    points.length ? `${L},${T + plotH} ${line} ${L + plotW},${T + plotH}` : ''
  );

  // Y gridlines / labels: 4 evenly spaced ticks across the value range.
  let yTicks = $derived.by(() => {
    if (!bounds) return [];
    const n = 4;
    return Array.from({ length: n + 1 }, (_, i) => {
      const v = bounds.min + ((bounds.max - bounds.min) * i) / n;
      return { v, y: T + (1 - i / n) * plotH };
    });
  });

  // X labels: up to ~6 evenly spaced points, formatted by granularity.
  function fmtDate(dateStr) {
    const d = new Date(dateStr + 'T00:00:00');
    if (gran === 'year') return String(d.getFullYear());
    if (gran === 'month') return d.toLocaleDateString(undefined, { month: 'short', year: '2-digit' });
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
  let xTicks = $derived.by(() => {
    if (points.length < 2) return [];
    const maxLabels = 6;
    const stride = Math.max(1, Math.ceil(points.length / maxLabels));
    const out = [];
    for (let i = 0; i < points.length; i += stride) out.push(points[i]);
    if (out[out.length - 1] !== points[points.length - 1]) out.push(points[points.length - 1]);
    return out;
  });

  // Crosshair: nearest point to the pointer, tracked in viewBox coords.
  let cross = $state(null);
  let svgEl = $state();
  function onMove(e) {
    if (points.length < 2 || !svgEl) return;
    const rect = svgEl.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * W;
    let nearest = points[0];
    for (const p of points) {
      if (Math.abs(p.x - px) < Math.abs(nearest.x - px)) nearest = p;
    }
    cross = nearest;
  }
  function onLeave() {
    cross = null;
  }
</script>

<div class="wrap">
  <div class="head">
    <div class="grans">
      {#each GRANS as g (g.key)}
        <button class:active={gran === g.key} onclick={() => (gran = g.key)}>{$t(g.labelKey)}</button>
      {/each}
    </div>
  </div>

  {#if points.length >= 2}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <svg
      bind:this={svgEl}
      viewBox="0 0 {W} {H}"
      role="img"
      aria-label={$t('portfolios.valueChart.ariaLabel')}
      onmousemove={onMove}
      onmouseleave={onLeave}
    >
      <!-- Y gridlines + labels -->
      {#each yTicks as t (t.y)}
        <line x1={L} y1={t.y} x2={L + plotW} y2={t.y} class="grid" />
        <text x={L - 8} y={t.y + 3} class="axis y">{fmtMoney(t.v, currency, 0)}</text>
      {/each}

      <!-- X labels -->
      {#each xTicks as t (t.snap.snap_date)}
        <text x={t.x} y={H - 8} class="axis x">{fmtDate(t.snap.snap_date)}</text>
      {/each}

      <!-- area + line -->
      <polygon points={area} fill={color} opacity="0.1" />
      <polyline points={line} fill="none" stroke={color} stroke-width="1.75" vector-effect="non-scaling-stroke" />

      <!-- crosshair -->
      {#if cross}
        <line x1={cross.x} y1={T} x2={cross.x} y2={T + plotH} class="cross" />
        <line x1={L} y1={cross.y} x2={L + plotW} y2={cross.y} class="cross" />
        <circle cx={cross.x} cy={cross.y} r="3.5" fill={color} />
        <text x={L + plotW} y={cross.y - 5} class="readout val" text-anchor="end">
          {fmtMoney(cross.snap.market_value, currency)}
        </text>
        <text x={cross.x} y={T + 12} class="readout date" text-anchor="middle">
          {cross.snap.snap_date}
        </text>
      {/if}
    </svg>
  {:else}
    <p class="muted small">{$t('portfolios.valueChart.notEnoughHistory')}</p>
  {/if}
</div>

<style>
  .wrap {
    width: 100%;
  }
  .head {
    display: flex;
    justify-content: flex-end;
    margin-bottom: var(--space-2);
  }
  .grans {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .grans button {
    background: var(--surface-2);
    border: none;
    border-left: 1px solid var(--border);
    color: var(--muted);
    font-size: 0.78rem;
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .grans button:first-child {
    border-left: none;
  }
  .grans button.active {
    color: var(--text);
    background: var(--surface);
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
    font-size: 11px;
  }
  .axis.y {
    text-anchor: end;
  }
  .cross {
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 3 3;
    vector-effect: non-scaling-stroke;
  }
  .readout {
    font-size: 12px;
    font-weight: 600;
  }
  .readout.val {
    fill: var(--text);
  }
  .readout.date {
    fill: var(--muted);
    font-weight: 500;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.82rem;
  }
</style>
