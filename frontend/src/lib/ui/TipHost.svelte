<script>
  // Renders the chart hover readout. Mount once at the root, like ToastHost.
  // Fixed to the viewport and flipped near an edge so the box is never clipped and never
  // sits under the cursor.
  import { tip } from '$lib/ui/tip.svelte.js';

  const GAP = 14;

  // A readout follows the pointer, so anything that moves the page out from under it
  // (a scroll, a click, the window losing focus) has to close it.
  $effect(() => {
    const hide = () => tip.hide();
    window.addEventListener('scroll', hide, true);
    window.addEventListener('pointerdown', hide, true);
    window.addEventListener('blur', hide);
    return () => {
      window.removeEventListener('scroll', hide, true);
      window.removeEventListener('pointerdown', hide, true);
      window.removeEventListener('blur', hide);
    };
  });

  let w = $state(0);
  let h = $state(0);

  const pos = $derived.by(() => {
    const vw = typeof window === 'undefined' ? 0 : window.innerWidth;
    const vh = typeof window === 'undefined' ? 0 : window.innerHeight;
    let left = tip.x + GAP;
    let top = tip.y + GAP;
    if (left + w > vw - 8) left = tip.x - GAP - w;
    if (top + h > vh - 8) top = tip.y - GAP - h;
    return { left: Math.max(8, left), top: Math.max(8, top) };
  });
</script>

{#if tip.open && (tip.title || tip.rows.length)}
  <div
    class="tip"
    role="tooltip"
    bind:clientWidth={w}
    bind:clientHeight={h}
    style:left={`${pos.left}px`}
    style:top={`${pos.top}px`}
  >
    {#if tip.title}<span class="ttl">{tip.title}</span>{/if}
    {#if tip.rows.length}
      <ul>
        {#each tip.rows as r, i (r.label ?? i)}
          <li>
            {#if r.color}<span class="dot" style:background={r.color}></span>{/if}
            {#if r.label}<span class="lab">{r.label}</span>{/if}
            <span class="val" class:pos={r.tone === 'pos'} class:neg={r.tone === 'neg'}
              class:warn={r.tone === 'warn'}>{r.value}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .tip {
    position: fixed;
    z-index: var(--z-toast);
    pointer-events: none;
    max-width: 260px;
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .ttl {
    font-size: var(--text-xs);
    color: var(--dim);
    line-height: var(--lh-tight);
    white-space: nowrap;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    line-height: var(--lh-tight);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .lab {
    color: var(--muted);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .val {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .warn {
    color: var(--amber);
  }
</style>
