<script>
  // 52-week position: a 4px track with a 6px marker where the price sits between the
  // yearly low and high. The low and high print under the track on row hover. Drawn only
  // when the source served the range (or a full year of stored candles did); otherwise
  // the cell says n/a instead of a bar it could not measure.
  import { rangePosition, shortPrice } from './views.js';
  import { fmtQuote } from './api.js';
  import { t } from '$lib/i18n';

  let { price = null, low = null, high = null } = $props();

  const pos = $derived(rangePosition(price, low, high));
</script>

{#if pos == null}
  <span class="na" title={$t('watchlists.col.notServed')}>{$t('watchlists.col.na')}</span>
{:else}
  <span
    class="range"
    title={$t('watchlists.col.rangeTitle', { low: fmtQuote(low), high: fmtQuote(high), pct: Math.round(pos * 100) })}
  >
    <span class="track"><span class="mark" style:left="{pos * 100}%"></span></span>
    <span class="ends">
      <span>{shortPrice(low, fmtQuote)}</span>
      <span>{shortPrice(high, fmtQuote)}</span>
    </span>
  </span>
{/if}

<style>
  .range {
    position: relative;
    display: inline-flex;
    flex-direction: column;
    justify-content: center;
    width: 104px;
    height: 28px;
    vertical-align: middle;
  }
  .track {
    position: relative;
    width: 100%;
    height: 4px;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
  }
  .mark {
    position: absolute;
    top: 50%;
    width: 6px;
    height: 6px;
    margin: -3px 0 0 -3px;
    border-radius: 50%;
    background: var(--accent);
  }
  .ends {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    justify-content: space-between;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
    line-height: 1;
    color: var(--dim);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  :global(tr:hover) .ends,
  .range:hover .ends {
    opacity: 1;
  }
  .na {
    color: var(--muted);
    font-size: 12px;
  }
  @media (hover: none) {
    .ends {
      opacity: 1;
    }
  }
</style>
