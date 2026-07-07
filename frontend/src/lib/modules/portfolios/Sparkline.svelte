<script>
  // Tiny inline SVG sparkline for a portfolio card: a normalized polyline + soft fill, colored
  // green when the series ends up vs its start, red when down. Purely a trend "esquisse" — no axes,
  // no hover. Falls back to a flat baseline when there aren't at least two points.
  import { t } from '$lib/i18n';

  let { values = [], height = 44 } = $props();

  const W = 240;
  const PAD = 3;

  let up = $derived(values.length >= 2 ? values[values.length - 1] >= values[0] : true);
  let color = $derived(up ? 'var(--green)' : 'var(--red)');

  let points = $derived.by(() => {
    if (values.length < 2) return [];
    const min = Math.min(...values);
    const max = Math.max(...values);
    const span = max - min || 1;
    const stepX = (W - 2 * PAD) / (values.length - 1);
    return values.map((v, i) => ({
      x: PAD + i * stepX,
      y: PAD + (1 - (v - min) / span) * (height - 2 * PAD)
    }));
  });

  let line = $derived(points.map((p) => `${p.x},${p.y}`).join(' '));
  let area = $derived(points.length ? `${PAD},${height - PAD} ${line} ${W - PAD},${height - PAD}` : '');
</script>

<div class="spark" style="height:{height}px">
  {#if points.length}
    <svg viewBox="0 0 {W} {height}" preserveAspectRatio="none" role="img" aria-label={$t('portfolios.sparkline.trend')}>
      <polygon points={area} fill={color} opacity="0.1" />
      <polyline points={line} fill="none" stroke={color} stroke-width="1.5" vector-effect="non-scaling-stroke" />
    </svg>
  {:else}
    <div class="flat" aria-label={$t('portfolios.sparkline.notEnoughHistory')}></div>
  {/if}
</div>

<style>
  .spark {
    width: 100%;
  }
  svg {
    width: 100%;
    height: 100%;
    display: block;
  }
  .flat {
    width: 100%;
    height: 100%;
    background: repeating-linear-gradient(
      90deg,
      var(--border) 0 6px,
      transparent 6px 12px
    );
    opacity: 0.4;
    align-self: center;
    height: 1px;
    margin-top: calc(50% - 1px);
  }
</style>
