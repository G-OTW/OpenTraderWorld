<script>
  // `book` — what the portfolio is, right now. The one panel that works on day one: no
  // bars, no connector, no history.
  import { t } from '$lib/i18n';
  import { fmtMoney, fmtSignedMoney, fmtPct } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  // Deployed share, so the invested/cash split reads as one bar rather than two figures the
  // user has to divide in their head. A ledger with no deposit has no cash account, so
  // there is no split to draw: the bar would claim 100% deployed, which is a measurement
  // and not the absence of one.
  const tracked = $derived(d?.cash_tracked !== false);
  const investedPct = $derived(d && d.net_worth && tracked ? 100 - d.cash_pct : null);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="stats">
    <StatCard label={$t('portfolios.analytics.book.netWorth')} value={fmtMoney(d.net_worth, currency)} />
    <StatCard label={$t('portfolios.analytics.book.invested')} value={fmtMoney(d.invested, currency)} />
    {#if tracked}
      <StatCard
        label={$t('portfolios.analytics.book.cash')}
        value={fmtMoney(d.cash_total, currency)}
        hint={fmtPct(d.cash_pct)}
      />
    {/if}
    <StatCard label={$t('portfolios.analytics.book.unrealized')} value={fmtSignedMoney(d.unrealized, currency)} />
    <StatCard label={$t('portfolios.analytics.book.realized')} value={fmtSignedMoney(d.realized, currency)} />
    <StatCard label={$t('portfolios.analytics.book.income')} value={fmtMoney(d.income.total, currency)} />
  </div>

  {#if investedPct != null}
    <div class="split">
      <div class="bar" role="img" aria-label={$t('portfolios.analytics.book.splitAria')}>
        <div class="seg invested" style="width:{Math.max(0, Math.min(100, investedPct))}%"></div>
        <div class="seg cash" style="width:{Math.max(0, Math.min(100, d.cash_pct))}%"></div>
      </div>
      <div class="legend">
        <span><i class="dot invested"></i>{$t('portfolios.analytics.book.invested')} {fmtPct(investedPct)}</span>
        <span><i class="dot cash"></i>{$t('portfolios.analytics.book.cash')} {fmtPct(d.cash_pct)}</span>
      </div>
    </div>
  {/if}

  {#if !tracked}
    <!-- Not a warning about a wrong number, a note about a number that does not exist yet:
         book a deposit and the cash line starts saying something. -->
    <p class="hint">{$t('portfolios.analytics.book.noCash')}</p>
  {/if}

  {#if tracked && d.cash?.length > 1}
    <!-- Per currency, in that currency: converting them into one figure hides that half the
         cash is sitting in a currency the user did not mean to hold. -->
    <div class="ccys">
      {#each d.cash as c (c.currency)}
        <span class="chip" class:neg={c.amount < 0}>{c.currency} {fmtMoney(c.amount, c.currency)}</span>
      {/each}
    </div>
  {/if}

  {#if d.unpriced > 0}
    <p class="warn">{$t('portfolios.analytics.book.unpriced', { n: d.unpriced })}</p>
  {/if}

  {#if d.allocation?.length}
    <table class="tbl alloc">
      <thead>
        <tr>
          <th>{$t('portfolios.analytics.book.bucket')}</th>
          <th class="r">{$t('portfolios.analytics.book.value')}</th>
          <th class="r">{$t('portfolios.analytics.book.share')}</th>
          <th class="w"></th>
        </tr>
      </thead>
      <tbody>
        {#each d.allocation as a (a.bucket)}
          <tr>
            <td>{$t(`portfolios.class.${a.bucket}`)}</td>
            <td class="r num">{fmtMoney(a.value, currency)}</td>
            <td class="r num">{fmtPct(a.pct)}</td>
            <td class="w">
              <div class="mini"><div style="width:{Math.max(0, Math.min(100, a.pct))}%"></div></div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  <Sample sample={block.sample} />
{/if}

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: var(--space-3);
  }
  .split {
    margin-top: var(--space-4);
  }
  .bar {
    display: flex;
    height: 10px;
    border-radius: 999px;
    overflow: hidden;
    background: var(--surface-2);
  }
  .seg.invested {
    background: var(--accent);
  }
  .seg.cash {
    background: var(--muted);
    opacity: 0.5;
  }
  .legend {
    display: flex;
    gap: var(--space-4);
    margin-top: var(--space-2);
    font-size: 12px;
    color: var(--muted);
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    margin-right: var(--space-1);
  }
  .dot.invested {
    background: var(--accent);
  }
  .dot.cash {
    background: var(--muted);
  }
  .ccys {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  .chip {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 12px;
  }
  .chip.neg {
    color: var(--red);
    border-color: var(--red);
  }
  .warn {
    margin-top: var(--space-3);
    color: var(--amber);
    font-size: 12px;
  }
  .hint {
    margin-top: var(--space-3);
    color: var(--muted);
    font-size: 12px;
  }
  .alloc {
    margin-top: var(--space-4);
  }
  .r {
    text-align: right;
  }
  .w {
    width: 30%;
  }
  .mini {
    height: 6px;
    background: var(--surface-2);
    border-radius: 999px;
    overflow: hidden;
  }
  .mini > div {
    height: 100%;
    background: var(--accent);
  }
</style>
