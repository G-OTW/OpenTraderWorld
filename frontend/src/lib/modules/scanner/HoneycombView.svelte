<script>
  // Honeycomb view: one hexagon per item, packed. The fill is the view's measure on a
  // diverging (signed change) or sequential (volume, hit recency) scale, or lit/unlit for a
  // signal; the border is the item's assigned color (its own, else its hive's). The tile
  // shows the ticker and the value, hover gives the row, Enter or a click opens the chart. A
  // tile pulses once when it gets a new hit.
  import { goto } from '$app/navigation';
  import { chartUrl } from './api.js';
  import {
    hexLayout,
    hexPoints,
    colorScale,
    scaleKind,
    inkOn,
    metricValue,
    rowMetrics,
    sortRows,
    itemColor,
    concreteColor
  } from './board.js';
  import { fmtNum, fmtSignedPct } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { items = [], hives = [], signals = [], live = {}, metric = 'change_pct', sort = 'ticker', size = 'm', palette, hitsOnly = false } = $props();

  let width = $state(600);
  let hover = $state(null);
  let pulsing = $state({});
  let lastHits = {};

  const R = { s: 20, m: 28, l: 38 };
  const r = $derived(R[size] ?? 28);
  const rows = $derived(sortRows(hitsOnly ? items.filter((i) => i.last_hit_at) : items, sort, live));
  const layout = $derived(hexLayout(rows.length, Math.max(width, 80), r, 3));
  const values = $derived(rows.map((it) => metricValue(it, metric, live[it.id])));
  const signal = $derived(metric.startsWith('signal:') ? signals.find((s) => s.id === metric.slice(7)) : null);
  const colors = $derived({
    neg: palette.diverge[0],
    mid: palette.diverge[1],
    pos: palette.diverge[2],
    low: palette.surface2,
    high: palette.accent,
    lit: concreteColor(signal?.color, palette.series) || palette.accent,
    off: palette.surface2,
    none: palette.border
  });
  const scale = $derived(colorScale(metric, values, colors));

  // The ticker is sized to its hexagon: the font shrinks to fit the inner width, then the
  // quote suffix drops, then the label truncates. The full ticker stays in the tooltip.
  const CHAR_W = 0.62;
  const MIN_FONT = 8;
  const QUOTE = /(USDT|USDC|USD)$/;
  function fitLabel(ticker, r) {
    const max = r > 30 ? 14 : r > 22 ? 11 : 10;
    const room = Math.sqrt(3) * (r - 1) - 6;
    const fit = (s) => Math.min(max, room / (s.length * CHAR_W));
    let text = ticker;
    if (fit(text) < MIN_FONT) text = text.replace(QUOTE, '') || text;
    if (fit(text) < MIN_FONT) text = text.slice(0, Math.max(1, Math.floor(room / (MIN_FONT * CHAR_W)) - 1)) + '…';
    return { text, size: Math.max(MIN_FONT, fit(text)) };
  }
  const labels = $derived(rows.map((it) => fitLabel(it.ticker, r)));

  // A new hit since the last render pulses its tile once.
  $effect(() => {
    const fresh = {};
    for (const it of items) {
      const prev = lastHits[it.id];
      if (it.last_hit_at && prev !== undefined && prev !== it.last_hit_at) fresh[it.id] = true;
      lastHits[it.id] = it.last_hit_at ?? null;
    }
    if (Object.keys(fresh).length) {
      pulsing = fresh;
      setTimeout(() => (pulsing = {}), 1400);
    }
  });

  function label(v) {
    if (v == null) return '';
    if (typeof v === 'boolean') return v ? $t('scanner.board.lit') : $t('scanner.board.unlit');
    if (metric === 'volume_ratio') return `${fmtNum(v, 1)}x`;
    if (metric === 'hit_recency') return `${fmtNum(v, 1)}h`;
    return fmtSignedPct(v, 1);
  }

  // The tip sits in the viewport, not in the card: the card clips its overflow.
  const TIP_W = 200;
  const TIP_H = 130;
  function point(it, el) {
    const b = el.getBoundingClientRect();
    const x = Math.max(4, Math.min(b.right, window.innerWidth - TIP_W - 4));
    const y = b.bottom + TIP_H > window.innerHeight ? Math.max(4, b.top - TIP_H) : b.bottom;
    hover = { it, x, y };
  }

  const open = (it) => goto(chartUrl(it));
  const legend = $derived(scaleKind(metric));
</script>

<svelte:window onscrollcapture={() => (hover = null)} />

<div class="hc" bind:clientWidth={width}>
  {#if !rows.length}
    <p class="hint pad">{hitsOnly ? $t('scanner.board.noHits') : $t('scanner.board.noItems')}</p>
  {:else}
    <svg width={layout.width} height={layout.height + 4} role="group" aria-label={$t('scanner.view.honeycomb')}>
      {#each rows as it, i (it.id)}
        {@const c = layout.cells[i]}
        {@const fill = scale(values[i])}
        {@const border = concreteColor(itemColor(it, hives), palette.series) || palette.borderControl}
        <a
          href={chartUrl(it)}
          class="cell"
          class:pulse={pulsing[it.id]}
          aria-label={`${it.ticker} ${label(values[i])}`}
          onmouseenter={(e) => point(it, e.currentTarget)}
          onmouseleave={() => (hover = null)}
          onfocus={(e) => point(it, e.currentTarget)}
          onblur={() => (hover = null)}
          onclick={(e) => {
            e.preventDefault();
            open(it);
          }}
        >
          <polygon points={hexPoints(c.x, c.y + 2, r - 1)} fill={fill} stroke={border} stroke-width="2" />
          <text x={c.x} y={c.y + 2 - (r > 22 ? 4 : 0)} fill={inkOn(fill, palette.text, '#ffffff')} class="tk" font-size={labels[i].size}>{labels[i].text}</text>
          {#if r > 22}
            <text x={c.x} y={c.y + 2 + 10} fill={inkOn(fill, palette.text, '#ffffff')} class="val" font-size="10">{label(values[i])}</text>
          {/if}
        </a>
      {/each}
    </svg>
    {#if hover}
      {@const m = rowMetrics(hover.it, live[hover.it.id])}
      <div class="tip" style="left:{hover.x}px; top:{hover.y}px" role="tooltip">
        <div class="tt">{hover.it.ticker} <span>{hover.it.timeframe}</span></div>
        <div>{$t('scanner.col.close')}: {m.close != null ? fmtNum(m.close, 4) : ''}</div>
        <div>{$t('scanner.col.change')}: {m.change_pct != null ? fmtSignedPct(m.change_pct) : ''}</div>
        <div>{$t('scanner.col.fromOpen')}: {m.change_from_open != null ? fmtSignedPct(m.change_from_open) : ''}</div>
        <div>{$t('scanner.col.volume')}: {m.volume_ratio != null ? `${fmtNum(m.volume_ratio, 2)}x` : ''}</div>
        {#if hover.it.last_hit_at}<div>{$t('scanner.col.lastHit')}: {new Date(hover.it.last_hit_at).toLocaleString()}</div>{/if}
      </div>
    {/if}
    <div class="legend" aria-hidden="true">
      {#if legend === 'diverging'}
        <span>{fmtSignedPct(scale.domain[0], 1)}</span>
        <span class="bar" style="background:linear-gradient(90deg,{colors.neg},{colors.mid},{colors.pos})"></span>
        <span>{fmtSignedPct(scale.domain[1], 1)}</span>
      {:else if legend === 'sequential'}
        <span>{metric === 'hit_recency' ? '0h' : '0'}</span>
        <span class="bar" style="background:linear-gradient(90deg,{metric === 'hit_recency' ? colors.high : colors.low},{metric === 'hit_recency' ? colors.low : colors.high})"></span>
        <span>{metric === 'hit_recency' ? '24h' : `${fmtNum(scale.domain[1], 1)}x`}</span>
      {:else}
        <span class="sw" style="background:{colors.lit}"></span><span>{$t('scanner.board.lit')}</span>
        <span class="sw" style="background:{colors.off}"></span><span>{$t('scanner.board.unlit')}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .hc {
    position: relative;
    padding: var(--space-2);
    min-width: 0;
  }
  .pad {
    padding: var(--space-4);
  }
  svg {
    display: block;
    max-width: 100%;
  }
  .cell {
    cursor: pointer;
    outline: none;
  }
  .cell:focus-visible polygon,
  .cell:hover polygon {
    stroke-width: 3;
    filter: brightness(0.96);
  }
  .tk {
    text-anchor: middle;
    dominant-baseline: middle;
    font-family: var(--mono);
    pointer-events: none;
  }
  .val {
    text-anchor: middle;
    dominant-baseline: middle;
    font-family: var(--mono);
    pointer-events: none;
    opacity: 0.85;
  }
  .pulse polygon {
    animation: pulse 1.2s ease-out 1;
    transform-box: fill-box;
    transform-origin: center;
  }
  @keyframes pulse {
    0% {
      transform: scale(1);
    }
    30% {
      transform: scale(1.12);
    }
    100% {
      transform: scale(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pulse polygon {
      animation: none;
    }
  }
  .tip {
    position: fixed;
    z-index: 50;
    background: var(--surface-raised, var(--surface));
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-1);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
    pointer-events: none;
    min-width: 160px;
  }
  .tt {
    color: var(--text);
    font-family: var(--mono);
    margin-bottom: var(--space-1);
  }
  .tt span {
    color: var(--dim);
  }
  .legend {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--dim);
    margin-top: var(--space-2);
    font-family: var(--mono);
  }
  .bar {
    width: 120px;
    height: 8px;
    border-radius: 2px;
  }
  .sw {
    width: 10px;
    height: 10px;
    border-radius: 2px;
  }
</style>
