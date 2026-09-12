<script>
  // The full presentation of a value that evolves over time: headline figure, its change
  // over the visible window, a range rail, and an area curve with a hairline grid.
  //
  // This is the shape every "evolution" card shares (portfolio value, net worth, equity
  // curve, a quote series), so the card only supplies the series and the formatters —
  // the window arithmetic, the axes and the change figure live here once.
  //
  // Geometry is measured in real pixels rather than a stretched viewBox: axis labels have
  // to stay upright and the last-point dot has to stay round whatever cell the widget
  // lands in.
  import { tip } from '$lib/ui/tip.svelte.js';
  import { fmtNum, fmtSignedPct } from '$lib/format';
  import { t, locale } from '$lib/i18n';

  let {
    points = [],           // [{ at, value }] oldest→newest. `at` may be an ISO date, epoch
                           // seconds/ms or a Date; without it the range rail is dropped.
    label = 'trend',
    valueLabel = '',       // eyebrow over the headline figure
    format = (v) => fmtNum(v, 2),        // headline + tooltip
    axisFormat = null,     // y ticks; falls back to `format`
    changeFormat = null,   // the signed absolute change; falls back to a signed `format`
    ranges = ['1w', '1m', '3m', '1y', 'all'],
    defaultRange = 'all',
    tone = 'auto',         // 'auto' (last vs first) | 'pos' | 'neg' | 'accent'
    showValue = true,      // false when the card prints its own headline above the chart
    showPill = true
  } = $props();

  const uid = $props.id();

  // ── series ────────────────────────────────────────────────────────────────
  // A bare `YYYY-MM-DD` is read as local midnight: parsed as UTC it slips back a day
  // west of Greenwich and lands the point in the wrong bucket.
  function toMs(at) {
    if (at == null) return null;
    if (at instanceof Date) return at.getTime();
    if (typeof at === 'number') return at < 1e11 ? at * 1000 : at;
    const s = String(at);
    const d = new Date(/^\d{4}-\d{2}-\d{2}$/.test(s) ? `${s}T00:00:00` : s);
    return Number.isNaN(d.getTime()) ? null : d.getTime();
  }

  const rows = $derived(
    points
      .map((p) => ({ t: toMs(p.at), v: Number(p.value ?? p.v), text: p.label ?? '' }))
      .filter((r) => Number.isFinite(r.v))
  );
  const dated = $derived(rows.length > 0 && rows.every((r) => r.t != null));

  // ── range rail ────────────────────────────────────────────────────────────
  const SPAN_DAYS = { '1d': 1, '1w': 7, '1m': 30, '3m': 91, '6m': 182, '1y': 365, '2y': 730 };
  const DAY = 86400000;

  function windowOf(key) {
    if (key === 'all' || !dated) return rows;
    const days = SPAN_DAYS[key];
    if (!days) return rows;
    const from = rows.at(-1).t - days * DAY;
    return rows.filter((r) => r.t >= from);
  }

  // A range only earns a chip when it draws a line (two points) and actually crops the
  // series — a "1Y" that selects everything is a second ALL button.
  const rail = $derived.by(() => {
    if (!dated || rows.length < 2) return [];
    const out = [];
    for (const key of ranges) {
      if (key === 'all') continue;
      const n = windowOf(key).length;
      if (n >= 2 && n < rows.length) out.push(key);
    }
    if (out.length === 0) return [];
    if (ranges.includes('all')) out.push('all');
    return out;
  });

  let picked = $state(null);
  const range = $derived(
    picked && rail.includes(picked) ? picked : rail.includes(defaultRange) ? defaultRange : 'all'
  );
  const series = $derived(rail.length ? windowOf(range) : rows);

  const rangeLabel = (key) => (key === 'all' ? $t('dashboard.widgets.trend.all') : key.toUpperCase());

  // ── figures ───────────────────────────────────────────────────────────────
  const last = $derived(series.at(-1)?.v ?? null);
  const first = $derived(series[0]?.v ?? null);
  const delta = $derived(series.length > 1 ? last - first : null);
  const pct = $derived(delta != null && first ? (delta / Math.abs(first)) * 100 : null);
  const dir = $derived(tone !== 'auto' ? tone : (delta ?? 0) < 0 ? 'neg' : 'pos');
  const signed = $derived(changeFormat ?? ((v) => (v >= 0 ? `+${format(v)}` : format(v))));
  const yFormat = $derived(axisFormat ?? format);

  // ── geometry ──────────────────────────────────────────────────────────────
  let w = $state(0);
  let h = $state(0);
  const showY = $derived(w >= 210);
  const showX = $derived(h >= 78);
  const L = $derived(showY ? 46 : 2);
  const R = 8;
  const T = 8;
  const B = $derived(showX ? 17 : 2);
  const plotW = $derived(Math.max(0, w - L - R));
  const plotH = $derived(Math.max(0, h - T - B));
  const drawable = $derived(series.length > 1 && plotW > 8 && plotH > 8);

  const bounds = $derived.by(() => {
    const vals = series.map((r) => r.v);
    let min = Math.min(...vals);
    let max = Math.max(...vals);
    if (min === max) {
      min -= Math.abs(min) * 0.02 || 1;
      max += Math.abs(max) * 0.02 || 1;
    }
    const pad = (max - min) * 0.08;
    return { min: min - pad, max: max + pad };
  });

  const xAt = (i) => L + (series.length > 1 ? (i / (series.length - 1)) * plotW : plotW / 2);
  const yAt = (v) => T + (1 - (v - bounds.min) / (bounds.max - bounds.min)) * plotH;

  const path = $derived(
    drawable ? series.map((r, i) => `${i ? 'L' : 'M'}${xAt(i).toFixed(1)} ${yAt(r.v).toFixed(1)}`).join(' ') : ''
  );
  const area = $derived(
    path ? `${path} L${xAt(series.length - 1).toFixed(1)} ${T + plotH} L${L} ${T + plotH} Z` : ''
  );

  // Grid: enough lines to read a level off, never enough to compete with the curve.
  const yTicks = $derived.by(() => {
    if (!drawable) return [];
    const n = Math.max(2, Math.min(5, Math.round(plotH / 46)));
    return Array.from({ length: n + 1 }, (_, i) => {
      const v = bounds.min + ((bounds.max - bounds.min) * i) / n;
      return { v, y: yAt(v) };
    });
  });

  // X labels are dated when the series is, and fall back to the caller's own labels
  // (a year, a period name) when it isn't.
  const spanDays = $derived(dated && series.length > 1 ? (series.at(-1).t - series[0].t) / DAY : 0);
  function xLabel(r) {
    if (!dated) return r.text;
    const d = new Date(r.t);
    if (spanDays <= 2) return d.toLocaleTimeString($locale, { hour: '2-digit', minute: '2-digit' });
    if (spanDays <= 75) return d.toLocaleDateString($locale, { month: 'short', day: 'numeric' });
    if (spanDays <= 800) return d.toLocaleDateString($locale, { month: 'short', year: '2-digit' });
    return String(d.getFullYear());
  }
  function pointTitle(r) {
    if (!dated) return r.text || label;
    return new Date(r.t).toLocaleDateString($locale, { day: 'numeric', month: 'short', year: 'numeric' });
  }
  const xTicks = $derived.by(() => {
    if (!drawable || !showX) return [];
    const n = Math.max(2, Math.min(6, Math.floor(plotW / 78)));
    const step = (series.length - 1) / (n - 1);
    const seen = new Set();
    const out = [];
    for (let k = 0; k < n; k++) {
      const i = Math.round(k * step);
      if (seen.has(i)) continue;
      seen.add(i);
      const text = xLabel(series[i]);
      if (text) out.push({ i, x: xAt(i), text });
    }
    return out;
  });

  // ── hover ─────────────────────────────────────────────────────────────────
  let hover = $state(-1);
  const cursor = $derived(hover >= 0 && hover < series.length ? series[hover] : null);

  function at(event) {
    if (!drawable) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (event.clientX - rect.left - L) / (plotW || 1)));
    hover = Math.round(frac * (series.length - 1));
    tip.show(event, {
      title: pointTitle(series[hover]),
      rows: [{ label, value: format(series[hover].v) }]
    });
  }
  function leave() {
    hover = -1;
    tip.hide();
  }
</script>

<div class="trend {dir}">
  {#if showValue || rail.length}
    <div class="head">
      {#if showValue}
        <div class="figure">
          {#if valueLabel}<span class="w-eyebrow">{valueLabel}</span>{/if}
          <span class="w-metric-value">{last == null ? '—' : format(last)}</span>
          {#if delta != null}
            <span class="chg">
              {signed(delta)}{#if pct != null}<span class="pctpart"> ({fmtSignedPct(pct)})</span>{/if}
            </span>
          {/if}
        </div>
      {/if}
      {#if rail.length}
        <div class="w-chips ranges" role="group" aria-label={$t('dashboard.widgets.trend.range')}>
          {#each rail as key (key)}
            <button
              type="button"
              class="w-chip"
              aria-pressed={range === key}
              onclick={() => (picked = key)}>{rangeLabel(key)}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <div class="plot w-fill" bind:clientWidth={w} bind:clientHeight={h}>
    {#if drawable}
      <svg
        width={w}
        height={h}
        role="img"
        aria-label={label}
        onpointermove={at}
        onpointerleave={leave}
      >
        <defs>
          <linearGradient id="{uid}-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" class="g0" />
            <stop offset="100%" class="g1" />
          </linearGradient>
        </defs>

        <!-- Grid is scaffolding: dotted hairlines under the data, never competing. -->
        {#each yTicks as tick (tick.y)}
          <line class="grid" x1={L} y1={tick.y.toFixed(1)} x2={L + plotW} y2={tick.y.toFixed(1)} />
          {#if showY}
            <text class="axis y" x={L - 8} y={tick.y + 3}>{yFormat(tick.v)}</text>
          {/if}
        {/each}
        {#each xTicks as tick (tick.i)}
          <line class="grid vert" x1={tick.x.toFixed(1)} y1={T} x2={tick.x.toFixed(1)} y2={T + plotH} />
          <text class="axis x" x={tick.x} y={h - 4}>{tick.text}</text>
        {/each}

        <path d={area} fill="url(#{uid}-fill)" stroke="none" />
        <path
          d={path}
          class="curve"
          fill="none"
          stroke="currentColor"
          stroke-width="1.75"
          stroke-linejoin="round"
          stroke-linecap="round"
        />

        {#if cursor}
          <line class="cross" x1={xAt(hover)} y1={T} x2={xAt(hover)} y2={T + plotH} />
          <circle class="pin" cx={xAt(hover)} cy={yAt(cursor.v)} r="3.5" />
        {:else}
          <circle class="pin" cx={xAt(series.length - 1)} cy={yAt(last)} r="3.5" />
        {/if}
      </svg>
      {#if showPill && pct != null}
        <span class="w-pill mark" class:pos={dir === 'pos'} class:neg={dir === 'neg'}>
          {fmtSignedPct(pct)}
        </span>
      {/if}
    {/if}
  </div>
</div>

<style>
  .trend {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 100%;
    min-height: 0;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .accent {
    color: var(--accent);
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    flex-shrink: 0;
  }
  .figure {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  /* The change reads in the series' own colour: it is the one figure on the card whose
     sign is the message. */
  .chg {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: currentColor;
    white-space: nowrap;
  }
  .pctpart {
    opacity: 0.85;
  }
  .ranges {
    margin-top: 1px;
  }
  .ranges .w-chip {
    height: 22px;
    padding: 0 8px;
    font-family: var(--mono);
    font-size: 11px;
    letter-spacing: 0.01em;
  }
  .plot {
    position: relative;
    min-height: 48px;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke: var(--grid-line);
    stroke-width: var(--hairline);
    stroke-dasharray: 2 4;
  }
  /* Vertical rules sit a rank below the horizontal ones: they locate a date, they are
     not levels to read a value against. */
  .grid.vert {
    opacity: 0.55;
  }
  .axis {
    fill: var(--faint);
    font-family: var(--mono);
    font-size: 10px;
  }
  .axis.y {
    text-anchor: end;
  }
  .axis.x {
    text-anchor: middle;
  }
  .g0 {
    stop-color: currentColor;
    stop-opacity: 0.2;
  }
  .g1 {
    stop-color: currentColor;
    stop-opacity: 0;
  }
  .cross {
    stroke: var(--border-strong);
    stroke-width: var(--hairline);
  }
  .pin {
    fill: currentColor;
    stroke: var(--surface);
    stroke-width: 2;
  }
  /* Anchored to the plot's top-right: the window's result, beside the end of the line. */
  .mark {
    position: absolute;
    top: 0;
    right: 0;
    pointer-events: none;
  }
</style>
