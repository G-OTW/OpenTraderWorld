<script>
  // Donut + legend, the shape half the module mockups are built from (goals by status,
  // todos by category, journal exit reasons…). Pure SVG so it follows the theme through
  // `var(--chart-N)` without a canvas repaint on theme flip.
  //
  //   <Donut segments={[{ label: 'Reached', value: 4 }]} center="12" centerLabel="Goals" />
  //
  // A zero total draws the track alone rather than an empty circle of nothing.
  //
  // Hovering an arc or its legend row is the same gesture: the arc thickens, the rest of
  // the ring dims, and the hole reads that slice (value and share) instead of the total.
  import { tip } from '$lib/ui/tip.svelte.js';
  import { fmtNum } from '$lib/format';

  let {
    segments = [],          // [{ label, value, color? }]
    center = null,          // big figure in the hole (defaults to the total)
    centerLabel = '',
    size = 116,             // px, the ring box
    thickness = 17,
    legend = true,
    showPct = true,
    // Legend rows that are not arcs: a total, a baseline — listed but never drawn.
    extra = [],            // [{ label, value }]
    valueFormat = (v) => fmtNum(v, 0)
  } = $props();

  const rows = $derived(segments.filter((s) => Number(s.value) > 0));
  const total = $derived(rows.reduce((sum, s) => sum + Number(s.value), 0));

  const R = 50;
  const C = 2 * Math.PI * R;
  // Offsets accumulate so each arc starts where the previous one ended, at 12 o'clock.
  const arcs = $derived.by(() => {
    let at = 0;
    return rows.map((s, i) => {
      const frac = total > 0 ? Number(s.value) / total : 0;
      const arc = { ...s, frac, len: frac * C, off: at, color: s.color ?? `var(--chart-${(i % 8) + 1})` };
      at += arc.len;
      return arc;
    });
  });

  let hover = $state(-1);
  const active = $derived(hover >= 0 && hover < arcs.length ? arcs[hover] : null);
  const centerText = $derived(
    active ? valueFormat(active.value) : (center ?? valueFormat(total))
  );
  const centerSub = $derived(
    active ? `${active.label} · ${Math.round(active.frac * 100)}%` : centerLabel
  );

  function enter(event, i) {
    hover = i;
    const a = arcs[i];
    tip.show(event, {
      title: a.label,
      rows: [
        { color: a.color, label: `${Math.round(a.frac * 100)}%`, value: valueFormat(a.value) }
      ]
    });
  }
  function leave() {
    hover = -1;
    tip.hide();
  }
</script>

<div class="donut" class:with-legend={legend}>
  <div class="ring" style:width={`${size}px`} style:height={`${size}px`}>
    <svg viewBox="0 0 120 120" role="group" aria-label={centerLabel || 'chart'}>
      <circle class="track" cx="60" cy="60" r={R} fill="none" stroke-width={thickness} />
      {#each arcs as a, i (a.label)}
        <circle
          cx="60" cy="60" r={R} fill="none"
          role="img"
          aria-label={`${a.label}: ${valueFormat(a.value)} (${Math.round(a.frac * 100)}%)`}
          class="arc"
          class:dim={hover >= 0 && hover !== i}
          stroke={a.color}
          stroke-width={hover === i ? thickness + 4 : thickness}
          stroke-dasharray={`${a.len} ${C - a.len}`}
          stroke-dashoffset={-a.off}
          transform="rotate(-90 60 60)"
          onpointerenter={(e) => enter(e, i)}
          onpointermove={(e) => tip.move(e)}
          onpointerleave={leave}
        />
      {/each}
    </svg>
    <div class="hole">
      <span class="cval">{centerText}</span>
      {#if centerSub}<span class="clab">{centerSub}</span>{/if}
    </div>
  </div>

  {#if legend}
    <ul class="legend">
      {#each arcs as a, i (a.label)}
        <li
          class:on={hover === i}
          class:dim={hover >= 0 && hover !== i}
          onpointerenter={(e) => enter(e, i)}
          onpointermove={(e) => tip.move(e)}
          onpointerleave={leave}
        >
          <span class="w-dot" style:background={a.color}></span>
          <span class="w-name">{a.label}</span>
          <span class="w-num v">{valueFormat(a.value)}</span>
          {#if showPct}<span class="w-num p">{Math.round(a.frac * 100)}%</span>{/if}
        </li>
      {/each}
      {#each extra as e (e.label)}
        <li class="static">
          <span class="w-dot muted"></span>
          <span class="w-name">{e.label}</span>
          <span class="w-num v">{valueFormat(e.value)}</span>
          {#if showPct}<span class="w-num p"></span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .donut {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }
  .ring {
    position: relative;
    flex-shrink: 0;
  }
  .ring svg {
    width: 100%;
    height: 100%;
    display: block;
    overflow: visible;
  }
  .track {
    stroke: var(--surface-3);
  }
  /* Only the drawn arc takes the pointer, so the ring's hole and gaps stay inert. */
  .arc {
    pointer-events: stroke;
    cursor: default;
    transition:
      stroke-width var(--dur-fast) var(--ease),
      opacity var(--dur-fast) var(--ease);
  }
  .arc.dim {
    opacity: 0.35;
  }
  .hole {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1px;
    pointer-events: none;
    padding: 0 12px;
  }
  .cval {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 20px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.01em;
    line-height: 1.1;
    color: var(--text);
  }
  .clab {
    font-size: 10px;
    color: var(--dim);
    line-height: 1.15;
    text-align: center;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .legend {
    flex: 1;
    min-width: 0;
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    line-height: var(--lh-tight);
    min-width: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .legend li.dim {
    opacity: 0.45;
  }
  .legend li.on .w-name {
    color: var(--text);
  }
  .legend .w-name {
    flex: 1;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .w-dot.muted {
    background: var(--faint);
  }
  .v {
    color: var(--text);
  }
  .p {
    color: var(--dim);
    font-size: var(--text-xs);
    min-width: 34px;
  }
</style>
