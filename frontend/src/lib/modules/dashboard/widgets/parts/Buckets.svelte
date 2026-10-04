<script>
  // A row of 2-4 counts split by hairlines: overdue / due today / due soon, ok / failed /
  // timeout, queued / running / failed. One number per cell, its label under it, and an
  // optional share line: the mockups' three-up stat strip.
  //
  // Hovering a cell lights it, and reads `tip` (a sentence the cell can't fit) when the
  // caller supplies one.
  import { tip as tipHost } from '$lib/ui/tip.svelte.js';

  let {
    // [{ value, label, tone?: 'pos'|'neg'|'warn'|'accent', sub?, href?, tip? }]
    cells = [],
    align = 'center'
  } = $props();

  let hover = $state(-1);

  // The cell already shows its number and label, so a readout only earns its place when
  // the caller passes a `tip` line the strip could not fit.
  function enter(event, c, i) {
    hover = i;
    if (!c.tip) return;
    tipHost.show(event, { title: c.label, rows: [{ label: '', value: c.tip }] });
  }
</script>

<ul class="buckets" style:text-align={align} onpointerleave={() => { hover = -1; tipHost.hide(); }}>
  {#each cells as c, i (c.label ?? i)}
    <li class:dim={hover >= 0 && hover !== i}>
      <svelte:element
        this={c.href ? 'a' : 'div'}
        class="cell"
        href={c.href}
        role={c.href ? undefined : 'img'}
        aria-label={c.href ? undefined : `${c.label}: ${c.value}`}
        onpointerenter={(e) => enter(e, c, i)}
        onpointermove={(e) => tipHost.move(e)}
      >
        <span class="val" class:w-pos={c.tone === 'pos'} class:w-neg={c.tone === 'neg'}
          class:w-warn={c.tone === 'warn'} class:accent={c.tone === 'accent'}>{c.value}</span>
        <span class="lab">{c.label}</span>
        {#if c.sub}<span class="sub" class:w-pos={c.tone === 'pos'} class:w-neg={c.tone === 'neg'}
          class:w-warn={c.tone === 'warn'} class:accent={c.tone === 'accent'}>{c.sub}</span>{/if}
      </svelte:element>
    </li>
  {/each}
</ul>

<style>
  .buckets {
    display: flex;
    list-style: none;
    margin: 0;
    padding: 0;
    min-width: 0;
  }
  .buckets > li {
    flex: 1;
    min-width: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .buckets > li.dim {
    opacity: 0.5;
  }
  /* Separators between cells only, so the strip never reads as a box inside the card. */
  .buckets > li + li {
    border-left: var(--hairline) solid var(--border);
  }
  .cell {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: 0 var(--space-2);
    text-decoration: none;
    min-width: 0;
  }
  a.cell:hover .lab {
    color: var(--text);
  }
  .val {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 26px;
    font-weight: var(--fw-medium);
    line-height: 1.1;
    letter-spacing: -0.015em;
    color: var(--text);
  }
  .accent {
    color: var(--accent);
  }
  .lab {
    font-size: var(--text-xs);
    color: var(--dim);
    line-height: var(--lh-tight);
    text-align: center;
  }
  .sub {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
    color: var(--dim);
  }
</style>
