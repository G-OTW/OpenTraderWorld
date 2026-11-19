<script>
  // Part to whole as one bar: up to `max` named parts plus "Other", widths proportional
  // to the values, 2px surface gap between parts, legend rows under it (name, value,
  // share). Hover a part for its tooltip; click a legend row to emphasise it.
  //
  //   <Segmented items={rows.map((r) => ({ label: r.name, value: r.value }))}
  //              otherLabel={$t('...other')} format={(v) => fmtMoney(v, ccy)} />
  //
  // props:
  //   items   [{ key?, label, value, color?, colorIndex? }]  values <= 0 are dropped.
  //           Colour follows the entity: pass `color` (or a stable `colorIndex`) when the
  //           list can be filtered; otherwise parts take the identity order by rank.
  //   max (named parts, default 4, at most 6), otherLabel, format(v), size ('thin' 8px |
  //   'tall' 28px with shares printed where they fit), legend (default true),
  //   emphasized ($bindable key), sensitive (default true), ariaLabel
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import { fmtNum, fmtPct } from '$lib/format.js';
  import Tooltip from './Tooltip.svelte';
  import Legend from './Legend.svelte';
  import { partsOf, MASK } from './palette.js';
  import { textWidth } from './scale.js';

  let {
    items = [],
    max = 4,
    otherLabel = 'Other',
    format = (v) => fmtNum(v, 2),
    size = 'thin',
    legend = true,
    emphasized = $bindable(null),
    sensitive = true,
    ariaLabel = ''
  } = $props();

  const c = $derived(chartColors());
  const masked = $derived(sensitive && privacy.hidden);
  const fmt = (v) => (masked ? MASK : format(v));
  const share = (p) => fmtPct(p * 100, 1);

  const model = $derived(partsOf(items, { max, otherLabel, colors: c }));
  const parts = $derived(model.parts);
  const tall = $derived(size === 'tall');

  let wrapWidth = $state(0);
  let hoverKey = $state(null);
  let legendHover = $state(null);
  let tip = $state(null);
  const focusKey = $derived(hoverKey ?? legendHover ?? emphasized);

  // Pixel geometry, for tooltips and for deciding which shares fit inside a tall block.
  const GAP = 2;
  const geo = $derived.by(() => {
    const free = Math.max(0, wrapWidth - GAP * Math.max(0, parts.length - 1));
    let x = 0;
    return parts.map((p) => {
      const w = free * p.share;
      const g = { x, w };
      x += w + GAP;
      return g;
    });
  });

  // Ink for a share printed inside a fill: white on a dark fill, black on a light one.
  function ink(color) {
    const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(String(color ?? ''));
    if (!m) return 'white';
    const [r, g, b] = m.slice(1, 4).map((h) => {
      const v = parseInt(h, 16) / 255;
      return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.4 ? 'black' : 'white';
  }

  function show(i) {
    const p = parts[i];
    hoverKey = p.key;
    const g = geo[i];
    tip = {
      x: g.x + g.w / 2,
      y: 0,
      title: p.label,
      rows: [{ value: fmt(p.value), label: share(p.share), color: p.color }]
    };
  }
  function hide() {
    hoverKey = null;
    tip = null;
  }

  const legendItems = $derived(
    parts.map((p) => ({ key: p.key, label: p.label, color: p.color, kind: 'rect', value: fmt(p.value), share: share(p.share) }))
  );
  const label = $derived(ariaLabel || parts.map((p) => `${p.label} ${share(p.share)}`).join(', '));
</script>

{#if parts.length}
  <div class="seg-wrap">
    <div
      class="seg"
      class:tall
      bind:clientWidth={wrapWidth}
      role="img"
      aria-label={label}
      onpointerleave={hide}
    >
      {#each parts as p, i (p.key)}
        {@const fits = tall && geo[i] && textWidth(share(p.share), 11) + 12 <= geo[i].w}
        <span
          class="part"
          class:faded={focusKey != null && focusKey !== p.key}
          style:flex-grow={p.share}
          style:background={p.color}
          style:color={ink(p.color)}
          role="presentation"
          onpointerenter={() => show(i)}
        >
          {#if fits}<span class="inside">{share(p.share)}</span>{/if}
        </span>
      {/each}
      {#if tip}
        <Tooltip x={tip.x} y={tip.y} width={wrapWidth} title={tip.title} rows={tip.rows} />
      {/if}
    </div>
    {#if legend}
      <Legend
        items={legendItems}
        layout="rows"
        active={emphasized ?? legendHover}
        onToggle={(k) => (emphasized = k)}
        onHover={(k) => (legendHover = k)}
      />
    {/if}
  </div>
{/if}

<style>
  .seg-wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .seg {
    position: relative;
    display: flex;
    gap: 2px;
    width: 100%;
    height: 8px;
  }
  .seg.tall {
    height: 28px;
  }
  .part {
    flex-basis: 0;
    min-width: 1px;
    display: flex;
    align-items: center;
    transition: opacity 160ms ease;
  }
  /* Thin bar: fully rounded outer ends, square joints. */
  .seg:not(.tall) .part:first-child {
    border-radius: 4px 0 0 4px;
  }
  .seg:not(.tall) .part:last-child {
    border-radius: 0 4px 4px 0;
  }
  .seg:not(.tall) .part:only-child {
    border-radius: 4px;
  }
  /* Tall bar: every part its own block. */
  .seg.tall .part {
    border-radius: 4px;
  }
  .part.faded {
    opacity: 0.35;
  }
  .inside {
    padding-left: var(--space-2);
    font-family: var(--mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
  }
</style>
