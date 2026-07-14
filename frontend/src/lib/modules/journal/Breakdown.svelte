<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Performance breakdown for the selected category (or all), further filterable by
  // side, ticker, strategy, signal, and date range. Equity curve + stat grid.
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum, ASSET_CLASSES } from './api.js';
  import EquityChart from './EquityChart.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  // categoryId: '' means aggregate across all categories.
  let {
    categoryId = '',
    category = null, // the current category object (null when scope is "all")
    strategies = [],
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

  // Trade-level filters layered on top of the category scope.
  let fSide = $state('');
  let fTicker = $state('');
  let fStrategy = $state('');
  let fSignal = $state('');
  let fAsset = $state('');
  let fSince = $state(''); // yyyy-mm-dd (date input)
  let fUntil = $state('');

  // Persist the filter bar across refreshes (per browser).
  const FILTERS_KEY = 'otw.journal.breakdown.filters.v1';
  let filtersLoaded = $state(false);
  (function loadFilters() {
    try {
      const p = JSON.parse(localStorage.getItem(FILTERS_KEY) || '{}');
      if (['', 'long', 'short'].includes(p.fSide)) fSide = p.fSide;
      if (typeof p.fTicker === 'string') fTicker = p.fTicker;
      if (typeof p.fStrategy === 'string') fStrategy = p.fStrategy;
      if (typeof p.fSignal === 'string') fSignal = p.fSignal;
      if (typeof p.fAsset === 'string') fAsset = p.fAsset;
      if (typeof p.fSince === 'string') fSince = p.fSince;
      if (typeof p.fUntil === 'string') fUntil = p.fUntil;
    } catch {
      /* corrupt — ignore */
    }
    filtersLoaded = true;
  })();

  $effect(() => {
    const snap = { fSide, fTicker, fStrategy, fSignal, fAsset, fSince, fUntil };
    if (!filtersLoaded) return;
    try {
      localStorage.setItem(FILTERS_KEY, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  let bd = $state(null);
  let loading = $state(true);

  // The active filter payload sent to the API; date inputs become RFC3339 day bounds.
  const filter = $derived({
    category_id: categoryId || undefined,
    side: fSide,
    ticker: fTicker,
    strategy_id: fStrategy,
    signal_name: fSignal,
    asset_class: fAsset,
    since: fSince ? new Date(fSince + 'T00:00:00').toISOString() : '',
    until: fUntil ? new Date(fUntil + 'T23:59:59').toISOString() : ''
  });

  const activeFilterCount = $derived(
    [fSide, fTicker, fStrategy, fSignal, fAsset, fSince, fUntil].filter(Boolean).length
  );

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

  function clearFilters() {
    fSide = fTicker = fStrategy = fSignal = fAsset = fSince = fUntil = '';
  }

  const stats = $derived(
    bd
      ? [
          // signedMoney/signedPct for the figures that mean gain-or-loss; plain money()
          // for the neutral ones (capital, fees, equity — a "+" there says nothing).
          { label: $t('journal.breakdown.stat.investedCapital'), value: money(bd.invested_capital) },
          { label: $t('journal.breakdown.stat.realizedPnl'), value: signedMoney(bd.realized_pnl), tone: bd.realized_pnl },
          { label: $t('journal.breakdown.stat.currentEquity'), value: money(bd.current_equity) },
          { label: $t('journal.breakdown.stat.return'), value: signedPct(bd.return_pct), tone: bd.return_pct },
          { label: $t('journal.breakdown.stat.marginDeployed'), value: money(bd.total_margin) },
          { label: $t('journal.breakdown.stat.returnOnMargin'), value: signedPct(bd.return_on_margin), tone: bd.return_on_margin },
          { label: $t('journal.breakdown.stat.totalFees'), value: money(bd.total_fees) },
          { label: $t('journal.breakdown.stat.trades'), value: $t('journal.breakdown.stat.tradesValue', { closed: bd.closed_count, open: bd.open_count }) },
          {
            label: $t('journal.breakdown.stat.winRate'),
            value: bd.win_rate == null ? '—' : fmtPct(bd.win_rate * 100)
          },
          { label: $t('journal.breakdown.stat.profitFactor'), value: fmtNum(bd.profit_factor) },
          { label: $t('journal.breakdown.stat.expectancy'), value: signedMoney(bd.expectancy), tone: bd.expectancy },
          // avg win / avg loss are signed by definition; the tone is fixed, not derived.
          { label: $t('journal.breakdown.stat.avgWin'), value: signedMoney(bd.avg_win), tone: 1 },
          { label: $t('journal.breakdown.stat.avgLoss'), value: signedMoney(bd.avg_loss), tone: -1 },
          { label: $t('journal.breakdown.stat.sharpe'), value: fmtNum(bd.sharpe) },
          // Max drawdown is a magnitude, not a signed result — the label carries the sense.
          { label: $t('journal.breakdown.stat.maxDrawdown'), value: fmtPct(bd.max_drawdown), tone: -1 },
          { label: $t('journal.breakdown.stat.bestTrade'), value: signedMoney(bd.best_trade), tone: 1 },
          { label: $t('journal.breakdown.stat.worstTrade'), value: signedMoney(bd.worst_trade), tone: -1 }
        ]
      : []
  );

  function toneClass(t) {
    if (t === undefined || t === null) return '';
    return t > 0 ? 'pos' : t < 0 ? 'neg' : '';
  }
</script>

<datalist id="bd-tickers">
  {#each suggestions.tickers as v}<option value={v}></option>{/each}
</datalist>
<datalist id="bd-signals">
  {#each suggestions.signals as v}<option value={v}></option>{/each}
</datalist>

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

  <section class="filter-bar">
    <select bind:value={fSide} title={$t('journal.breakdown.filter.side')}>
      <option value="">{$t('journal.breakdown.filter.anySide')}</option>
      <option value="long">{$t('journal.side.long')}</option>
      <option value="short">{$t('journal.side.short')}</option>
    </select>
    <input placeholder={$t('journal.breakdown.filter.ticker')} list="bd-tickers" autocomplete="off" bind:value={fTicker} />
    <select bind:value={fStrategy} title={$t('journal.breakdown.filter.strategy')}>
      <option value="">{$t('journal.breakdown.filter.anyStrategy')}</option>
      {#each strategies as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
    </select>
    <input placeholder={$t('journal.breakdown.filter.signal')} list="bd-signals" autocomplete="off" bind:value={fSignal} />
    <select bind:value={fAsset} title={$t('journal.breakdown.filter.assetClass')}>
      <option value="">{$t('journal.breakdown.filter.anyClass')}</option>
      {#each ASSET_CLASSES as a (a.id)}<option value={a.id}>{a.label}</option>{/each}
    </select>
    <label class="date">{$t('journal.breakdown.filter.from')} <input type="date" bind:value={fSince} /></label>
    <label class="date">{$t('journal.breakdown.filter.to')} <input type="date" bind:value={fUntil} /></label>
    {#if activeFilterCount > 0}
      <div class="filter-clear">
        <Button size="sm" icon="x" onclick={clearFilters}>
          {$t('journal.breakdown.filter.clear', { count: activeFilterCount })}
        </Button>
      </div>
    {/if}
  </section>

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
          <span class="stat-value num {toneClass(s.tone)}">{s.value}</span>
        </div>
      {/each}
    </section>
  {/if}
</div>

<style>
  /* .filter-bar / .filter-clear come from theme/components.css (shared with Trades). */
  .filter-bar input:not([type]) {
    width: 110px;
  }
  .breakdown {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  /* Collapsible category description ("Details" banner). */
  .details {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
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
    background: var(--surface-2, var(--surface));
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
    border: 1px solid color-mix(in srgb, var(--amber) 45%, transparent);
    color: var(--text);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .cur {
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-sm);
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .card h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-semibold);
    margin-bottom: var(--space-4);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-4);
  }
  .stat-label {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .stat-value {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
    line-height: var(--lh-tight);
  }
  /* Color is the second channel; signedMoney/signedPct already put the +/− in the text. */
  .stat-value.pos {
    color: var(--green);
  }
  .stat-value.neg {
    color: var(--red);
  }
</style>
