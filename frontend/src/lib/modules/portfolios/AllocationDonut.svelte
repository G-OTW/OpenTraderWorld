<script>
  // Allocation donut for one portfolio: share of market value by asset or by asset class
  // (toggle). Inline SVG, no chart lib. Top 7 slices + "Other"; 2px surface gaps between
  // segments; the legend direct-labels every slice (name, weight, value) since color alone
  // is never the identity carrier. Center of the donut shows the total.
  import { fmtMoney, fmtPct } from './api.js';
  import { t } from '$lib/i18n';

  let { assets = [], currency = 'USD' } = $props();

  const COLORS = [
    'var(--chart-1)',
    'var(--chart-2)',
    'var(--chart-3)',
    'var(--chart-4)',
    'var(--chart-5)',
    'var(--chart-6)',
    'var(--chart-7)'
  ];
  const OTHER_COLOR = 'var(--chart-8)';
  const MAX_SLICES = 7;

  let by = $state('asset'); // 'asset' | 'class'

  // labelKey resolved at render via $t so class/"Other" names relabel on language switch;
  // asset symbols have no labelKey and render as-is (ticker codes aren't translated).
  const CLASS_LABEL_KEYS = {
    crypto: 'portfolios.alloc.classCrypto',
    stock: 'portfolios.alloc.classStocks',
    etf: 'portfolios.alloc.classEtfs'
  };

  // Positive-value positions only; allocation is about what the portfolio holds now.
  let rows = $derived.by(() => {
    const held = assets.filter((a) => (a.market_value ?? 0) > 0);
    let groups;
    if (by === 'class') {
      const m = new Map();
      for (const a of held) {
        const key = a.asset_class;
        m.set(key, (m.get(key) ?? 0) + a.market_value);
      }
      groups = [...m.entries()].map(([cls, value]) => ({
        label: CLASS_LABEL_KEYS[cls] ? $t(CLASS_LABEL_KEYS[cls]) : cls,
        value
      }));
    } else {
      groups = held.map((a) => ({ label: a.symbol, value: a.market_value }));
    }
    groups.sort((a, b) => b.value - a.value);
    if (groups.length > MAX_SLICES + 1) {
      const head = groups.slice(0, MAX_SLICES);
      const rest = groups.slice(MAX_SLICES);
      head.push({
        label: $t('portfolios.alloc.other'),
        value: rest.reduce((s, g) => s + g.value, 0),
        other: true
      });
      groups = head;
    }
    return groups;
  });

  let total = $derived(rows.reduce((s, r) => s + r.value, 0));

  // Donut geometry: SVG arc per slice, gaps come from a surface-colored stroke.
  const SIZE = 180;
  const CX = SIZE / 2;
  const CY = SIZE / 2;
  const RO = 84; // outer radius
  const RI = 52; // inner radius

  function polar(r, angle) {
    return [CX + r * Math.sin(angle), CY - r * Math.cos(angle)];
  }
  // Annular sector path from startAngle to endAngle (radians, clockwise from 12 o'clock).
  function slicePath(a0, a1) {
    const large = a1 - a0 > Math.PI ? 1 : 0;
    const [x0, y0] = polar(RO, a0);
    const [x1, y1] = polar(RO, a1);
    const [x2, y2] = polar(RI, a1);
    const [x3, y3] = polar(RI, a0);
    return `M${x0},${y0} A${RO},${RO} 0 ${large} 1 ${x1},${y1} L${x2},${y2} A${RI},${RI} 0 ${large} 0 ${x3},${y3} Z`;
  }

  let slices = $derived.by(() => {
    if (total <= 0) return [];
    // A full-circle single slice degenerates the arc path — cap the sweep just below 2π.
    let angle = 0;
    return rows.map((r, i) => {
      const sweep = Math.min((r.value / total) * Math.PI * 2, Math.PI * 2 - 0.0001);
      const s = {
        ...r,
        color: r.other ? OTHER_COLOR : COLORS[i % COLORS.length],
        path: slicePath(angle, angle + sweep),
        pct: (r.value / total) * 100
      };
      angle += sweep;
      return s;
    });
  });

  let hovered = $state(null); // slice label or null
  let center = $derived.by(() => {
    const h = slices.find((s) => s.label === hovered);
    if (h) return { label: h.label, value: h.value, pct: h.pct };
    return { label: $t('portfolios.alloc.total'), value: total, pct: null };
  });
</script>

<div class="alloc">
  <div class="head">
    <h4>{$t('portfolios.alloc.title')}</h4>
    <div class="seg">
      <button class:active={by === 'asset'} onclick={() => (by = 'asset')}>{$t('portfolios.alloc.byAsset')}</button>
      <button class:active={by === 'class'} onclick={() => (by = 'class')}>{$t('portfolios.alloc.byClass')}</button>
    </div>
  </div>

  {#if slices.length === 0}
    <p class="muted small">{$t('portfolios.alloc.empty')}</p>
  {:else}
    <div class="body">
      <svg viewBox="0 0 {SIZE} {SIZE}" role="img" aria-label={$t('portfolios.alloc.title')}>
        {#each slices as s (s.label)}
          <path
            d={s.path}
            fill={s.color}
            class="slice"
            class:dim={hovered && hovered !== s.label}
            role="presentation"
            onmouseenter={() => (hovered = s.label)}
            onmouseleave={() => (hovered = null)}
          />
        {/each}
        <text x={CX} y={CY - 8} class="c-label" text-anchor="middle">{center.label}</text>
        <text x={CX} y={CY + 10} class="c-value" text-anchor="middle">
          {fmtMoney(center.value, currency, 0)}
        </text>
        {#if center.pct != null}
          <text x={CX} y={CY + 26} class="c-pct" text-anchor="middle">
            {center.pct.toFixed(1)}%
          </text>
        {/if}
      </svg>

      <ul class="legend">
        {#each slices as s (s.label)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_mouse_events_have_key_events -->
          <li
            class:dim={hovered && hovered !== s.label}
            onmouseenter={() => (hovered = s.label)}
            onmouseleave={() => (hovered = null)}
          >
            <span class="dot" style="background:{s.color}"></span>
            <span class="name">{s.label}</span>
            <span class="pct">{s.pct.toFixed(1)}%</span>
            <span class="val">{fmtMoney(s.value, currency, 0)}</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .alloc {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: 100%;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  h4 {
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    background: var(--surface-2);
    border: none;
    border-left: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .seg button:first-child {
    border-left: none;
  }
  .seg button.active {
    color: var(--text);
    background: var(--surface);
  }
  .body {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  svg {
    width: 180px;
    height: 180px;
    flex: 0 0 auto;
  }
  .slice {
    stroke: var(--surface);
    stroke-width: 2;
    transition: opacity 0.12s;
  }
  .slice.dim {
    opacity: 0.35;
  }
  .c-label {
    fill: var(--muted);
    font-size: 11px;
  }
  .c-value {
    fill: var(--text);
    font-size: 15px;
    font-weight: var(--fw-semibold);
  }
  .c-pct {
    fill: var(--muted);
    font-size: 11px;
  }
  .legend {
    list-style: none;
    flex: 1 1 140px;
    min-width: 140px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-1);
    border-radius: var(--radius);
    cursor: default;
    transition: opacity 0.12s;
  }
  .legend li.dim {
    opacity: 0.45;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 3px;
    flex: 0 0 auto;
  }
  .name {
    font-weight: var(--fw-semibold);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pct {
    margin-left: auto;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .val {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    min-width: 64px;
    text-align: right;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
</style>
