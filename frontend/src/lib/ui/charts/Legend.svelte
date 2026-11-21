<script>
  // Legend for 2+ series: a mark beside each name (a line key for lines, a square
  // swatch for bars and areas), text in text tokens, never in the series colour.
  // With `onToggle`, each entry is a button: clicking it emphasises that series (the
  // others fade) and clicking it again clears the emphasis.
  //
  // `layout="inline"` wraps entries on one line under a chart; `layout="rows"` lists
  // name, value and share as aligned columns (part-to-whole charts).
  // Figures arrive already formatted (and masked by the caller under the privacy screen).
  // props: items [{ key?, label, color, kind?: 'line'|'rect', dashed?, value?, share? }],
  //        layout ('inline'|'rows'), active (emphasised key), onToggle(key), onHover(key|null)
  let { items = [], layout = 'inline', active = null, onToggle = null, onHover = null } = $props();

  const keyOf = (it, i) => it.key ?? it.label ?? i;
</script>

{#if items.length}
  <ul class="chart-legend {layout}" class:interactive={!!onToggle}>
    {#each items as it, i (keyOf(it, i))}
      {@const k = keyOf(it, i)}
      {@const faded = active != null && active !== k}
      <li class:faded>
        {#if onToggle}
          <button
            type="button"
            class="entry"
            aria-pressed={active === k}
            onclick={() => onToggle(active === k ? null : k)}
            onmouseenter={() => onHover?.(k)}
            onmouseleave={() => onHover?.(null)}
            onfocus={() => onHover?.(k)}
            onblur={() => onHover?.(null)}
          >
            {@render body(it)}
          </button>
        {:else}
          <span class="entry">{@render body(it)}</span>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

{#snippet body(it)}
  <span
    class="mark {it.kind === 'line' ? 'line' : 'rect'}"
    class:dashed={it.dashed}
    style:--mark={it.color}
    aria-hidden="true"
  ></span>
  <span class="name" title={it.label}>{it.label}</span>
  {#if it.value != null && it.value !== ''}<span class="value">{it.value}</span>{/if}
  {#if layout === 'rows' && it.share != null}<span class="share">{it.share}</span>{/if}
{/snippet}

<style>
  .chart-legend {
    list-style: none;
    margin: 0;
    padding: 0;
    min-width: 0;
  }
  .chart-legend.inline {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-4);
  }
  .chart-legend.rows {
    display: flex;
    flex-direction: column;
  }
  li {
    min-width: 0;
    transition: opacity 160ms ease;
  }
  li.faded {
    opacity: 0.45;
  }
  .entry {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    width: 100%;
    padding: 0;
    background: none;
    border: 0;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
  }
  .rows .entry {
    min-height: 28px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
  }
  button.entry {
    cursor: pointer;
    transition: background-color 160ms ease;
  }
  .rows button.entry:hover,
  .rows button.entry[aria-pressed='true'] {
    background: var(--surface-hover);
  }
  .inline button.entry:hover .name {
    color: var(--text);
  }
  .mark {
    flex: none;
  }
  .mark.rect {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--mark);
  }
  .mark.line {
    width: 14px;
    border-top: 2px solid var(--mark);
  }
  .mark.line.dashed {
    border-top-style: dashed;
  }
  .mark.rect.dashed {
    background: transparent;
    border: 1px dashed var(--mark);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rows .name {
    flex: 1 1 auto;
    color: var(--text);
  }
  .value,
  .share {
    flex: none;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .inline .value {
    color: var(--text);
  }
  .rows .value {
    color: var(--text);
    min-width: 72px;
  }
  .share {
    color: var(--muted);
    min-width: 48px;
  }
</style>
