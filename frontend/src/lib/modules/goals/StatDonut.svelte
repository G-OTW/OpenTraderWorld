<script>
  // One donut + legend for a goals breakdown. Inline SVG, no chart lib — same arc
  // geometry as the portfolios allocation donut.
  //
  // `slices` is [{ key, label, value, color }]. Color is passed in rather than cycled:
  // every breakdown here is ordinal (status, urgency), so the hue carries meaning and
  // must not shift when a bucket empties out. Zero-value buckets stay in the legend —
  // "0 overdue" is information — but draw no arc.
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { title = '', slices = [], onpick = null, active = null } = $props();

  const total = $derived(slices.reduce((s, r) => s + r.value, 0));

  const SIZE = 168;
  const CX = SIZE / 2;
  const CY = SIZE / 2;
  const RO = 78;
  const RI = 50;

  function polar(r, angle) {
    return [CX + r * Math.sin(angle), CY - r * Math.cos(angle)];
  }
  function slicePath(a0, a1) {
    const large = a1 - a0 > Math.PI ? 1 : 0;
    const [x0, y0] = polar(RO, a0);
    const [x1, y1] = polar(RO, a1);
    const [x2, y2] = polar(RI, a1);
    const [x3, y3] = polar(RI, a0);
    return `M${x0},${y0} A${RO},${RO} 0 ${large} 1 ${x1},${y1} L${x2},${y2} A${RI},${RI} 0 ${large} 0 ${x3},${y3} Z`;
  }

  const arcs = $derived.by(() => {
    if (total <= 0) return [];
    let angle = 0;
    return slices
      .filter((s) => s.value > 0)
      .map((s) => {
        // A lone full-circle slice degenerates the arc path — cap just below 2π.
        const sweep = Math.min((s.value / total) * Math.PI * 2, Math.PI * 2 - 0.0001);
        const a = { ...s, path: slicePath(angle, angle + sweep) };
        angle += sweep;
        return a;
      });
  });

  let hovered = $state(null);
  // Hover wins over the pinned filter for the centre readout, so pointing at a slice
  // always answers "what is that one".
  const focusKey = $derived(hovered ?? active);
  const focus = $derived(slices.find((s) => s.key === focusKey) ?? null);
  const pct = (v) => (total > 0 ? (v / total) * 100 : 0);
</script>

<section class="donut-card">
  <h3>{title}</h3>

  {#if total === 0}
    <EmptyState icon="pie-chart" compact title={$t('goals.stats.noData')} />
  {:else}
    <div class="body">
      <svg viewBox="0 0 {SIZE} {SIZE}" role="img" aria-label={title}>
        {#each arcs as a (a.key)}
          <path
            d={a.path}
            fill={a.color}
            class="slice"
            class:dim={focusKey && focusKey !== a.key}
            class:clickable={!!onpick}
            role="presentation"
            onmouseenter={() => (hovered = a.key)}
            onmouseleave={() => (hovered = null)}
            onclick={() => onpick?.(a.key)}
          />
        {/each}
        <text x={CX} y={CY - 4} class="c-value" text-anchor="middle">
          {focus ? focus.value : total}
        </text>
        <text x={CX} y={CY + 14} class="c-label" text-anchor="middle">
          {focus ? `${pct(focus.value).toFixed(0)}%` : $t('goals.stats.total')}
        </text>
      </svg>

      <ul class="legend">
        {#each slices as s (s.key)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_mouse_events_have_key_events, a11y_click_events_have_key_events -->
          <li
            class:dim={focusKey && focusKey !== s.key}
            class:empty={s.value === 0}
            class:clickable={!!onpick && s.value > 0}
            onmouseenter={() => (hovered = s.key)}
            onmouseleave={() => (hovered = null)}
            onclick={() => s.value > 0 && onpick?.(s.key)}
          >
            <span class="dot" style="background:{s.color}"></span>
            <span class="name">{s.label}</span>
            <span class="val num">{s.value}</span>
            <span class="pct num">{pct(s.value).toFixed(0)}%</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</section>

<style>
  .donut-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    min-width: 0;
  }
  h3 {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .body {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  svg {
    width: 148px;
    height: 148px;
    flex: none;
  }
  .slice {
    transition: opacity var(--dur-fast) var(--ease);
    stroke: var(--surface);
    stroke-width: 2;
  }
  .slice.clickable {
    cursor: pointer;
  }
  .slice.dim,
  li.dim {
    opacity: 0.35;
  }
  .c-value {
    fill: var(--text);
    font-size: 22px;
    font-family: var(--mono);
  }
  .c-label {
    fill: var(--muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .legend {
    flex: 1;
    min-width: 148px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--space-2);
    padding: 3px var(--space-2);
    font-size: var(--text-sm);
    border-radius: var(--radius);
    transition: background-color var(--dur-fast) var(--ease), opacity var(--dur-fast) var(--ease);
  }
  li.clickable {
    cursor: pointer;
  }
  li.clickable:hover {
    background: var(--surface-2);
  }
  /* An empty bucket still earns its legend row, just a quieter one. */
  li.empty .name,
  li.empty .val,
  li.empty .pct {
    color: var(--dim);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .val {
    font-variant-numeric: tabular-nums;
  }
  .pct {
    color: var(--muted);
    font-size: var(--text-xs);
    min-width: 4ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
