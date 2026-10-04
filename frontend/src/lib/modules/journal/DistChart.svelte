<script>
  // One distribution, drawn as a signed bar strip: net PnL per bucket above/below a zero
  // line, the trade count under each bar. Green above, red below, and the numbers are
  // written next to the shape, so the chart explains itself without a legend.
  //
  // Buckets come straight from `/api/journal/analytics`; `label` turns a bucket key into
  // the axis text (the caller owns the wording, this owns the geometry).
  import { fmtMoney, fmtPct } from './api.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let {
    buckets = [],
    currency = 'USD',
    label = (b) => b.key,
    // What the bar height encodes. 'net' = total PnL, 'avg' = mean PnL per trade,
    // 'trades' = how many trades fell in the bucket.
    metric = 'net',
    // Bars narrower than this get their count hidden (24-hour strips get crowded).
    compact = false,
    // Write one tick every N columns; a long histogram cannot label every bar.
    tickEvery = 1,
    height = 150
  } = $props();

  const val = (b) => (metric === 'avg' ? b.avg : metric === 'trades' ? b.trades : b.net);
  // A count is never negative, so a 'trades' strip is one-sided: the colour comes from
  // the bucket's own range instead (a bucket ending at or below zero is a loss bucket).
  const winning = (b) => (metric === 'trades' ? (b.hi ?? b.lo ?? 0) > 0 : val(b) > 0);
  const shown = $derived(buckets.filter((b) => b.trades > 0));
  // Scale on the largest magnitude so the zero line sits where the data puts it.
  const max = $derived(Math.max(1e-9, ...buckets.map((b) => Math.abs(val(b)))));
  const anyPos = $derived(buckets.some((b) => val(b) > 0));
  const anyNeg = $derived(buckets.some((b) => val(b) < 0));
  // A one-sided distribution should not waste half the box on empty space.
  // Unitless share of the plot height that sits above the zero line.
  const posShare = $derived(anyPos && anyNeg ? 0.5 : anyPos ? 1 : 0);

  const pct = (b) => (Math.abs(val(b)) / max) * 100;

  function tip(b) {
    const parts = [
      `${label(b)}`,
      $t('journal.analytics.chart.tipTrades', { count: b.trades }),
      $t('journal.analytics.chart.tipNet', { net: fmtMoney(b.net, currency) }),
      $t('journal.analytics.chart.tipAvg', { avg: fmtMoney(b.avg, currency) })
    ];
    if (b.win_rate != null) {
      parts.push($t('journal.analytics.chart.tipWinRate', { rate: fmtPct(b.win_rate) }));
    }
    return parts.join('\n');
  }
</script>

{#if shown.length === 0}
  <EmptyState compact icon="bar-chart" title={$t('journal.analytics.chart.empty')} />
{:else}
  <div class="chart" style="--h:{height}px; --pos:{posShare}">
    <div class="plot">
      {#each buckets as b, i (b.key)}
        <div class="col" class:muted={b.trades === 0} title={tip(b)}>
          <div class="up">
            {#if val(b) > 0}
              <span
                class="bar"
                class:pos={winning(b)}
                class:neg={!winning(b)}
                style="height:{pct(b)}%"
              ></span>
            {/if}
          </div>
          <div class="zero"></div>
          <div class="down">
            {#if val(b) < 0}
              <span class="bar neg" style="height:{pct(b)}%"></span>
            {/if}
          </div>
          <span class="tick">{i % tickEvery === 0 ? label(b) : ''}</span>
          {#if !compact}
            <span class="count">{b.trades}</span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .chart {
    width: 100%;
    overflow-x: auto;
  }
  .plot {
    display: flex;
    align-items: stretch;
    gap: 2px;
    min-width: 100%;
  }
  .col {
    flex: 1 1 0;
    min-width: 18px;
    display: grid;
    /* The plot rows are sized off --h in px and the axis rows are auto: the labels
       always get their own space, whatever the zero line does. Percentage rows would
       resolve against the whole column (labels included) and push the axis out of the
       box on a one-sided distribution. */
    grid-template-rows: calc(var(--h) * var(--pos)) 0 calc(var(--h) * (1 - var(--pos))) auto auto;
  }
  .up {
    display: flex;
    align-items: flex-end;
    justify-content: center;
  }
  .down {
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }
  .zero {
    border-top: 0.5px solid var(--border);
  }
  .bar {
    width: 100%;
    max-width: 34px;
    min-height: 1px;
    display: block;
  }
  .bar.pos {
    background: color-mix(in srgb, var(--green) 62%, transparent);
  }
  .bar.neg {
    background: color-mix(in srgb, var(--red) 62%, transparent);
  }
  .col:hover .bar.pos {
    background: var(--green);
  }
  .col:hover .bar.neg {
    background: var(--red);
  }
  .tick,
  .count {
    text-align: center;
    font-size: 10px;
    line-height: 1.4;
    white-space: nowrap;
  }
  .tick {
    color: var(--dim);
    padding-top: var(--space-1);
  }
  .count {
    color: var(--faint);
    font-family: var(--mono);
  }
  .col.muted .tick {
    color: var(--faint);
  }
</style>
