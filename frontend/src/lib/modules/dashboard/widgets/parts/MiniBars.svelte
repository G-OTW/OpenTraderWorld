<script>
  // A column sparkline: the backlog histogram, monthly spend bars, gross win/loss bars.
  // Values only need to be comparable to each other; there is no axis, so a bar carries
  // its meaning from the figure it sits under.
  //
  // Hovering a column lifts it out of the series and reads it in the app tooltip; pass
  // `labels` so the readout can name the column instead of falling back to the series.
  import { tip } from '$lib/ui/tip.svelte.js';
  import { fmtNum } from '$lib/format';

  let {
    values = [],
    tone = '',             // '' | 'pos' | 'neg' | 'accent' | 'signed'
    height = 56,
    gap = 2,
    label = 'trend',
    labels = [],
    valueFormat = (v) => fmtNum(v, 2)
  } = $props();

  const max = $derived(Math.max(1, ...values.map((v) => Math.abs(Number(v) || 0))));
  let hover = $state(-1);

  function enter(event, i) {
    hover = i;
    const v = Number(values[i]) || 0;
    tip.show(event, {
      title: labels[i] ?? label,
      rows: [
        {
          label: labels.length ? label : '',
          value: valueFormat(values[i]),
          tone: tone === 'signed' ? (v < 0 ? 'neg' : 'pos') : ''
        }
      ]
    });
  }
  function leave() {
    hover = -1;
    tip.hide();
  }
</script>

<div class="bars" style:height={height ? `${height}px` : '100%'} style:gap={`${gap}px`}
  role="group" aria-label={label} onpointerleave={leave}>
  {#each values as v, i (i)}
    {@const mag = (Math.abs(Number(v) || 0) / max) * 100}
    <span
      class="slot"
      role="img"
      aria-label={`${labels[i] ? `${labels[i]}: ` : ''}${valueFormat(v)}`}
      onpointerenter={(e) => enter(e, i)}
      onpointermove={(e) => tip.move(e)}
    >
      <span
        class="bar"
        class:pos={tone === 'pos' || (tone === 'signed' && Number(v) >= 0)}
        class:neg={tone === 'neg' || (tone === 'signed' && Number(v) < 0)}
        class:accent={tone === 'accent'}
        class:dim={hover >= 0 && hover !== i}
        style:height={`${Math.max(6, mag)}%`}
      ></span>
    </span>
  {/each}
</div>

<style>
  .bars {
    display: flex;
    align-items: flex-end;
    min-width: 0;
  }
  /* The hit area is the full column height, so a two-pixel bar is still hoverable. */
  .slot {
    flex: 1;
    min-width: 2px;
    height: 100%;
    display: flex;
    align-items: flex-end;
  }
  .bar {
    width: 100%;
    border-radius: 2px 2px 0 0;
    background: var(--surface-3);
    transition: opacity var(--dur-fast) var(--ease);
  }
  .bar.dim {
    opacity: 0.45;
  }
  .bar.accent {
    background: var(--accent);
  }
  .bar.pos {
    background: var(--green);
  }
  .bar.neg {
    background: var(--red);
  }
</style>
