<script>
  // A line/area sparkline for an equity curve or a trend. Fixed viewBox with
  // preserveAspectRatio="none": the path stretches to whatever cell it lands in, and
  // vector-effect keeps the stroke one pixel at any stretch.
  //
  // Hovering reads the series: a crosshair snaps to the nearest point and the app tooltip
  // shows its label and value. The crosshair and the dot are HTML, not SVG, because the
  // non-uniform stretch would squash a circle into an ellipse.
  import { tip } from '$lib/ui/tip.svelte.js';
  import { fmtNum } from '$lib/format';

  let {
    values = [],
    tone = 'auto',         // 'auto' (last vs first) | 'pos' | 'neg' | 'accent'
    height = 48,
    area = true,
    label = 'trend',
    labels = [],           // one x label per value (dates, periods…)
    valueFormat = (v) => fmtNum(v, 2)
  } = $props();

  const nums = $derived(values.map(Number).filter((v) => Number.isFinite(v)));
  const dir = $derived(
    tone !== 'auto' ? tone : nums.at(-1) >= nums[0] ? 'pos' : 'neg'
  );

  const bounds = $derived.by(() => {
    const min = Math.min(...nums);
    const max = Math.max(...nums);
    return { min, span: max - min || 1 };
  });

  const path = $derived.by(() => {
    if (nums.length < 2) return '';
    return nums
      .map((v, i) => {
        const x = (i / (nums.length - 1)) * 100;
        const y = 100 - ((v - bounds.min) / bounds.span) * 100;
        return `${i ? 'L' : 'M'}${x.toFixed(2)} ${y.toFixed(2)}`;
      })
      .join(' ');
  });
  const fill = $derived(path ? `${path} L100 100 L0 100 Z` : '');
  const uid = `spk${Math.random().toString(36).slice(2, 8)}`;

  let hover = $state(-1);
  const point = $derived.by(() => {
    if (hover < 0 || hover >= nums.length) return null;
    const v = nums[hover];
    return {
      v,
      left: nums.length > 1 ? (hover / (nums.length - 1)) * 100 : 50,
      top: 100 - ((v - bounds.min) / bounds.span) * 100
    };
  });

  function at(event) {
    const rect = event.currentTarget.getBoundingClientRect();
    if (!rect.width || nums.length === 0) return;
    const frac = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
    hover = Math.round(frac * (nums.length - 1));
    tip.show(event, {
      title: labels[hover] ?? label,
      rows: [{ label: labels.length ? label : '', value: valueFormat(nums[hover]) }]
    });
  }
  function leave() {
    hover = -1;
    tip.hide();
  }
</script>

<div
  class="sparkbox {dir}"
  style:height={height ? `${height}px` : '100%'}
  onpointermove={at}
  onpointerleave={leave}
  role="img"
  aria-label={label}
>
  <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
    {#if path}
      <defs>
        <linearGradient id={uid} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" class="g0" />
          <stop offset="100%" class="g1" />
        </linearGradient>
      </defs>
      {#if area}<path d={fill} fill={`url(#${uid})`} stroke="none" />{/if}
      <path d={path} fill="none" stroke="currentColor" stroke-width="1.5"
        stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
    {/if}
  </svg>
  {#if point}
    <span class="cross" style:left={`${point.left}%`}></span>
    <span class="pin" style:left={`${point.left}%`} style:top={`${point.top}%`}></span>
  {/if}
</div>

<style>
  .sparkbox {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .accent {
    color: var(--accent);
  }
  .g0 {
    stop-color: currentColor;
    stop-opacity: 0.22;
  }
  .g1 {
    stop-color: currentColor;
    stop-opacity: 0;
  }
  .cross {
    position: absolute;
    top: 0;
    bottom: 0;
    width: var(--hairline);
    background: var(--border-strong);
    pointer-events: none;
  }
  .pin {
    position: absolute;
    width: 7px;
    height: 7px;
    margin: -3.5px 0 0 -3.5px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 0 2px var(--surface);
    pointer-events: none;
  }
</style>
