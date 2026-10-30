<script>
  // A single-value progress ring (goal completion, budget usage). Same ring geometry as
  // Donut, but one arc against its track and a percentage in the hole. Hovering reads the
  // exact figure, plus any extra rows the caller passes (done / total, spent / budget…).
  import { tip } from '$lib/ui/tip.svelte.js';

  let {
    percent = 0,
    size = 96,
    thickness = 12,
    tone = 'pos',          // 'pos' | 'accent' | 'warn' | 'neg'
    label = '',
    rows = []              // extra tooltip rows: [{ label, value, tone? }]
  } = $props();

  const R = 50;
  const C = 2 * Math.PI * R;
  const pct = $derived(Math.max(0, Math.min(100, Number(percent) || 0)));
  const len = $derived((pct / 100) * C);

  function enter(event) {
    tip.show(event, {
      title: label,
      rows: [{ label: '', value: `${pct.toFixed(1)}%`, tone: tone === 'accent' ? '' : tone }, ...rows]
    });
  }
</script>

<div
  class="gauge {tone}"
  style:width={`${size}px`}
  style:height={`${size}px`}
  onpointerenter={enter}
  onpointermove={(e) => tip.move(e)}
  onpointerleave={() => tip.hide()}
  role="img"
  aria-label={label || `${Math.round(pct)}%`}
>
  <svg viewBox="0 0 120 120" aria-hidden="true">
    <circle class="track" cx="60" cy="60" r={R} fill="none" stroke-width={thickness} />
    <circle class="arc" cx="60" cy="60" r={R} fill="none" stroke-width={thickness}
      stroke-linecap="round" stroke-dasharray={`${len} ${C - len}`}
      transform="rotate(-90 60 60)" />
  </svg>
  <span class="pct">{Math.round(pct)}%</span>
</div>

<style>
  .gauge {
    position: relative;
    flex-shrink: 0;
  }
  .gauge svg {
    width: 100%;
    height: 100%;
    display: block;
  }
  .track {
    stroke: var(--surface-3);
  }
  .arc {
    stroke: var(--accent);
    transition: stroke-dasharray var(--dur-base) var(--ease);
  }
  .pos .arc {
    stroke: var(--green);
  }
  .warn .arc {
    stroke: var(--amber);
  }
  .neg .arc {
    stroke: var(--red);
  }
  .pct {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 17px;
    font-weight: var(--fw-medium);
    color: var(--text);
    pointer-events: none;
  }
</style>
