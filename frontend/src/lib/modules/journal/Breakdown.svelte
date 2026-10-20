<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Performance breakdown for the selected category (or all), further filterable by
  // side, ticker, strategy, signal, and date range. Equity curve + stat grid.
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import EquityChart from './EquityChart.svelte';
  import FilterBar from './FilterBar.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  // categoryId: '' means aggregate across all categories.
  let {
    categoryId = '',
    category = null, // the current category object (null when scope is "all")
    strategies = [],
    tags = [],
    suggestions = { tickers: [], exchanges: [], signals: [] },
    displayCurrency = 'USD',
    oncategoryChanged = () => {}
  } = $props();

  // ── Category description (collapsible "Details" banner) ──
  // Persist the collapsed state per browser; default collapsed.
  const DETAILS_KEY = 'otw.journal.breakdown.detailsOpen.v1';
  let detailsOpen = $state(false);
  let descDraft = $state('');
  let descSaving = $state(false);
  let descDirty = $state(false);

  (function loadDetailsOpen() {
    try {
      detailsOpen = localStorage.getItem(DETAILS_KEY) === '1';
    } catch {
      /* ignore */
    }
  })();
  function toggleDetails() {
    detailsOpen = !detailsOpen;
    try {
      localStorage.setItem(DETAILS_KEY, detailsOpen ? '1' : '0');
    } catch {
      /* ignore */
    }
  }

  // Sync the editable draft when the selected category changes (not on each keystroke).
  $effect(() => {
    const _ = category?.id;
    descDraft = category?.description ?? '';
    descDirty = false;
  });

  async function saveDescription() {
    if (!category) return;
    descSaving = true;
    try {
      // Empty string clears it (backend COALESCE keeps '' as the new value).
      await journalApi.updateCategory(category.id, { description: descDraft });
      descDirty = false;
      oncategoryChanged();
    } finally {
      descSaving = false;
    }
  }

  // Money values are labelled in the journal's display currency. No FX conversion yet,
  // so amounts are summed as-entered; a daily FX feed (coming) will convert them.
  const money = (n) => fmtMoney(n, displayCurrency);
  // For figures that mean gain-or-loss. The +/− goes in the text so the green/red
  // tint is a second channel, not the only one.
  const signedMoney = (n) => fmtSignedMoney(n, displayCurrency);
  const signedPct = (n) => (n == null ? '—' : `${n > 0 ? '+' : n < 0 ? '−' : ''}${Math.abs(n).toFixed(2)}%`);

  // The filter bar owns its own state and hands back the payload the API expects.
  let filter = $state({});

  let bd = $state(null);
  let loading = $state(true);

  $effect(() => {
    // Re-fetch whenever the scope or any filter changes.
    const f = filter;
    loading = true;
    journalApi
      .breakdown(f)
      .then((d) => {
        bd = d;
      })
      .finally(() => {
        loading = false;
      });
  });

  const stats = $derived(
    bd
      ? [
          // signedMoney/signedPct for the figures that mean gain-or-loss; plain money()
          // for the neutral ones (capital, fees, equity — a "+" there says nothing).
          //
          // The order is the reading order, one row of four per question: what the
          // account is worth, how the edge behaves, what it cost to run, the extremes
          // of a single trade, then the one figure that sums it up.
          //
          // Row 1 — the account.
          { label: $t('journal.breakdown.stat.investedCapital'), value: money(bd.invested_capital) },
          { label: $t('journal.breakdown.stat.currentEquity'), value: money(bd.current_equity) },
          { label: $t('journal.breakdown.stat.realizedPnl'), value: signedMoney(bd.realized_pnl), tone: bd.realized_pnl },
          { label: $t('journal.breakdown.stat.return'), value: signedPct(bd.return_pct), tone: bd.return_pct },
          // Row 2 — the edge.
          {
            label: $t('journal.breakdown.stat.winRate'),
            value: bd.win_rate == null ? '—' : fmtPct(bd.win_rate * 100)
          },
          { label: $t('journal.breakdown.stat.profitFactor'), value: fmtNum(bd.profit_factor) },
          {
            label: $t('journal.breakdown.stat.sharpe'),
            value: fmtNum(bd.sharpe),
            sub: $t('journal.breakdown.stat.sharpeSub', { sortino: fmtNum(bd.sortino) })
          },
          // Max drawdown is a magnitude, not a signed result — the label carries the sense.
          { label: $t('journal.breakdown.stat.maxDrawdown'), value: fmtPct(bd.max_drawdown), tone: -1 },
          // Row 3 — the cost of running it.
          { label: $t('journal.breakdown.stat.trades'), value: $t('journal.breakdown.stat.tradesValue', { closed: bd.closed_count, open: bd.open_count }) },
          { label: $t('journal.breakdown.stat.marginDeployed'), value: money(bd.peak_margin) },
          { label: $t('journal.breakdown.stat.returnOnMargin'), value: signedPct(bd.return_on_margin), tone: bd.return_on_margin },
          { label: $t('journal.breakdown.stat.totalFees'), value: money(bd.total_fees) },
          // Row 4 — the extremes. Avg win / avg loss are signed by definition, so the
          // tone is fixed, not derived; each sits next to the trade that set the record.
          { label: $t('journal.breakdown.stat.avgWin'), value: signedMoney(bd.avg_win), tone: 1 },
          { label: $t('journal.breakdown.stat.bestTrade'), value: signedMoney(bd.best_trade), tone: 1 },
          { label: $t('journal.breakdown.stat.avgLoss'), value: signedMoney(bd.avg_loss), tone: -1 },
          { label: $t('journal.breakdown.stat.worstTrade'), value: signedMoney(bd.worst_trade), tone: -1 },
          // Row 5 — the summary figure, alone.
          { label: $t('journal.breakdown.stat.expectancy'), value: signedMoney(bd.expectancy), tone: bd.expectancy }
        ]
      : []
  );

  function toneClass(t) {
    if (t === undefined || t === null) return '';
    return t > 0 ? 'pos' : t < 0 ? 'neg' : '';
  }
  // Empty metric value "—" renders in --faint (spec: .is-empty), not full-strength text.
  const isEmptyVal = (v) => v === '—' || v == null || v === '';
</script>

<div class="breakdown">
  {#if category}
    <section class="details">
      <button class="details-bar" onclick={toggleDetails} aria-expanded={detailsOpen}>
        <span class="chev" class:open={detailsOpen}><Icon name="chevron-right" size={13} /></span>
        <span class="details-title">{$t('journal.breakdown.details.title')}</span>
        {#if !detailsOpen && (category.description ?? '').trim()}
          <span class="details-peek">{category.description}</span>
        {/if}
      </button>
      {#if detailsOpen}
        <div class="details-body">
          <textarea
            bind:value={descDraft}
            oninput={() => (descDirty = true)}
            placeholder={$t('journal.breakdown.details.placeholder')}
          ></textarea>
          <div class="details-actions">
            <Button variant="primary" size="sm" onclick={saveDescription} disabled={!descDirty} loading={descSaving}>
              {$t('common.save')}
            </Button>
          </div>
        </div>
      {/if}
    </section>
  {/if}

  <FilterBar
    {categoryId}
    {strategies}
    {tags}
    {suggestions}
    storageKey="otw.journal.breakdown.filters.v1"
    idPrefix="bd"
    bind:value={filter}
  />

  {#if loading && !bd}
    <!-- Hold the chart + stat-grid shape rather than collapsing to a line of text. -->
    <div class="card"><Skeleton height="260px" /></div>
    <section class="grid">
      {#each Array(8) as _, i (i)}
        <Skeleton height="76px" />
      {/each}
    </section>
  {:else if bd}
    {#if bd.unconverted_trades > 0}
      <div class="warn">
        <Icon name="alert-triangle" size={13} /> {@html $t('journal.breakdown.unconvertedWarning', { count: bd.unconverted_trades, plural: bd.unconverted_trades === 1 ? '' : 's', currency: displayCurrency })}
      </div>
    {/if}
    <section class="card">
      <h3>{$t('journal.breakdown.equityCurve.title')} <span class="cur">{$t('journal.breakdown.equityCurve.in', { currency: displayCurrency })}</span></h3>
      <EquityChart points={bd.equity_curve} currency={displayCurrency} />
    </section>

    <section class="grid">
      {#each stats as s (s.label)}
        <div class="stat">
          <span class="stat-label">{s.label}</span>
          <!-- .num: figures line up across the grid even at different widths. -->
          <span class="stat-value num {toneClass(s.tone)}" class:is-empty={isEmptyVal(s.value)}>{s.value}</span>
          {#if s.sub}<span class="stat-sub">{s.sub}</span>{/if}
        </div>
      {/each}
      <!-- Fill the last row with empty cells so the grid stays rectangular. -->
      {#each Array((4 - (stats.length % 4)) % 4) as _, i (i)}
        <div class="stat stat-empty"></div>
      {/each}
    </section>
  {/if}
</div>

<style>
  .breakdown {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  /* Collapsible category description ("Details" banner). */
  .details {
    border: 0.5px solid var(--border);
    border-radius: 0;
    background: var(--surface);
    overflow: hidden;
  }
  .details-bar {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    padding: var(--space-3) var(--space-4);
    text-align: left;
    font: inherit;
  }
  .details-bar:hover {
    background: var(--surface-2);
  }
  .chev {
    color: var(--muted);
    display: inline-flex;
    transition: transform var(--dur-fast) var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .details-title {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .details-peek {
    color: var(--muted);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .details-body {
    padding: 0 var(--space-4) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .details-body textarea {
    width: 100%;
  }
  .details-actions {
    display: flex;
    justify-content: flex-end;
  }
  /* The save button is Button.svelte now. */

  /* Warning banner: the icon and the text carry the meaning, amber only reinforces it. */
  .warn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    border: 0.5px solid color-mix(in srgb, var(--amber) 45%, transparent);
    color: var(--text);
    border-radius: 0;
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .cur {
    color: var(--dim);
    font-weight: var(--fw-normal);
    font-size: 11px;
  }
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-4);
  }
  .card h3 {
    font-size: 12.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.03em;
    margin-bottom: var(--space-4);
  }
  /* Continuous filet grid: cells on --bg, separated by 0.5px hairlines (gap on a
     --border backing). 4 columns; the last row is padded with empty cells. */
  .grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--bg);
    border-radius: 0;
    padding: var(--pad-metric);
  }
  .stat-empty {
    padding: 0;
  }
  .stat-label {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-normal);
    color: var(--dim);
  }
  .stat-sub {
    font-size: 10.5px;
    color: var(--faint);
    line-height: var(--lh-tight);
  }
  .stat-value {
    font-family: var(--mono);
    font-size: var(--fs-metric-value);
    font-weight: var(--fw-normal);
    color: var(--text);
    line-height: var(--lh-tight);
  }
  /* Color is the second channel; signedMoney/signedPct already put the +/− in the text. */
  .stat-value.pos {
    color: var(--green);
  }
  .stat-value.neg {
    color: var(--red);
  }
  /* "is-" prefix: the bare .empty class is the global dashed empty-state box. */
  .stat-value.is-empty {
    color: var(--faint);
  }
</style>
