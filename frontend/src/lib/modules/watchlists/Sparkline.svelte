<script>
  // Inline SVG sparkline for a watchlist row: normalized polyline + soft fill, green when the
  // series ends up vs its start, red when down. Trend only — no axes, no hover. Falls back to
  // a dashed baseline when there aren't at least two points.
  import { t } from '$lib/i18n';

  let { values = [], width = 120, height = 30 } = $props();

  const PAD = 2;

  let up = $derived(values.length >= 2 ? values[values.length - 1] >= values[0] : true);
  let color = $derived(up ? 'var(--green)' : 'var(--red)');

  let points = $derived.by(() => {
    if (values.length < 2) return [];
    const min = Math.min(...values);
    const max = Math.max(...values);
    const span = max - min || 1;
    const stepX = (width - 2 * PAD) / (values.length - 1);
    return values.map((v, i) => ({
      x: PAD + i * stepX,
      y: PAD + (1 - (v - min) / span) * (height - 2 * PAD)
    }));
  });

  let line = $derived(points.map((p) => `${p.x},${p.y}`).join(' '));
</script>

<div class="spark" style="width:{width}px;height:{height}px">
  {#if points.length}
    <svg viewBox="0 0 {width} {height}" preserveAspectRatio="none" role="img" aria-label={$t('watchlists.table.trend')}>
      <polyline points={line} fill="none" stroke={color} stroke-width="1" vector-effect="non-scaling-stroke" />
    </svg>
  {:else}
    <div class="flat" aria-label={$t('watchlists.table.trend')}></div>
  {/if}
</div>

<style>
  .spark {
    display: flex;
    align-items: center;
  }
  svg {
    width: 100%;
    height: 100%;
    display: block;
  }
  .flat {
    width: 100%;
    height: 1px;
    background: var(--border);
  }
</style>
