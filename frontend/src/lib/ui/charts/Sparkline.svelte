<script>
  // Trend sketch for a row or a tile: no axes, no hover. Toned by sign (green when the
  // last value is above the first, red below, muted when flat); a dashed hairline
  // stands in when there are fewer than two points, or `emptyText` when given.
  //
  //   <Sparkline values={closes} width={120} height={30} />
  //   <Sparkline values={history} width="100%" height={44} emptyText={$t('...notEnough')} />
  //
  // props: values (numbers, nulls skipped), width (px number or a CSS width such as
  //        '100%', default 120), height (px, default 30), tone ('sign' | 'accent' |
  //        'muted', default 'sign'), invert (down is good), area (10% wash, default
  //        false), endDot (default true), strokeWidth (default 1.5), emptyText, label
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { toneColor, fade } from './palette.js';

  let {
    values = [],
    width = 120,
    height = 30,
    tone = 'sign',
    invert = false,
    area = false,
    endDot = true,
    strokeWidth = 1.5,
    emptyText = '',
    label = ''
  } = $props();

  const c = $derived(chartColors());
  const fixed = $derived(typeof width === 'number');
  let measured = $state(0);
  const W = $derived(fixed ? width : measured);
  const PAD = 3;

  const pts = $derived(
    (values ?? []).map((v, i) => [i, v]).filter(([, v]) => typeof v === 'number' && Number.isFinite(v))
  );
  const enough = $derived(pts.length >= 2);
  const first = $derived(pts[0]?.[1]);
  const last = $derived(pts[pts.length - 1]?.[1]);
  const stroke = $derived(
    tone === 'accent' ? c.accent : tone === 'muted' ? c.muted : enough ? toneColor(last - first, c, invert) : c.muted
  );

  const coords = $derived.by(() => {
    if (!enough || !(W > 0)) return [];
    const ys = pts.map((p) => p[1]);
    const lo = Math.min(...ys);
    const hi = Math.max(...ys);
    const span = hi - lo || 1;
    const n = (values?.length ?? 1) - 1 || 1;
    return pts.map(([i, v]) => [
      PAD + (i / n) * (W - 2 * PAD),
      hi === lo ? height / 2 : PAD + (1 - (v - lo) / span) * (height - 2 * PAD)
    ]);
  });
  const line = $derived(coords.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join(''));
  const fill = $derived(
    area && coords.length
      ? `${line}L${coords[coords.length - 1][0].toFixed(1)},${height}L${coords[0][0].toFixed(1)},${height}Z`
      : ''
  );
  const end = $derived(coords[coords.length - 1]);
</script>

<span
  class="spark"
  style:width={fixed ? `${width}px` : width}
  style:height="{height}px"
  bind:clientWidth={measured}
  role="img"
  aria-label={label || undefined}
>
  {#if enough && W > 0}
    <svg width={W} {height} viewBox="0 0 {W} {height}" aria-hidden="true">
      {#if fill}<path d={fill} fill={fade(stroke, 0.1)} />{/if}
      <path d={line} fill="none" {stroke} stroke-width={strokeWidth} stroke-linejoin="round" stroke-linecap="round" />
      {#if endDot && end}
        <circle cx={end[0]} cy={end[1]} r="2.5" fill={stroke} stroke={c.surface} stroke-width="1.5" />
      {/if}
    </svg>
  {:else if !enough && emptyText}
    <span class="spark-empty">{emptyText}</span>
  {:else if !enough}
    <span class="flat" aria-hidden="true"></span>
  {/if}
</span>

<style>
  .spark {
    display: inline-flex;
    align-items: center;
    flex: none;
    max-width: 100%;
    min-width: 0;
    vertical-align: middle;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .flat {
    width: 100%;
    border-top: 1px dashed var(--faint);
  }
  .spark-empty {
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: center;
    color: var(--muted);
    font-size: 12px;
  }
</style>
