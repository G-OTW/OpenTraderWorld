<script>
  // Change over time (uPlot): one or more lines on one time axis, crosshair and tooltip
  // built in (date, every series' value, change vs its own start), a legend from two
  // series up, a hover marker with a 2px surface ring.
  //
  //   <TimeSeries series={[{ label: 'Net worth', points: [['2026-01-31', 1200], ...] }]}
  //               area format={(v) => fmtMoney(v, ccy)} axisFormat={(v) => fmtCompactValue(v, ccy)} />
  //
  // A point is [ts, value, note?]. `ts` is a Date, epoch seconds or ms, or "YYYY-MM-DD"
  // (local midnight). `value` null is a real gap (a break in the line). `note` truthy
  // marks an estimate: from the last measured point on, that run is drawn dashed (a
  // string note is shown in the tooltip, `true` shows `estimateLabel`).
  //
  // props:
  //   series   [{ key?, label, points, color?, dashed?, tone?: 'sign', area?, width?, step? }]
  //            step: hold each value until the next point (a price that changes on dates).
  //            default colour: --accent for one series, identity order for several.
  //            tone 'sign': green above the baseline (or 0), red below, on stroke and fill.
  //   bands    [{ key?, label, lower: points, upper: points, color? }]  a shaded range
  //            (projection cone p10 to p90); put the p50 in `series` with dashed: true.
  //   baseline number | null  solid reference line (and the fill target for `area`).
  //   area     bool  10% wash under the first series (a series' own `area` wins).
  //   indexed  bool  rebase every series to 100 at its first value (baseline 100).
  //   format(v) tooltip values; axisFormat(v) y ticks (defaults to format).
  //   xFormat(sec) tooltip date; xAxisFormat(sec) x ticks (defaults by span).
  //   change   'pct' | 'abs' | 'none'  change vs the series start, in the tooltip.
  //   height   px (plot + axes, default 240).  legend 'auto' | 'none'.
  //   estimateLabel  legend + tooltip word for dashed estimates ('' hides the entry).
  //   emphasized  $bindable key; a legend click emphasises one series (others fade).
  //   sensitive   mask figures under the privacy screen (default true).
  //   ariaLabel, onHover({ ts, index, values: [{ key, label, value }] } | null)
  import { onMount, onDestroy, untrack } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import { fmtNum, fmtDate, fmtDateTime, fmtSignedPct } from '$lib/format.js';
  import { locale, LOCALES } from '$lib/i18n';
  import Tooltip from './Tooltip.svelte';
  import Legend from './Legend.svelte';
  import { identityColor, fade, MASK, toneOf } from './palette.js';
  import { alignSeries, splitEstimated, indexTo100, niceTicks, extent, changePct } from './scale.js';

  let {
    series = [],
    bands = [],
    baseline = null,
    area = false,
    indexed = false,
    format = (v) => fmtNum(v, 2),
    axisFormat = null,
    xFormat = null,
    xAxisFormat = null,
    change = 'pct',
    height = 240,
    legend = 'auto',
    estimateLabel = '',
    emphasized = $bindable(null),
    sensitive = true,
    ariaLabel = '',
    onHover = null
  } = $props();

  // Concrete colours for the canvas, re-read on a theme flip (the build effect tracks it).
  const c = $derived(chartColors());
  const masked = $derived(sensitive && privacy.hidden);
  const intl = $derived(LOCALES.find((l) => l.code === $locale)?.html ?? $locale);

  const DAY = 86400;

  // Everything uPlot needs, derived from the props: aligned columns plus, per uPlot
  // series index, what it draws (a band edge, a measured run, an estimated run).
  const model = $derived.by(() => {
    const logical = series.map((s, i) => {
      const pts = s.points ?? [];
      return {
        key: s.key ?? s.label ?? String(i),
        label: s.label ?? '',
        points: indexed ? indexTo100(pts) : pts,
        color: s.color ?? (series.length === 1 ? c.accent : identityColor(i, c)),
        dashed: !!s.dashed,
        sign: s.tone === 'sign',
        area: s.area ?? (area && i === 0),
        width: s.width ?? 2,
        step: !!s.step
      };
    });
    const bandList = bands.map((b, i) => ({
      key: b.key ?? b.label ?? `band-${i}`,
      label: b.label ?? '',
      color: b.color ?? logical[0]?.color ?? c.accent,
      upper: b.upper ?? [],
      lower: b.lower ?? []
    }));
    const { xs, ys, notes } = alignSeries([
      ...logical.map((s) => s.points),
      ...bandList.flatMap((b) => [b.upper, b.lower])
    ]);
    const base = baseline ?? (indexed ? 100 : null);
    const cols = [xs];
    const meta = [null];
    const uBands = [];
    bandList.forEach((b, bi) => {
      const off = logical.length + bi * 2;
      b.hi = ys[off];
      b.lo = ys[off + 1];
      cols.push(b.hi);
      meta.push({ kind: 'band', band: b });
      cols.push(b.lo);
      meta.push({ kind: 'band', band: b });
      uBands.push({ series: [cols.length - 2, cols.length - 1], fill: fade(b.color, 0.12) });
    });
    logical.forEach((s, si) => {
      s.raw = ys[si];
      s.notes = notes[si];
      s.startIdx = s.raw.findIndex((v) => Number.isFinite(v));
      s.start = s.startIdx >= 0 ? s.raw[s.startIdx] : null;
      const split = s.dashed ? { any: false } : splitEstimated(s.raw, s.notes);
      s.anyEst = split.any;
      if (split.any) {
        cols.push(split.solid);
        meta.push({ kind: 'line', s, dashed: false });
        cols.push(split.dashed);
        meta.push({ kind: 'line', s, dashed: true });
      } else {
        cols.push(s.raw);
        meta.push({ kind: 'line', s, dashed: s.dashed });
      }
    });
    const ext = extent(cols.slice(1), base != null ? [base] : []);
    return { logical, bandList, xs, cols, meta, uBands, base, ext };
  });

  const span = $derived(model.xs.length > 1 ? model.xs[model.xs.length - 1] - model.xs[0] : 0);

  function defaultAxisX(sec) {
    const d = new Date(sec * 1000);
    if (span > 3 * 365 * DAY) return String(d.getFullYear());
    if (span > 120 * DAY) return d.toLocaleDateString(intl, { month: 'short', year: '2-digit' });
    if (span > 2 * DAY) return d.toLocaleDateString(intl, { month: 'short', day: 'numeric' });
    return d.toLocaleTimeString(intl, { hour: '2-digit', minute: '2-digit' });
  }
  const defaultTipX = (sec) => (span > 2 * DAY ? fmtDate(new Date(sec * 1000)) : fmtDateTime(new Date(sec * 1000)));

  const legendItems = $derived.by(() => {
    const items = [
      ...model.logical.map((s) => ({
        key: s.key,
        label: s.label,
        color: s.sign ? c.muted : s.color,
        kind: 'line',
        dashed: s.dashed
      })),
      ...model.bandList.map((b) => ({ key: b.key, label: b.label, color: fade(b.color, 0.35), kind: 'rect' }))
    ];
    const est = model.logical.find((s) => s.anyEst);
    if (est && estimateLabel) items.push({ key: '__est', label: estimateLabel, color: est.color, kind: 'line', dashed: true });
    return items.filter((it) => it.label);
  });
  const showLegend = $derived(legend !== 'none' && legendItems.length >= 2);

  let wrap = $state(null);
  let el = $state(null);
  let wrapWidth = $state(0);
  let mounted = $state(false);
  let plot = null;
  let ro;
  let tip = $state(null);
  let hoverWasNull = true;

  let measureCtx = null;
  function measure(text, font) {
    if (typeof document === 'undefined') return text.length * 6;
    measureCtx ??= document.createElement('canvas').getContext('2d');
    measureCtx.font = font;
    return measureCtx.measureText(text).width;
  }

  function yDomain(ext) {
    if (!ext) return [0, 1];
    let [lo, hi] = ext;
    if (lo === hi) {
      const p = Math.abs(lo) * 0.05 || 1;
      lo -= p;
      hi += p;
    }
    const pad = (hi - lo) * 0.08;
    return [lo - pad, hi + pad];
  }

  // Two-tone paint split at the baseline: hard stops, not a decorative gradient.
  const signPaint = (pos, neg, base) => (u) => {
    const { top, height: h } = u.bbox;
    if (!(h > 0)) return pos;
    const y = u.valToPos(base, 'y', true);
    const f = Math.max(0, Math.min(1, (y - top) / h));
    const g = u.ctx.createLinearGradient(0, top, 0, top + h);
    g.addColorStop(0, pos);
    g.addColorStop(f, pos);
    g.addColorStop(f, neg);
    g.addColorStop(1, neg);
    return g;
  };

  function hoverAt(u) {
    const m = model;
    const i = u.cursor.idx;
    if (i == null || u.cursor.left == null || u.cursor.left < 0 || !wrap) {
      tip = null;
      if (!hoverWasNull) onHover?.(null);
      hoverWasNull = true;
      return;
    }
    const fmt = (v) => (masked ? MASK : format(v));
    const rows = [];
    const values = [];
    let topY = Infinity;
    let note = '';
    for (const b of m.bandList) {
      const hi = b.hi[i];
      const lo = b.lo[i];
      if (hi == null || lo == null) continue;
      rows.push({ label: b.label, value: `${fmt(lo)} – ${fmt(hi)}`, color: fade(b.color, 0.5) });
      topY = Math.min(topY, u.valToPos(hi, 'y'));
    }
    for (const s of m.logical) {
      const v = s.raw[i];
      if (v == null) continue;
      values.push({ key: s.key, label: s.label, value: v });
      const n = s.notes[i];
      if (n && !note) note = typeof n === 'string' ? n : estimateLabel;
      let ch = null;
      if (change !== 'none' && i !== s.startIdx && s.start != null) {
        ch = change === 'abs' ? v - s.start : changePct(v, s.start);
      }
      let chText = '';
      if (ch != null) {
        chText =
          change === 'abs'
            ? masked
              ? MASK
              : `${ch > 0 ? '+' : ch < 0 ? '−' : ''}${format(Math.abs(ch))}`
            : fmtSignedPct(ch);
      }
      rows.push({
        label: s.label,
        value: fmt(v),
        color: s.sign ? (v >= (m.base ?? 0) ? c.green : c.red) : s.color,
        dashed: s.dashed || !!n,
        change: chText,
        changeTone: toneOf(ch)
      });
      topY = Math.min(topY, u.valToPos(v, 'y'));
    }
    if (!rows.length) {
      tip = null;
      return;
    }
    const o = u.over.getBoundingClientRect();
    const w = wrap.getBoundingClientRect();
    const x = m.xs[i];
    tip = {
      x: o.left - w.left + u.valToPos(x, 'x'),
      y: o.top - w.top + (Number.isFinite(topY) ? topY : u.cursor.top),
      title: (xFormat ?? defaultTipX)(x),
      rows,
      note
    };
    hoverWasNull = false;
    onHover?.({ ts: x, index: i, values });
  }

  function build() {
    plot?.destroy();
    plot = null;
    tip = null;
    if (!el || model.xs.length === 0) return;
    const m = model;
    const axisFont = `10px ${c.mono}`;
    const [ymin, ymax] = yDomain(m.ext);
    const faded = (key) => emphasized != null && emphasized !== '__est' && emphasized !== key;
    const yLabel = (v) => (masked ? MASK : (axisFormat ?? format)(v));

    const uSeries = [{}];
    for (let i = 1; i < m.meta.length; i++) {
      const it = m.meta[i];
      if (it.kind === 'band') {
        uSeries.push({
          label: it.band.label,
          stroke: 'transparent',
          width: 0,
          points: { show: false },
          alpha: faded(it.band.key) ? 0.3 : 1
        });
        continue;
      }
      const s = it.s;
      const base0 = m.base ?? 0;
      const stroke = s.sign ? signPaint(c.green, c.red, base0) : s.color;
      const fill =
        s.area && !it.dashed
          ? s.sign
            ? signPaint(fade(c.green, 0.1), fade(c.red, 0.1), base0)
            : fade(s.color, 0.1)
          : undefined;
      uSeries.push({
        label: s.label,
        stroke,
        width: s.width,
        paths: s.step ? uPlot.paths.stepped({ align: 1 }) : undefined,
        dash: it.dashed ? [5, 4] : undefined,
        fill,
        fillTo: (u) => (m.base != null ? m.base : u.scales.y.min),
        alpha: faded(s.key) ? 0.25 : 1,
        points: { show: m.xs.length === 1, size: 8, width: 2, stroke: c.surface, fill: s.color }
      });
    }

    const opts = {
      width: el.clientWidth || 600,
      height,
      padding: [10, 8, 0, 0],
      legend: { show: false },
      scales: {
        x: { time: true },
        y: { range: () => [ymin, ymax] }
      },
      bands: m.uBands,
      cursor: {
        x: true,
        y: false,
        drag: { x: false, y: false, setScale: false },
        focus: { prox: -1 },
        // The crosshair snaps to the nearest data position: the reader aims at a date.
        move: (u, left, top) => {
          if (left < 0 || !m.xs.length) return [left, top];
          const idx = u.posToIdx(left);
          const x = m.xs[Math.max(0, Math.min(m.xs.length - 1, idx))];
          return [x == null ? left : u.valToPos(x, 'x'), top];
        },
        points: {
          size: (u, i) => (m.meta[i]?.kind === 'line' ? 10 : 0),
          width: (u, i) => (m.meta[i]?.kind === 'line' ? 2 : 0),
          stroke: () => c.surface,
          fill: (u, i) => {
            const it = m.meta[i];
            if (!it?.s) return 'transparent';
            if (!it.s.sign) return it.s.color;
            const v = u.data[i]?.[u.cursor.idx ?? -1];
            return v != null && v < (m.base ?? 0) ? c.red : c.green;
          }
        }
      },
      axes: [
        {
          stroke: c.dim,
          font: axisFont,
          size: 26,
          gap: 6,
          space: 72,
          grid: { show: false },
          ticks: { show: false },
          values: (u, splits) => splits.map((s) => (xAxisFormat ?? defaultAxisX)(s))
        },
        {
          stroke: c.dim,
          font: axisFont,
          gap: 6,
          grid: { stroke: c.gridLine, width: 0.5 },
          ticks: { show: false },
          splits: (u, ai, min, max) =>
            niceTicks(min, max, 4).filter((v) => v >= min - 1e-9 && v <= max + 1e-9),
          values: (u, splits) => splits.map(yLabel),
          size: (u, values) =>
            values?.length ? Math.ceil(Math.max(...values.map((v) => measure(String(v), axisFont)))) + 12 : 40
        }
      ],
      series: uSeries,
      hooks: {
        // The baseline sits under the series, over the grid: a solid hairline.
        drawAxes: [
          (u) => {
            if (m.base == null) return;
            const y = Math.round(u.valToPos(m.base, 'y', true)) + 0.5;
            if (y < u.bbox.top || y > u.bbox.top + u.bbox.height) return;
            const ctx = u.ctx;
            ctx.save();
            ctx.strokeStyle = c.faint;
            ctx.lineWidth = uPlot.pxRatio;
            ctx.beginPath();
            ctx.moveTo(u.bbox.left, y);
            ctx.lineTo(u.bbox.left + u.bbox.width, y);
            ctx.stroke();
            ctx.restore();
          }
        ],
        setCursor: [(u) => hoverAt(u)]
      }
    };
    plot = new uPlot(opts, m.cols, el);
  }

  onMount(() => {
    mounted = true;
    ro = new ResizeObserver(() => {
      if (plot && el) plot.setSize({ width: el.clientWidth, height });
    });
    if (el) ro.observe(el);
  });
  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
    plot = null;
  });

  // Rebuild on data, palette (theme flip), privacy, emphasis, size or formatter change:
  // colours and formatters are baked into the uPlot instance at construction.
  $effect(() => {
    void model;
    void c;
    void masked;
    void emphasized;
    void height;
    void intl;
    void format;
    void axisFormat;
    void xAxisFormat;
    if (!mounted) return;
    untrack(build);
  });
</script>

<div
  class="ts"
  bind:this={wrap}
  bind:clientWidth={wrapWidth}
  style:height="{height}px"
  role="img"
  aria-label={ariaLabel || legendItems.map((it) => it.label).join(', ') || undefined}
>
  <div class="plot" bind:this={el}></div>
  {#if tip}
    <Tooltip x={tip.x} y={tip.y} width={wrapWidth} title={tip.title} rows={tip.rows} note={tip.note} />
  {/if}
</div>
{#if showLegend}
  <div class="ts-legend">
    <Legend
      items={legendItems}
      active={emphasized}
      onToggle={(k) => (emphasized = k === '__est' ? null : k)}
    />
  </div>
{/if}

<style>
  .ts {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  /* Clipped so the canvas never holds a shrinking container open while the resize
     observer catches up. */
  .plot {
    width: 100%;
    min-width: 0;
    overflow: hidden;
  }
  /* Crosshair: one solid hairline that snaps to the nearest date. */
  .ts :global(.u-cursor-x) {
    border-right: 1px solid var(--border-control);
  }
  .ts :global(.u-cursor-y),
  .ts :global(.u-select) {
    display: none;
  }
  .ts-legend {
    margin-top: var(--space-2);
  }
</style>
