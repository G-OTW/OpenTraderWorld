<script>
  // Two-thumb range slider: a min and a max on one track. Two native range inputs stacked on
  // the same track, only their thumbs taking the pointer, so keyboard and screen readers get
  // the platform control for free.
  //
  //   <RangeSlider label="Sharpe" max={200} bind:lo bind:hi loText="0.4" hiText="2.1" />
  // props: lo, hi (bindable, integer positions in [min, max]), min, max, step, label,
  //        loText / hiText (what the positions mean, shown under the label), oninput
  let {
    lo = $bindable(0),
    hi = $bindable(100),
    min = 0,
    max = 100,
    step = 1,
    label = '',
    loText = '',
    hiText = '',
    disabled = false,
    oninput = null
  } = $props();

  const span = $derived(Math.max(1e-9, max - min));
  const left = $derived(((lo - min) / span) * 100);
  const right = $derived(((hi - min) / span) * 100);
  const narrowed = $derived(lo > min || hi < max);

  // The thumbs never cross: the one dragged stops at the other.
  function setLo(e) {
    lo = Math.min(Number(e.currentTarget.value), hi);
    e.currentTarget.value = String(lo);
    oninput?.(lo, hi);
  }
  function setHi(e) {
    hi = Math.max(Number(e.currentTarget.value), lo);
    e.currentTarget.value = String(hi);
    oninput?.(lo, hi);
  }
</script>

<div class="range" class:narrowed>
  <div class="head">
    <span class="label">{label}</span>
    <span class="vals">{loText} – {hiText}</span>
  </div>
  <div class="track">
    <div class="rail"></div>
    <div class="sel" style:left="{left}%" style:right="{100 - right}%"></div>
    <input
      type="range"
      {min}
      {max}
      {step}
      value={lo}
      {disabled}
      aria-label={`${label} min`}
      oninput={setLo}
      class:top={lo >= max}
    />
    <input type="range" {min} {max} {step} value={hi} {disabled} aria-label={`${label} max`} oninput={setHi} />
  </div>
</div>

<style>
  .range {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-2);
    font-size: 11px;
    min-width: 0;
  }
  .label {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .vals {
    color: var(--dim);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .narrowed .vals {
    color: var(--text);
  }
  .track {
    position: relative;
    height: 18px;
  }
  .rail,
  .sel {
    position: absolute;
    top: 50%;
    height: 3px;
    transform: translateY(-50%);
    border-radius: 2px;
  }
  .rail {
    left: 0;
    right: 0;
    background: var(--surface-3);
  }
  .sel {
    background: var(--accent);
  }
  input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0;
    background: transparent;
    border: 0;
    appearance: none;
    -webkit-appearance: none;
    pointer-events: none;
  }
  /* A min thumb parked at the far right would sit under the max one for good: lift it. */
  input.top {
    z-index: 1;
  }
  input:focus-visible {
    outline: none;
  }
  input::-webkit-slider-runnable-track {
    background: transparent;
    border: 0;
  }
  input::-moz-range-track {
    background: transparent;
    border: 0;
  }
  input::-webkit-slider-thumb {
    -webkit-appearance: none;
    pointer-events: auto;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--surface);
    border: 1.5px solid var(--accent);
    cursor: grab;
  }
  input::-moz-range-thumb {
    pointer-events: auto;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--surface);
    border: 1.5px solid var(--accent);
    cursor: grab;
  }
  input:focus-visible::-webkit-slider-thumb {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  input:focus-visible::-moz-range-thumb {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  input:disabled::-webkit-slider-thumb {
    border-color: var(--border-control);
    cursor: default;
  }
</style>
