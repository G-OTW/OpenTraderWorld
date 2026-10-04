<script>
  // A ranked list as label + proportional bar + value: PnL by strategy, spend by category,
  // top projects, success rate per workflow. Bars are scaled to the largest magnitude, so
  // the longest bar always fills its track and the rest read against it.
  //
  // Hovering a row lifts it and reads it in full: the label is often truncated in the
  // column, and the tooltip adds the row's share of the series.
  import { tip } from '$lib/ui/tip.svelte.js';

  let {
    rows = [],             // [{ label, value, display?, color?, tone?: 'pos'|'neg' }]
    signed = false,        // a signed series colours by sign instead of by rank
    valueFormat = (v) => String(v),
    // "x% of total" only reads right for an additive series (spend, exposure, hours);
    // a series of rates or ratios must leave it off.
    showShare = false
  } = $props();

  const max = $derived(Math.max(1, ...rows.map((r) => Math.abs(Number(r.value) || 0))));
  const total = $derived(rows.reduce((sum, r) => sum + Math.abs(Number(r.value) || 0), 0));

  let hover = $state(-1);

  function enter(event, r, i) {
    hover = i;
    const share = total > 0 ? (Math.abs(Number(r.value) || 0) / total) * 100 : 0;
    tip.show(event, {
      title: r.label,
      rows: [
        {
          color: r.color,
          label: showShare && total > 0 ? `${share.toFixed(1)}%` : '',
          value: r.display ?? valueFormat(r.value),
          tone: r.tone ?? (signed ? (Number(r.value) < 0 ? 'neg' : 'pos') : '')
        }
      ]
    });
  }
</script>

<ul class="ranks" onpointerleave={() => { hover = -1; tip.hide(); }}>
  {#each rows as r, i (r.label ?? i)}
    {@const pct = (Math.abs(Number(r.value) || 0) / max) * 100}
    {@const tone = r.tone ?? (signed ? (Number(r.value) < 0 ? 'neg' : 'pos') : '')}
    <li
      class:on={hover === i}
      class:dim={hover >= 0 && hover !== i}
      onpointerenter={(e) => enter(e, r, i)}
      onpointermove={(e) => tip.move(e)}
    >
      <span class="w-name">{r.label}</span>
      <span class="track">
        <span
          class="fill"
          class:neg={tone === 'neg'}
          class:pos={tone === 'pos'}
          style:width={`${pct}%`}
          style:background={r.color}
        ></span>
      </span>
      <span class="w-num" class:w-pos={tone === 'pos'} class:w-neg={tone === 'neg'}>
        {r.display ?? valueFormat(r.value)}
      </span>
    </li>
  {/each}
</ul>

<style>
  .ranks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 9px;
    min-width: 0;
  }
  .ranks li {
    display: grid;
    grid-template-columns: minmax(56px, 88px) 1fr auto;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-sm);
    min-width: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .ranks li.dim {
    opacity: 0.5;
  }
  .w-name {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  li.on .w-name {
    color: var(--text);
  }
  .track {
    height: 8px;
    border-radius: 999px;
    background: var(--surface-3);
    overflow: hidden;
    min-width: 0;
  }
  li.on .track {
    background: var(--border);
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--chart-1);
    transition: width var(--dur-base) var(--ease);
  }
  .fill.pos {
    background: var(--green);
  }
  .fill.neg {
    background: var(--red);
  }
  .w-num {
    font-size: var(--text-xs);
    color: var(--text);
  }
</style>
