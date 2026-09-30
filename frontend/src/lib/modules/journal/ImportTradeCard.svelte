<script>
  // One imported trade, drawn the way the journal will hold it — before anything is
  // written. This is the check the mapping step is built around: a mis-mapped column
  // shows up here as a date in the price row, a short that should be long, or a P&L
  // the wrong way round, without the reader having to audit a column table.
  //
  // Everything shown is server-computed (the same code path the journal uses on read),
  // so what this card says is what the import will produce.
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import { t, locale } from '$lib/i18n';
  import { fmtSignedMoney, fmtMoney, ASSET_CLASSES, UNIT_TYPES } from './api.js';

  let { item = null } = $props();

  const trade = $derived(item?.trade ?? null);
  const pnl = $derived(item?.computed ?? null);
  const isOpen = $derived(pnl?.net_pnl == null);
  const partial = $derived((pnl?.open_qty ?? 0) > 0 && pnl?.net_pnl != null);

  function fmtDate(iso) {
    if (!iso) return '—';
    return new Date(iso).toLocaleString($locale, {
      day: '2-digit',
      month: 'short',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    });
  }
  function fmtNum(v) {
    if (v == null) return '—';
    return Number(v.toFixed(8)).toLocaleString($locale, { maximumFractionDigits: 8 });
  }
  function assetLabel(id) {
    return ASSET_CLASSES.find((a) => a.id === id)?.label ?? id;
  }
  function unitLabel(id) {
    return UNIT_TYPES.find((u) => u.id === id)?.label ?? id;
  }

  // Trade-level fee plus every leg's own fee — advanced trades carry theirs on the legs.
  function sumFees(tr) {
    const legs = [...(tr.entries ?? []), ...(tr.exits ?? [])];
    return (tr.fees ?? 0) + legs.reduce((s, l) => s + (l.fees ?? 0), 0);
  }

  const customFields = $derived(Object.entries(trade?.fields ?? {}));
  const rowLabel = $derived(
    (item?.source_rows ?? []).length > 1
      ? `${item.source_rows[0]}–${item.source_rows[item.source_rows.length - 1]}`
      : String(item?.source_rows?.[0] ?? '')
  );
</script>

{#if trade}
  <article class="card" class:dupe={item.duplicate}>
    <header class="head">
      <div class="id">
        <span class="ticker">{trade.ticker || $t('journal.import.card.noTicker')}</span>
        <span class="side {trade.side}">{$t(`journal.side.${trade.side}`)}</span>
      </div>
      <div class="tags">
        <span class="tag">{assetLabel(trade.asset_class)}</span>
        {#if trade.exchange}<span class="tag">{trade.exchange}</span>{/if}
        <span class="tag mono">{$t('journal.import.card.line', { n: rowLabel })}</span>
      </div>
    </header>

    <div class="grid">
      {#if trade.advanced}
        <!-- Fills folded into a position: show the legs, they are the trade. -->
        <div class="legs">
          <span class="leg-title">{$t('journal.import.card.entries')}</span>
          {#each trade.entries as leg (leg.id)}
            <div class="leg">
              <span class="leg-date">{fmtDate(leg.at)}</span>
              <span class="leg-qty mono">{fmtNum(leg.qty)}</span>
              <span class="leg-at">@</span>
              <span class="leg-price mono">{fmtNum(leg.price)}</span>
            </div>
          {/each}
        </div>
        <div class="legs">
          <span class="leg-title">{$t('journal.import.card.exits')}</span>
          {#if trade.exits.length === 0}
            <div class="leg empty">{$t('journal.import.card.stillOpen')}</div>
          {/if}
          {#each trade.exits as leg (leg.id)}
            <div class="leg">
              <span class="leg-date">{fmtDate(leg.at)}</span>
              <span class="leg-qty mono">{fmtNum(leg.qty)}</span>
              <span class="leg-at">@</span>
              <span class="leg-price mono">{fmtNum(leg.price)}</span>
            </div>
          {/each}
        </div>
      {:else}
        <div class="line">
          <span class="lbl">{$t('journal.import.card.entry')}</span>
          <span class="date">{fmtDate(trade.entry_at)}</span>
          <span class="price mono">{fmtNum(trade.entry_price)}</span>
        </div>
        <div class="line">
          <span class="lbl">{$t('journal.import.card.exit')}</span>
          <span class="date">{isOpen ? '—' : fmtDate(trade.exit_at)}</span>
          <span class="price mono">{fmtNum(trade.exit_price)}</span>
        </div>
      {/if}
    </div>

    <div class="meta">
      <span
        >{$t('journal.import.card.qty')}
        <b class="mono">{fmtNum(pnl.entry_qty || trade.quantity)}</b>
        {unitLabel(trade.unit_type)}</span
      >
      <span>{$t('journal.import.card.fees')} <b class="mono">{fmtMoney(sumFees(trade), trade.currency)}</b></span>
      {#if trade.leverage !== 1}
        <span>{$t('journal.import.card.leverage')} <b class="mono">×{fmtNum(trade.leverage)}</b></span>
      {/if}
      {#if trade.multiplier !== 1}
        <span>{$t('journal.import.card.multiplier')} <b class="mono">×{fmtNum(trade.multiplier)}</b></span>
      {/if}
      <span class="ccy">{trade.currency}</span>
    </div>

    {#if customFields.length}
      <div class="customs">
        {#each customFields as [key, value]}
          <span class="chip"><b>{key}</b>{value}</span>
        {/each}
      </div>
    {/if}

    <footer class="pnl-row">
      <span class="pnl-lbl">{$t('journal.import.card.netPnl')}</span>
      {#if isOpen}
        <span class="pnl open">{$t('journal.trades.open')}</span>
      {:else}
        <!-- The sign is in the text; color is the second channel, never the only one. -->
        <span class="pnl {pnl.net_pnl >= 0 ? 'pos' : 'neg'}">
          {fmtSignedMoney(pnl.net_pnl, trade.currency)}
        </span>
      {/if}
    </footer>

    {#if item.duplicate || item.pnl_mismatch || partial || item.warnings.length}
      <div class="flags">
        {#if item.duplicate}
          <Badge tone="neutral" icon="check">{$t('journal.import.card.duplicate')}</Badge>
        {/if}
        {#if item.pnl_mismatch}
          <Badge tone="danger" icon="alert-triangle">
            {$t('journal.import.card.pnlMismatch', {
              file: fmtSignedMoney(item.pnl_source, trade.currency)
            })}
          </Badge>
        {/if}
        {#if partial}
          <Badge tone="warn">{$t('journal.import.card.partial', { qty: fmtNum(pnl.open_qty) })}</Badge>
        {/if}
        {#each item.warnings as w}
          <span class="warn"><Icon name="alert-triangle" size={12} />{w.message}</span>
        {/each}
      </div>
    {/if}
  </article>
{/if}

<style>
  .card {
    border: 0.5px solid var(--border);
    border-left: 2px solid var(--border);
    background: var(--surface);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .card.dupe {
    opacity: 0.62;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .id {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .ticker {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  /* Direction reads as a tinted word, like the trades table — no pill. */
  .side {
    text-transform: uppercase;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
  }
  .side.long {
    color: var(--green);
  }
  .side.short {
    color: var(--red);
  }
  .tags {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .tag {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-top: 0.5px solid var(--border);
    border-bottom: 0.5px solid var(--border);
  }
  .line {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .lbl,
  .leg-title {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--dim);
  }
  .date {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .price {
    font-size: var(--text-base);
    font-variant-numeric: tabular-nums;
  }
  .legs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .leg {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: var(--text-sm);
  }
  .leg-date {
    color: var(--muted);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .leg-at {
    color: var(--dim);
  }
  .leg-qty,
  .leg-price {
    font-variant-numeric: tabular-nums;
  }
  .leg.empty {
    color: var(--dim);
    font-style: italic;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .meta b {
    color: var(--text);
    font-weight: var(--fw-normal);
  }
  .ccy {
    margin-left: auto;
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .customs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    display: inline-flex;
    gap: 4px;
    border: 0.5px solid var(--border);
    padding: 1px 6px;
    font-size: var(--text-xs);
    color: var(--muted);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip b {
    color: var(--dim);
    font-weight: var(--fw-normal);
  }
  .pnl-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-2);
  }
  .pnl-lbl {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--dim);
  }
  .pnl {
    font-size: var(--text-lg);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-weight: var(--fw-medium);
  }
  .pnl.pos {
    color: var(--green);
  }
  .pnl.neg {
    color: var(--red);
  }
  .pnl.open {
    color: var(--muted);
    font-size: var(--text-sm);
    font-family: inherit;
    font-weight: var(--fw-normal);
  }
  .flags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .warn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--amber);
  }
</style>
