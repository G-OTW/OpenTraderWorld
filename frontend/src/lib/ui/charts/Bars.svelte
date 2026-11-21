<script>
  // Bars (inline SVG, horizontal as aligned HTML rows): a value per category, stacked
  // when several series are given (or grouped with stacked={false}). Thin marks (24px
  // cap, the slot's leftover is air), 4px rounded data end, square baseline end, 2px
  // surface gap between stacked segments, negative values grow down (or left) from one
  // zero line. Every slot is its own hit target (wider than the mark) with a tooltip,
  // on hover and on keyboard focus.
  //
  //   <Bars categories={['Jan', 'Feb']} series={[{ label: 'Spend', values: [120, 90] }]}
  //         format={(v) => fmtMoney(v, ccy)} axisFormat={(v) => fmtCompactValue(v, ccy)} />
  //   <Bars horizontal labels="ends" categories={names}
  //         series={[{ label: 'Value', values, colors: names.map((n) => colorFor(n)) }]} />
  //
  // props:
  //   categories  string[] | [{ label, key? }]
  //   series      [{ key?, label, values: (number|null)[], color?, colors?: string[] (per bar),
  //                 dashed?: bool | bool[] (estimate: hollow, dashed outline), tone?: 'sign' }]
  //               default colour: --accent for one series, identity order for several.
  //   lines       [{ key?, label, values, color?, dashed?, width? (px, default 2; 1 for a
  //               reference hairline such as an average) }] overlay on the same scale
  //               (vertical only), e.g. a running total.
  //   stacked (default true), horizontal, format(v), axisFormat(v), labels ('ends'|'none'),
  //   height (vertical px, default 240), rowHeight (horizontal px per row, default 32),
  //   legend ('auto'|'none'), totalLabel (tooltip total row of a stack), estimateLabel,
  //   emphasized ($bindable series key), sensitive (default true), ariaLabel, onSelect(index),
  //   tipExtra(index) -> [{ label, value }] extra tooltip rows (formatted and masked by the caller)
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import { fmtNum } from '$lib/format.js';
  import Tooltip from './Tooltip.svelte';
  import Legend from './Legend.svelte';
  import { identityColor, toneColor, fade, MASK } from './palette.js';
  import { niceTicks, barPath, linear, textWidth } from './scale.js';

  let {
    categories = [],
    series = [],
    lines = [],
    stacked = true,
    horizontal = false,
    format = (v) => fmtNum(v, 2),
    axisFormat = null,
    labels = 'none',
    height = 240,
    rowHeight = 32,
    legend = 'auto',
    totalLabel = '',
    estimateLabel = '',
    emphasized = $bindable(null),
    sensitive = true,
    ariaLabel = '',
    onSelect = null,
    tipExtra = null
  } = $props();

  const c = $derived(chartColors());
  const masked = $derived(sensitive && privacy.hidden);
  const fmt = (v) => (v == null ? '' : masked ? MASK : format(v));
  const fmtAxis = (v) => (masked ? MASK : (axisFormat ?? format)(v));

  const GAP = 2;
  const R = 4;
  const MAX_BAR = 24;

  const cats = $derived(
    categories.map((cat, i) =>
      typeof cat === 'object' && cat ? { key: cat.key ?? cat.label ?? i, label: String(cat.label ?? '') } : { key: cat ?? i, label: String(cat ?? '') }
    )
  );
  const n = $derived(cats.length);
  const num = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null);

  const S = $derived(
    series.map((s, si) => {
      const base = s.color ?? (series.length === 1 ? c.accent : identityColor(si, c));
      return {
        key: s.key ?? s.label ?? String(si),
        label: s.label ?? '',
        values: cats.map((_, i) => num(s.values?.[i])),
        colorAt: (i) =>
          s.colors?.[i] ?? (s.tone === 'sign' ? toneColor(num(s.values?.[i]), c) : base),
        legendColor: s.tone === 'sign' ? c.muted : base,
        dashedAt: (i) => (Array.isArray(s.dashed) ? !!s.dashed[i] : !!s.dashed)
      };
    })
  );
  const L = $derived(
    horizontal
      ? []
      : lines.map((l, li) => ({
          key: l.key ?? l.label ?? `line-${li}`,
          label: l.label ?? '',
          values: cats.map((_, i) => num(l.values?.[i])),
          color: l.color ?? c.text,
          dashed: !!l.dashed,
          width: l.width ?? 2
        }))
  );
  const isStacked = $derived(stacked && S.length > 1);

  // Value extent: stack sums per direction, or plain values; always includes zero.
  const dom = $derived.by(() => {
    let lo = 0;
    let hi = 0;
    for (let i = 0; i < n; i++) {
      if (isStacked) {
        let p = 0;
        let q = 0;
        for (const s of S) {
          const v = s.values[i] ?? 0;
          if (v > 0) p += v;
          else q += v;
        }
        hi = Math.max(hi, p);
        lo = Math.min(lo, q);
      } else {
        for (const s of S) {
          const v = s.values[i];
          if (v != null) {
            hi = Math.max(hi, v);
            lo = Math.min(lo, v);
          }
        }
      }
      for (const l of L) {
        const v = l.values[i];
        if (v != null) {
          hi = Math.max(hi, v);
          lo = Math.min(lo, v);
        }
      }
    }
    if (lo === 0 && hi === 0) hi = 1;
    const ticks = niceTicks(lo, hi, 4);
    return { ticks, min: ticks[0], max: ticks[ticks.length - 1] };
  });

  // Segments per category: [{ si, v, from, to, outer }] in value space. `outer` marks
  // the segment that carries the rounded end (the last one in its direction).
  const stacks = $derived.by(() =>
    cats.map((_, i) => {
      if (!isStacked) return S.map((s, si) => ({ si, v: s.values[i], from: 0, to: s.values[i] ?? 0, outer: true, inner: false }));
      let p = 0;
      let q = 0;
      const segs = [];
      for (let si = 0; si < S.length; si++) {
        const v = S[si].values[i];
        if (!v) continue;
        if (v > 0) {
          segs.push({ si, v, from: p, to: p + v, inner: p !== 0 });
          p += v;
        } else {
          segs.push({ si, v, from: q, to: q + v, inner: q !== 0 });
          q += v;
        }
      }
      const lastPos = segs.findLast((g) => g.v > 0);
      const lastNeg = segs.findLast((g) => g.v < 0);
      for (const g of segs) g.outer = g === lastPos || g === lastNeg;
      return segs;
    })
  );
  const totals = $derived(stacks.map((segs) => segs.reduce((t, g) => t + (g.v ?? 0), 0)));

  let wrapWidth = $state(0);
  let hovered = $state(null);
  let tip = $state(null);

  // ---- vertical geometry (px) ------------------------------------------------------
  const axisW = $derived(Math.ceil(Math.max(...dom.ticks.map((t) => textWidth(fmtAxis(t))), 12)) + 10);
  const padTop = $derived(labels === 'ends' ? 18 : 8);
  const padBottom = 22;
  const x0 = $derived(axisW);
  const x1 = $derived(Math.max(axisW + 1, wrapWidth - 4));
  const slot = $derived(n ? (x1 - x0) / n : 0);
  const yv = $derived(linear(dom.min, dom.max, height - padBottom, padTop));
  const barW = $derived.by(() => {
    if (!slot) return 0;
    if (isStacked || S.length <= 1) return Math.max(1, Math.min(MAX_BAR, slot * 0.7, slot - GAP));
    const k = S.length;
    const inner = Math.min(MAX_BAR * k + GAP * (k - 1), slot * 0.8);
    return Math.max(1, (inner - GAP * (k - 1)) / k);
  });
  const cx = (i) => x0 + slot * (i + 0.5);

  function segRect(i, g, gi) {
    const k = S.length;
    let x;
    if (isStacked || k <= 1) x = cx(i) - barW / 2;
    else {
      const inner = barW * k + GAP * (k - 1);
      x = cx(i) - inner / 2 + gi * (barW + GAP);
    }
    let yA = yv(g.from);
    let yB = yv(g.to);
    const up = g.to >= g.from;
    let top = Math.min(yA, yB);
    let h = Math.abs(yA - yB);
    // The surface gap is cut from the base side of a segment resting on another one.
    if (g.inner && h > GAP * 2) {
      h -= GAP;
      if (!up) top += GAP;
    }
    const end = g.outer ? (up ? 'top' : 'bottom') : 'none';
    return { x, y: top, w: barW, h, end };
  }

  const vMarks = $derived.by(() => {
    if (horizontal || !slot) return [];
    return stacks.map((segs, i) =>
      segs
        .map((g, gi) => {
          if (g.v == null || g.v === 0) return null;
          const r = segRect(i, g, isStacked ? 0 : gi);
          const s = S[g.si];
          const dashed = s.dashedAt(i);
          const color = s.colorAt(i);
          return {
            key: `${i}-${g.si}`,
            d: barPath(r.x, r.y, r.w, r.h, R, r.end),
            fill: dashed ? fade(color, 0.18) : color,
            stroke: dashed ? color : 'none',
            si: g.si
          };
        })
        .filter(Boolean)
    );
  });

  const linePaths = $derived(
    L.map((l) => ({
      ...l,
      d: l.values
        .map((v, i) => (v == null ? null : `${cx(i).toFixed(1)},${yv(v).toFixed(1)}`))
        .reduce((acc, p) => {
          if (p == null) return acc + ' ';
          return acc + (acc === '' || acc.endsWith(' ') ? 'M' : 'L') + p;
        }, '')
        .trim()
    }))
  );

  // Category labels thinned so they never collide, anchored on the newest (last) slot.
  const catStep = $derived.by(() => {
    if (!slot) return 1;
    const longest = Math.max(...cats.map((k) => textWidth(k.label)), 10);
    return Math.max(1, Math.ceil((Math.min(longest, 96) + 8) / slot));
  });
  const maxChars = $derived(Math.max(3, Math.floor((slot * catStep - 6) / textWidth('0'))));
  const clip = (s) => (s.length > maxChars ? s.slice(0, maxChars - 1) + '…' : s);

  // End labels: every cap that fits its slot; else only the extreme and the last bar.
  const endIdx = $derived.by(() => {
    if (labels !== 'ends' || horizontal) return new Set();
    const fitsAll = totals.every((t) => textWidth(fmt(t)) <= slot - 2);
    if (fitsAll) return new Set(cats.map((_, i) => i));
    let mx = 0;
    totals.forEach((t, i) => {
      if (Math.abs(t) > Math.abs(totals[mx])) mx = i;
    });
    return new Set([mx, n - 1]);
  });

  // ---- horizontal geometry (% of the track) ------------------------------------------
  const pct = $derived(linear(dom.min, dom.max, 0, 100));
  const hMarks = $derived.by(() => {
    if (!horizontal) return [];
    const zero = pct(0);
    return stacks.map((segs, i) =>
      segs
        .filter((g) => g.v != null && g.v !== 0)
        .map((g) => {
          const a = pct(g.from);
          const b = pct(g.to);
          const right = g.to >= g.from;
          const s = S[g.si];
          const color = s.colorAt(i);
          const dashed = s.dashedAt(i);
          return {
            key: `${i}-${g.si}`,
            left: Math.min(a, b),
            width: Math.abs(b - a),
            inset: g.inner ? GAP : 0,
            right,
            end: g.outer ? (right ? 'right' : 'left') : 'none',
            fill: dashed ? fade(color, 0.18) : color,
            outline: dashed ? color : null,
            si: g.si,
            lane: isStacked ? 0 : g.si,
            zero
          };
        })
    );
  });

  // ---- tooltip -------------------------------------------------------------------------
  function tipFor(i) {
    const rows = [];
    const order = isStacked ? [...S.keys()].reverse() : [...S.keys()];
    let dashedAny = false;
    for (const si of order) {
      const s = S[si];
      const v = s.values[i];
      if (v == null) continue;
      if (s.dashedAt(i)) dashedAny = true;
      rows.push({ label: s.label, value: fmt(v), color: s.colorAt(i), dashed: s.dashedAt(i) });
    }
    if (isStacked) rows.push({ label: totalLabel, value: fmt(totals[i]) });
    for (const l of L) {
      if (l.values[i] == null) continue;
      rows.push({ label: l.label, value: fmt(l.values[i]), color: l.color, dashed: l.dashed });
    }
    // Caller rows (already formatted and masked), e.g. a per-currency split of the bar.
    for (const r of tipExtra?.(i) ?? []) rows.push(r);
    return { title: cats[i]?.label ?? '', rows, note: dashedAny ? estimateLabel : '' };
  }

  function showV(i) {
    hovered = i;
    let top = height - padBottom;
    for (const g of stacks[i] ?? []) top = Math.min(top, yv(Math.max(g.from, g.to)));
    for (const l of L) if (l.values[i] != null) top = Math.min(top, yv(l.values[i]));
    tip = { x: cx(i), y: top, ...tipFor(i) };
  }
  function showH(i, e, row) {
    hovered = i;
    const wrapEl = row?.parentElement;
    const rect = wrapEl?.getBoundingClientRect();
    const x = e && rect ? e.clientX - rect.left : (wrapWidth * 2) / 3;
    tip = { x, y: row?.offsetTop ?? i * rowHeight, ...tipFor(i) };
  }
  function hide() {
    hovered = null;
    tip = null;
  }
  function keySelect(e, i) {
    if (onSelect && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault();
      onSelect(i);
    }
  }

  const legendItems = $derived(
    [
      ...(S.length > 1 || L.length ? S.map((s) => ({ key: s.key, label: s.label, color: s.legendColor, kind: 'rect' })) : []),
      ...L.map((l) => ({ key: l.key, label: l.label, color: l.color, kind: 'line', dashed: l.dashed }))
    ].filter((it) => it.label)
  );
  const showLegend = $derived(legend !== 'none' && legendItems.length >= 2);
  const fadedSeries = (si) => emphasized != null && S[si]?.key !== emphasized;
  const fadedLine = (l) => emphasized != null && l.key !== emphasized;
  const label = $derived(ariaLabel || [...S, ...L].map((s) => s.label).filter(Boolean).join(', '));
</script>

{#if horizontal}
  <div class="bars-h" bind:clientWidth={wrapWidth} role="list" aria-label={label || undefined}>
    {#each cats as cat, i (cat.key)}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        class="hrow"
        class:hovered={hovered === i}
        class:clickable={!!onSelect}
        style:height="{rowHeight}px"
        role="listitem"
        tabindex="0"
        aria-label="{cat.label}: {fmt(isStacked ? totals[i] : (S[0]?.values[i] ?? null))}"
        onpointermove={(e) => showH(i, e, e.currentTarget)}
        onpointerleave={hide}
        onfocus={(e) => showH(i, null, e.currentTarget)}
        onblur={hide}
        onclick={() => onSelect?.(i)}
        onkeydown={(e) => keySelect(e, i)}
      >
        <span class="hname" title={cat.label}>{cat.label}</span>
        <span class="htrack" style:--lanes={isStacked ? 1 : Math.max(1, S.length)}>
          {#if dom.min < 0}<span class="hzero" style:left="{pct(0)}%"></span>{/if}
          {#each hMarks[i] ?? [] as m (m.key)}
            <span
              class="hbar end-{m.end}"
              class:faded={fadedSeries(m.si)}
              class:outlined={!!m.outline}
              style:left={m.right ? `calc(${m.left}% + ${m.inset}px)` : `${m.left}%`}
              style:width="max(1px, calc({m.width}% - {m.inset}px))"
              style:--lane={m.lane}
              style:background={m.fill}
              style:--outline={m.outline}
            ></span>
          {/each}
        </span>
        {#if labels === 'ends'}
          <span class="hval">{fmt(isStacked ? totals[i] : (S[0]?.values[i] ?? null))}</span>
        {/if}
      </div>
    {/each}
    {#if tip}
      <Tooltip x={tip.x} y={tip.y} width={wrapWidth} title={tip.title} rows={tip.rows} note={tip.note} />
    {/if}
  </div>
{:else}
  <div class="bars-v" bind:clientWidth={wrapWidth} style:height="{height}px">
    {#if wrapWidth > 0 && n > 0}
      <svg width={wrapWidth} {height} role="img" aria-label={label || undefined}>
        <!-- grid + value axis -->
        {#each dom.ticks as t (t)}
          <line class="grid" x1={x0} x2={x1} y1={Math.round(yv(t)) + 0.5} y2={Math.round(yv(t)) + 0.5} stroke={c.gridLine} />
          <text class="axis" x={x0 - 8} y={yv(t) + 3} text-anchor="end" fill={c.dim}>{fmtAxis(t)}</text>
        {/each}
        {#if dom.min < 0}
          <line x1={x0} x2={x1} y1={Math.round(yv(0)) + 0.5} y2={Math.round(yv(0)) + 0.5} stroke={c.faint} stroke-width="1" />
        {/if}

        <!-- hover band, behind the marks -->
        {#if hovered != null}
          <rect x={x0 + slot * hovered} y={padTop - 4} width={slot} height={height - padBottom - padTop + 4} fill={c.surface2} />
        {/if}

        {#each vMarks as marks, i (cats[i].key)}
          {#each marks as m (m.key)}
            <path
              d={m.d}
              fill={m.fill}
              stroke={m.stroke}
              stroke-width={m.stroke === 'none' ? 0 : 1}
              stroke-dasharray={m.stroke === 'none' ? undefined : '3 2'}
              class="mark"
              class:faded={fadedSeries(m.si)}
            />
          {/each}
        {/each}

        {#each linePaths as l (l.key)}
          <path
            d={l.d}
            fill="none"
            stroke={l.color}
            stroke-width={l.width}
            stroke-linejoin="round"
            stroke-linecap="round"
            stroke-dasharray={l.dashed ? '5 4' : undefined}
            class="mark"
            class:faded={fadedLine(l)}
          />
          {#if hovered != null && l.values[hovered] != null}
            <circle cx={cx(hovered)} cy={yv(l.values[hovered])} r="4" fill={l.color} stroke={c.surface} stroke-width="2" />
          {/if}
        {/each}

        {#each [...endIdx] as i (i)}
          <text class="cap" x={cx(i)} y={yv(Math.max(0, totals[i])) - 5} text-anchor="middle" fill={c.muted} stroke={c.surface}>{fmt(totals[i])}</text>
        {/each}

        <!-- category axis -->
        {#each cats as cat, i (cat.key)}
          {#if (n - 1 - i) % catStep === 0}
            <text class="axis" x={cx(i)} y={height - 6} text-anchor="middle" fill={c.dim}>
              <title>{cat.label}</title>{clip(cat.label)}
            </text>
          {/if}
        {/each}

        <!-- hit targets: the whole slot, wider than the mark -->
        {#each cats as cat, i (cat.key)}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <rect
            class="hit"
            class:clickable={!!onSelect}
            x={x0 + slot * i}
            y={padTop - 4}
            width={slot}
            height={height - padBottom - padTop + 4}
            fill="transparent"
            tabindex="0"
            role="img"
            aria-label="{cat.label}: {fmt(totals[i])}"
            onpointerenter={() => showV(i)}
            onpointerleave={hide}
            onfocus={() => showV(i)}
            onblur={hide}
            onclick={() => onSelect?.(i)}
            onkeydown={(e) => keySelect(e, i)}
          />
        {/each}
      </svg>
      {#if tip}
        <Tooltip x={tip.x} y={tip.y} width={wrapWidth} title={tip.title} rows={tip.rows} note={tip.note} />
      {/if}
    {/if}
  </div>
{/if}
{#if showLegend}
  <div class="bars-legend">
    <Legend items={legendItems} active={emphasized} onToggle={(k) => (emphasized = k)} />
  </div>
{/if}

<style>
  .bars-v,
  .bars-h {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke-width: 0.5;
  }
  .axis,
  .cap {
    font-family: var(--mono);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  /* A surface halo keeps a cap label legible where an overlay line crosses it. */
  .cap {
    paint-order: stroke;
    stroke-width: 3px;
    stroke-linejoin: round;
  }
  .mark {
    transition: opacity 160ms ease;
  }
  .mark.faded,
  .hbar.faded {
    opacity: 0.3;
  }
  .hit {
    outline: none;
  }
  .hit.clickable,
  .hrow.clickable {
    cursor: pointer;
  }
  .hit:focus-visible {
    stroke: var(--accent);
    stroke-width: 1;
  }

  .hrow {
    display: grid;
    grid-template-columns: minmax(72px, 32%) minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    transition: background-color 160ms ease;
  }
  .hrow.hovered {
    background: var(--surface-hover);
  }
  .hname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-size: 12px;
  }
  .htrack {
    position: relative;
    height: calc(var(--lanes) * 12px + (var(--lanes) - 1) * 2px);
    min-width: 0;
  }
  .hzero {
    position: absolute;
    top: -4px;
    bottom: -4px;
    width: 1px;
    background: var(--faint);
  }
  .hbar {
    position: absolute;
    top: calc(var(--lane) * 14px);
    height: 12px;
    transition: opacity 160ms ease;
  }
  .hbar.end-right {
    border-radius: 0 4px 4px 0;
  }
  .hbar.end-left {
    border-radius: 4px 0 0 4px;
  }
  .hbar.outlined {
    border: 1px dashed var(--outline);
  }
  .hval {
    color: var(--text);
    font-family: var(--mono);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  .bars-legend {
    margin-top: var(--space-2);
  }
</style>
