<script>
  let { rows = [], line = '', format = String, label = '' } = $props();
  const vals = $derived(rows.map((r) => r.lines?.[line]));
  const upper = $derived(Math.max(0, ...vals.filter(Number.isFinite)));
  const lower = $derived(Math.min(0, ...vals.filter(Number.isFinite)));
  const span = $derived(upper - lower || 1);
  const zero = $derived(upper / span * 100);
  let selected = $state('');
</script>

<div class="statement-chart" role="group" aria-label={label}>
  <div class="plot">
    <span class="zero" style:top={`${zero}%`}></span>
    {#each rows as r, i (r.period)}
      {@const v = vals[i]}
      {@const text = `${r.period}: ${Number.isFinite(v) ? format(v) : '—'}`}
      <button type="button" class="slot" aria-label={text} title={text}
        onfocus={() => selected = text} onpointerenter={() => selected = text} onclick={() => selected = text}>
        {#if Number.isFinite(v)}
          <span class="bar" class:negative={v < 0} style:top={`${v < 0 ? zero : zero - v / span * 100}%`}
            style:height={`${Math.abs(v) / span * 100}%`}></span>
        {:else}<span class="gap">—</span>{/if}
      </button>
    {/each}
  </div>
  <div class="periods">{#each rows as r (r.period)}<span title={r.period}>{r.period.replace('TTM · ', '').replace(' FY', '\n')}</span>{/each}</div>
  <div class="readout" aria-live="polite">{selected || `${rows.at(-1)?.period ?? ''}: ${format(vals.at(-1))}`}</div>
</div>

<style>
  .statement-chart { display: flex; flex-direction: column; gap: 6px; width: 100%; min-height: 118px; }
  .plot { position: relative; display: flex; gap: 4px; height: 76px; }
  .zero { position: absolute; left: 0; right: 0; border-top: 1px solid var(--border-strong); }
  .slot { flex: 1; position: relative; min-width: 0; border: 0; padding: 0; background: transparent; cursor: pointer; }
  .slot:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .bar { position: absolute; left: 0; right: 0; background: var(--accent); border-radius: 2px; }
  .negative { background: var(--red); }
  .gap { color: var(--muted); font-size: 11px; }
  .periods { display: flex; gap: 4px; color: var(--muted); font: 10px var(--mono); text-align: center; }
  .periods span { flex: 1; min-width: 0; white-space: pre-line; overflow: hidden; }
  .readout { color: var(--muted); font-size: 11px; min-height: 16px; }
  @container widget (max-width: 360px) {
    .periods { justify-content: space-between; }
    .periods span { display: none; }
    .periods span:first-child, .periods span:last-child { display: block; flex: none; }
  }
</style>
