<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Compact alignment summary for a multi-asset run: merged-clock length, overlap window,
  // warm-up bars, and a per-asset row with its bar count and inactive (marked-to-market) bars.
  // Shown before running (the "alert the user if one starts before the other" — as data).
  let { alignment = null, loading = false } = $props();

  const fmtTs = (ts) => (ts ? String(ts).slice(0, 10) : '—');
</script>

{#if loading}
  <div class="banner mini"><span class="muted">{$t('backtest.align.checking')}</span></div>
{:else if alignment}
  <div class="banner">
    <div class="hdr">
      <Icon name="layers" size={13} />
      <span class="title">{$t('backtest.align.title')}</span>
      <span class="spacer"></span>
      <span class="stat">
        {$t('backtest.align.clock', { n: alignment.clock_len.toLocaleString() })}
      </span>
    </div>
    <div class="facts">
      <span class="fact">
        {$t('backtest.align.overlap', {
          from: fmtTs(alignment.overlap_from),
          to: fmtTs(alignment.overlap_to),
          n: alignment.overlap_bars.toLocaleString()
        })}
      </span>
      {#if alignment.warmup_bars > 0}
        <span class="fact warn">{$t('backtest.align.warmup', { n: alignment.warmup_bars })}</span>
      {/if}
    </div>
    <table>
      <thead>
        <tr>
          <th>{$t('backtest.align.asset')}</th>
          <th class="r">{$t('backtest.align.bars')}</th>
          <th class="r">{$t('backtest.align.range')}</th>
          <th class="r">{$t('backtest.align.inactive')}</th>
        </tr>
      </thead>
      <tbody>
        {#each alignment.assets as a (a.ticker)}
          <tr>
            <td class="tk">{a.ticker}</td>
            <td class="r num">{a.bars.toLocaleString()}</td>
            <td class="r rng">{fmtTs(a.first_ts)} → {fmtTs(a.last_ts)}</td>
            <td class="r num" class:warn={a.inactive_bars > 0}>{a.inactive_bars.toLocaleString()}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if !alignment.overlap_bars}
      <p class="err">{$t('backtest.align.noOverlap')}</p>
    {/if}
  </div>
{/if}

<style>
  .banner {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 40%, transparent);
    padding: var(--space-2) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .banner.mini {
    padding: var(--space-2) var(--space-3);
  }
  .hdr {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
  }
  .title {
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    color: var(--text);
  }
  .spacer {
    flex: 1;
  }
  .stat {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    color: var(--muted);
  }
  .fact.warn {
    color: var(--amber);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    padding: 2px var(--space-2);
    text-align: left;
  }
  th {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    font-weight: var(--fw-medium);
  }
  .r {
    text-align: right;
  }
  .tk {
    font-weight: var(--fw-medium);
  }
  .rng {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .num.warn {
    color: var(--amber);
  }
  .muted {
    color: var(--muted);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
  }
</style>
