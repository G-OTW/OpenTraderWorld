<script>
  // Part to whole as a ring, same data contract as Segmented: up to `max` named parts
  // plus "Other" (6 at most), 2px surface gaps of constant width, the total in the
  // centre. Hovering a part (or a legend row) puts its name, value and share in the
  // centre; clicking a legend row keeps that part emphasised.
  //
  //   <Donut items={rows} totalLabel={$t('...total')} otherLabel={$t('...other')}
  //          format={(v) => fmtMoney(v, ccy, 0)} />
  //
  // props: items [{ key?, label, value, color?, colorIndex? }], max (default 4, at most 6),
  //        otherLabel, totalLabel, total (override the centred figure, e.g. a net value
  //        that differs from the sum of the parts), format(v), size (px, default 168),
  //        thickness (px, default 20), legend ('side' | 'bottom' | 'none', default 'side'),
  //        emphasized ($bindable key), sensitive (default true), ariaLabel
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import { fmtNum, fmtPct } from '$lib/format.js';
  import Legend from './Legend.svelte';
  import { partsOf, MASK } from './palette.js';

  let {
    items = [],
    max = 4,
    otherLabel = 'Other',
    totalLabel = '',
    total = null,
    format = (v) => fmtNum(v, 2),
    size = 168,
    thickness = 20,
    legend = 'side',
    emphasized = $bindable(null),
    sensitive = true,
    ariaLabel = ''
  } = $props();

  const c = $derived(chartColors());
  const masked = $derived(sensitive && privacy.hidden);
  const fmt = (v) => (masked ? MASK : format(v));
  const share = (p) => fmtPct(p * 100, 1);

  const model = $derived(partsOf(items, { max: Math.min(6, max), otherLabel, colors: c }));
  const parts = $derived(model.parts);

  const GAP = 2;
  const CX = $derived(size / 2);
  const RO = $derived(size / 2 - 1);
  const RI = $derived(Math.max(4, RO - thickness));

  const pt = (r, a) => [CX + r * Math.sin(a), CX - r * Math.cos(a)];
  const f = (n) => n.toFixed(2);

  function sector(a0, a1) {
    const sweep = a1 - a0;
    // Constant-width gap: the angular pad is larger on the inner radius.
    const po = sweep > (GAP / RO) * 2 ? GAP / 2 / RO : 0;
    const pi = sweep > (GAP / RI) * 2 ? GAP / 2 / RI : 0;
    const large = sweep - 2 * po > Math.PI ? 1 : 0;
    const [x0, y0] = pt(RO, a0 + po);
    const [x1, y1] = pt(RO, a1 - po);
    const [x2, y2] = pt(RI, a1 - pi);
    const [x3, y3] = pt(RI, a0 + pi);
    return `M${f(x0)},${f(y0)}A${f(RO)},${f(RO)} 0 ${large} 1 ${f(x1)},${f(y1)}L${f(x2)},${f(y2)}A${f(RI)},${f(RI)} 0 ${large} 0 ${f(x3)},${f(y3)}Z`;
  }
  const ring = $derived(
    `M${f(CX - RO)},${f(CX)}a${f(RO)},${f(RO)} 0 1,0 ${f(2 * RO)},0a${f(RO)},${f(RO)} 0 1,0 ${f(-2 * RO)},0Z` +
      `M${f(CX - RI)},${f(CX)}a${f(RI)},${f(RI)} 0 1,0 ${f(2 * RI)},0a${f(RI)},${f(RI)} 0 1,0 ${f(-2 * RI)},0Z`
  );

  const arcs = $derived.by(() => {
    let a = 0;
    return parts.map((p) => {
      const a1 = a + p.share * Math.PI * 2;
      const d = parts.length === 1 ? ring : sector(a, a1);
      a = a1;
      return { ...p, d };
    });
  });

  let hoverKey = $state(null);
  let legendHover = $state(null);
  const focusKey = $derived(hoverKey ?? legendHover ?? emphasized);
  const focused = $derived(parts.find((p) => p.key === focusKey) ?? null);

  const legendItems = $derived(
    parts.map((p) => ({ key: p.key, label: p.label, color: p.color, kind: 'rect', value: fmt(p.value), share: share(p.share) }))
  );
  const label = $derived(ariaLabel || parts.map((p) => `${p.label} ${share(p.share)}`).join(', '));
  // The centre caption fits inside the hole; a long name is cut with an ellipsis.
  const maxChars = $derived(Math.max(4, Math.floor((2 * RI - 12) / 7)));
  const clip = (t) => (t && t.length > maxChars ? t.slice(0, maxChars - 1) + '…' : t);
  const centreValue = $derived(focused ? fmt(focused.value) : fmt(total ?? model.total));
</script>

{#if parts.length}
  <div class="donut {legend}">
    <svg width={size} height={size} viewBox="0 0 {size} {size}" role="img" aria-label={label}>
      {#each arcs as p (p.key)}
        <path
          d={p.d}
          fill={p.color}
          fill-rule="evenodd"
          class="arc"
          class:faded={focusKey != null && focusKey !== p.key}
          role="presentation"
          onpointerenter={() => (hoverKey = p.key)}
          onpointerleave={() => (hoverKey = null)}
        />
      {/each}
      <text class="c-label" x={CX} y={CX - 10} text-anchor="middle" fill={c.dim}>
        {clip(focused ? focused.label : totalLabel)}
      </text>
      <text class="c-value" x={CX} y={CX + 8} text-anchor="middle" fill={c.text}>{centreValue}</text>
      {#if focused}
        <text class="c-share" x={CX} y={CX + 24} text-anchor="middle" fill={c.muted}>{share(focused.share)}</text>
      {/if}
    </svg>
    {#if legend !== 'none'}
      <div class="d-legend">
        <Legend
          items={legendItems}
          layout="rows"
          active={emphasized ?? legendHover}
          onToggle={(k) => (emphasized = k)}
          onHover={(k) => (legendHover = k)}
        />
      </div>
    {/if}
  </div>
{/if}

<style>
  .donut {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }
  .donut.side {
    flex-wrap: wrap;
  }
  .donut.bottom {
    flex-direction: column;
    align-items: stretch;
  }
  .donut.bottom svg {
    align-self: center;
  }
  svg {
    flex: none;
    display: block;
  }
  .arc {
    transition: opacity 160ms ease;
  }
  .arc.faded {
    opacity: 0.3;
  }
  .c-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .c-value {
    font-family: var(--mono);
    font-size: 15px;
  }
  .c-share {
    font-family: var(--mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .d-legend {
    flex: 1 1 180px;
    min-width: 0;
  }
</style>
