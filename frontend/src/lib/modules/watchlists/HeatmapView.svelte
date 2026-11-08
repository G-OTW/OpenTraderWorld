<script>
  // Heatmap: one tile per symbol, sized by market cap where the source serves it (equal
  // size when it does not, never "tiny" for unknown: such a tile says "size unknown"),
  // tinted by the chosen period's change. The tint is the gain or loss tone at an alpha
  // that grows with the magnitude and stops at a wash, never a saturated block; the sign
  // is always in the text. Tiles are separated by a 2px surface gap.
  import Tooltip from '$lib/ui/charts/Tooltip.svelte';
  import { toneColor, fade } from '$lib/ui/charts/palette.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { treemap, heatAlpha, fmtCap } from './views.js';
  import { fmtSignedPct, fmtQuote } from './api.js';
  import { t } from '$lib/i18n';

  let { items = [], onopen = () => {} } = $props();

  const PERIODS = [
    // cap: the move that reaches the strongest tint, wider for the longer periods.
    { id: 'change_24h', key: 'watchlists.table.h24', cap: 4 },
    { id: 'change_7d', key: 'watchlists.table.d7', cap: 8 },
    { id: 'change_30d', key: 'watchlists.table.d30', cap: 15 }
  ];
  const GAP = 2;
  let period = $state('change_24h');
  let width = $state(0);
  let hover = $state(null); // { id, x, y }

  const c = $derived(chartColors());
  const height = $derived(width > 0 && width < 600 ? 320 : 420);
  const sized = (i) => typeof i.quote?.mcap === 'number' && i.quote.mcap > 0;
  const anySized = $derived(items.some(sized));
  const rects = $derived(
    treemap(
      items.map((i) => ({ id: i.id, weight: i.quote?.mcap })),
      width,
      height
    )
  );
  const byId = $derived(new Map(items.map((i) => [i.id, i])));
  const current = $derived(PERIODS.find((p) => p.id === period) ?? PERIODS[0]);
  const periodLabel = $derived($t(current.key));

  function fill(v) {
    const a = heatAlpha(v, current.cap, 0.12, 0.42);
    if (a == null || a === 0) return 'var(--surface-2)';
    return fade(toneColor(v, c), a);
  }

  const na = $derived($t('watchlists.col.na'));
  const tip = $derived.by(() => {
    if (!hover) return null;
    const it = byId.get(hover.id);
    if (!it) return null;
    const v = it.quote?.[period];
    return {
      x: hover.x,
      y: hover.y,
      title: it.name ? `${it.symbol} · ${it.name}` : it.symbol,
      rows: [
        { value: fmtSignedPct(v), label: periodLabel },
        { value: fmtQuote(it.quote?.price_usd, na), label: $t('watchlists.table.price') },
        {
          value: sized(it) ? fmtCap(it.quote.mcap) : $t('watchlists.heat.sizeUnknown'),
          label: $t('watchlists.col.mcap')
        }
      ]
    };
  });

  function show(r, e) {
    hover = { id: r.id, x: r.x + r.w / 2, y: e?.type === 'focus' ? r.y + r.h / 2 : r.y + Math.min(r.h / 2, 24) };
  }
</script>

<div class="heat">
  <div class="bar">
    <div class="seg" role="group" aria-label={$t('watchlists.view.period')}>
      {#each PERIODS as p (p.id)}
        <button class="chip" class:active={period === p.id} aria-pressed={period === p.id} onclick={() => (period = p.id)}>
          {$t(p.key)}
        </button>
      {/each}
    </div>
    <span class="note">{anySized ? $t('watchlists.heat.sized') : $t('watchlists.heat.equal')}</span>
  </div>
  <div class="box" bind:clientWidth={width} style:height="{height}px" role="group" aria-label={$t('watchlists.view.heatmap')}>
    {#each rects as r (r.id)}
      {@const it = byId.get(r.id)}
      {@const v = it?.quote?.[period]}
      {@const room = r.w > 52 && r.h > 34}
      <button
        class="tile"
        class:lit={hover?.id === r.id}
        style:left="{r.x + GAP / 2}px"
        style:top="{r.y + GAP / 2}px"
        style:width="{Math.max(0, r.w - GAP)}px"
        style:height="{Math.max(0, r.h - GAP)}px"
        style:background={fill(v)}
        aria-label="{it?.symbol} {fmtSignedPct(v)}"
        onclick={() => onopen(it)}
        onmouseenter={(e) => show(r, e)}
        onmouseleave={() => (hover = null)}
        onfocus={(e) => show(r, e)}
        onblur={() => (hover = null)}
      >
        {#if room}
          <span class="sym">{it?.symbol}</span>
          <span class="chg">{v == null ? na : fmtSignedPct(v)}</span>
          {#if anySized && !sized(it) && r.h > 56}
            <span class="unk">{$t('watchlists.heat.sizeUnknown')}</span>
          {/if}
        {/if}
      </button>
    {/each}
    {#if tip}
      <Tooltip x={tip.x} y={tip.y} {width} title={tip.title} rows={tip.rows} />
    {/if}
  </div>
</div>

<style>
  .heat {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .seg {
    display: inline-flex;
    gap: var(--space-1);
  }
  .note {
    font-size: 12px;
    color: var(--dim);
  }
  .box {
    position: relative;
    width: 100%;
    min-height: 160px;
  }
  .tile {
    position: absolute;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text);
    cursor: pointer;
    padding: 0 var(--space-1);
    overflow: hidden;
    transition: outline-color var(--dur-fast) var(--ease);
    outline: 1px solid transparent;
    outline-offset: -1px;
  }
  .tile.lit,
  .tile:focus-visible {
    outline-color: var(--muted);
  }
  .sym {
    font-size: 12px;
    font-weight: 600;
    line-height: 1.2;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chg {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    line-height: 1.2;
  }
  .unk {
    font-size: 10px;
    color: var(--dim);
    line-height: 1.2;
  }
</style>
