<script>
  // The one chart tooltip: a raised surface (hairline, --shadow-1, --radius-sm) that
  // floats over a chart without catching the pointer. Positioned in the coordinates of
  // its nearest positioned ancestor (the chart wrapper sets `position: relative`).
  //
  //   <Tooltip x={px} y={py} width={wrapWidth} title="Mar 14, 2026"
  //            rows={[{ label: 'Net worth', value: '$12,400', color, change: '+4.10%' }]} />
  //
  // Values lead, labels follow: the reader already knows the series and wants the
  // number. Each row is keyed by a short stroke of its series colour (dashed for an
  // estimate or projection), never a filled box. Text is set as text, never as HTML:
  // series and category names come from user data.
  // props: x, y, width (container px, for edge clamping), title, rows, note, visible
  //   rows: [{ label, value, color?, dashed?, change?, changeTone?: 'up'|'down'|'flat' }]
  let { x = 0, y = 0, width = 0, title = '', rows = [], note = '', visible = true } = $props();

  let w = $state(0);
  let h = $state(0);
  const GAP = 12;

  // Centred above the point and clamped inside the container. With no room above, it
  // moves beside the point (right, or left near the right edge) so it never covers the
  // mark being read.
  const room = $derived(y - h - GAP >= 0);
  const maxLeft = $derived(Math.max(0, (width || x + w + GAP) - w));
  const left = $derived(
    room
      ? Math.max(0, Math.min(x - w / 2, maxLeft))
      : x + GAP + w <= (width || Infinity)
        ? x + GAP
        : Math.max(0, x - GAP - w)
  );
  const top = $derived(room ? y - h - GAP : Math.max(0, y - h / 2));
</script>

{#if visible && (title || rows.length || note)}
  <div
    class="chart-tip"
    style:left="{left}px"
    style:top="{top}px"
    bind:clientWidth={w}
    bind:clientHeight={h}
    role="status"
    aria-live="polite"
  >
    {#if title}<div class="tip-title">{title}</div>{/if}
    {#each rows as r, i (i)}
      <div class="tip-row">
        {#if r.color}
          <span class="key" class:dashed={r.dashed} style:--key={r.color} aria-hidden="true"></span>
        {/if}
        <span class="val">{r.value}</span>
        {#if r.change}<span class="chg {r.changeTone ?? 'flat'}">{r.change}</span>{/if}
        {#if r.label}<span class="lbl">{r.label}</span>{/if}
      </div>
    {/each}
    {#if note}<div class="tip-note">{note}</div>{/if}
  </div>
{/if}

<style>
  .chart-tip {
    position: absolute;
    z-index: var(--z-dropdown);
    pointer-events: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: max-content;
    max-width: 280px;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-1);
    white-space: nowrap;
  }
  .tip-title,
  .tip-note {
    color: var(--dim);
    font-family: var(--mono);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .tip-note {
    white-space: normal;
  }
  .tip-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .key {
    flex: none;
    align-self: center;
    width: 12px;
    border-top: 2px solid var(--key);
  }
  .key.dashed {
    border-top-style: dashed;
  }
  .val {
    color: var(--text);
    font-family: var(--mono);
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .chg {
    font-family: var(--mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .chg.up {
    color: var(--green);
  }
  .chg.down {
    color: var(--red);
  }
  .chg.flat {
    color: var(--muted);
  }
  .lbl {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: 11px;
  }
</style>
