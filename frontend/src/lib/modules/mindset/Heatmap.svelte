<script>
  // Questions by day: one row per scored prompt, one cell per logged day, a single hue
  // from light (worst) to dark (best). Replaces a grid of per-question line charts: the
  // question that drags the score down is the pale row, and whether it improves reads
  // left to right.
  import { t } from '$lib/i18n';
  import { roundOf } from './scoring.js';

  let { questions = [], days = [], templateName = () => '', onpick = null } = $props();

  const cols = $derived(days.map((d) => d.date));
  // Label at most ~8 columns so a long window does not print every date.
  const every = $derived(Math.max(1, Math.ceil(cols.length / 8)));

  const shade = (v) => `color-mix(in srgb, var(--chart-1) ${Math.round(12 + v * 0.88)}%, var(--surface))`;
</script>

<div class="wrap">
  <div class="grid" style:--n={cols.length}>
    <span class="corner"></span>
    {#each cols as c, i (c)}
      <span class="col" class:lbl={i % every === 0}>{i % every === 0 ? c.slice(5) : ''}</span>
    {/each}
    {#each questions as q (q.prompt.id)}
      <span class="row" title="{templateName(q.prompt)}: {q.prompt.label}">
        <span class="qt">{templateName(q.prompt)}</span>
        <span class="ql">{q.prompt.label}</span>
      </span>
      {#each cols as c (c)}
        {@const cell = q.byDate.get(c)}
        {#if cell}
          <button
            class="cell"
            style:background={shade(cell.value)}
            title="{c}: {roundOf(cell.value)} ({cell.raw})"
            aria-label="{q.prompt.label} {c}: {roundOf(cell.value)}"
            onclick={() => onpick?.(c)}
          ></button>
        {:else}
          <span class="cell empty"></span>
        {/if}
      {/each}
    {/each}
  </div>
  <div class="legend">
    <span>{$t('mindset.history.heatmapLow')}</span>
    <span class="ramp"></span>
    <span>{$t('mindset.history.heatmapHigh')}</span>
  </div>
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow-x: auto;
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(180px, 260px) repeat(var(--n), minmax(12px, 1fr));
    gap: 2px;
    align-items: stretch;
    min-width: calc(180px + var(--n) * 14px);
  }
  .col {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--dim);
    text-align: center;
    overflow: visible;
    white-space: nowrap;
    min-width: 0;
  }
  .row {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
    padding-right: var(--space-2);
  }
  .qt {
    font-size: 10px;
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ql {
    font-size: var(--text-xs);
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cell {
    display: block;
    min-height: 22px;
    height: 100%;
    border: none;
    border-radius: 2px;
    padding: 0;
    cursor: pointer;
  }
  .cell:hover,
  .cell:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 0;
  }
  .cell.empty {
    background: transparent;
    border: var(--hairline) dashed var(--grid-line);
    cursor: default;
  }
  .legend {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .ramp {
    width: 120px;
    height: 8px;
    border-radius: 2px;
    background: linear-gradient(
      90deg,
      color-mix(in srgb, var(--chart-1) 12%, var(--surface)),
      var(--chart-1)
    );
  }
</style>
