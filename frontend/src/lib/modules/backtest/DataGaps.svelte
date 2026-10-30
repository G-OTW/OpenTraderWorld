<script>
  // Holes in the candles a run read, and the trades they cut through. The engine never trades
  // across a hole: a position open when one starts is left out of the results, listed here.
  // A hole is filled by a download (asked here for a backtest, automatic for a paper
  // session); one the provider has nothing for is confirmed and stays excluded.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let { gaps = [], excluded = [], onfill = null } = $props();

  let busy = $state(false);
  let error = $state('');
  let open = $state(false);

  const when = (v) => (v ? new Date(v).toLocaleString() : '—');
  const missing = $derived(gaps.filter((g) => g.status === 'missing'));

  async function fill() {
    busy = true;
    error = '';
    try {
      await onfill(missing);
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

{#if gaps.length || excluded.length}
  <section class="gaps" role="status">
    <div class="head">
      <Icon name="alert-triangle" size={14} />
      <b>{$t('backtest.gaps.title', { n: gaps.length })}</b>
      {#if excluded.length}<span>{$t('backtest.gaps.excluded', { n: excluded.length })}</span>{/if}
      {#if onfill && missing.length}
        <button class="fill" disabled={busy} onclick={fill}>{$t('backtest.gaps.fill')}</button>
      {/if}
    </div>
    <ul>
      {#each gaps as g (g.dataset_id + g.from)}
        <li>
          {#if g.ticker}<b>{g.ticker}</b>{/if}
          <span>{when(g.from)} → {when(g.to)}</span>
          <span class="status {g.status}">{$t(`backtest.gaps.status.${g.status ?? 'missing'}`)}</span>
        </li>
      {/each}
    </ul>
    {#if excluded.length}
      <button class="toggle" onclick={() => (open = !open)} aria-expanded={open}>
        {$t(open ? 'backtest.gaps.hideTrades' : 'backtest.gaps.showTrades')}
      </button>
      {#if open}
        <ul>
          {#each excluded as x (x.ticker + x.entry_ts)}
            <li>
              {#if x.ticker}<b>{x.ticker}</b>{/if}
              <span>{$t(`backtest.gaps.side.${x.direction}`)}</span>
              <span>{when(x.entry_ts)} → {when(x.exit_ts)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  </section>
{/if}

<style>
  .gaps {
    border: var(--hairline) solid var(--amber);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    color: var(--amber);
  }
  .head b {
    font-weight: 600;
  }
  .head span {
    color: var(--muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-variant-numeric: tabular-nums;
  }
  li b {
    color: var(--text);
    font-weight: 500;
  }
  .status.filling {
    color: var(--accent);
  }
  .status.confirmed {
    color: var(--red);
  }
  .fill,
  .toggle {
    font: inherit;
    cursor: pointer;
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent);
  }
  .fill {
    margin-left: auto;
    border: var(--hairline) solid var(--accent);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
  }
  .fill:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .toggle {
    align-self: flex-start;
  }
  .error {
    margin: 0;
    color: var(--red);
  }
</style>
