<script>
  // Quick-backtest panel — the strip under the chart, the way market tools put their trade
  // list there: collapsed it is one line of session numbers, expanded it is a resizable
  // window with three tabs (trades, statistics, P&L curve). It owns nothing: fills live with
  // the page, the engine derives the rest.
  import Icon from '$lib/ui/Icon.svelte';
  import Table from '$lib/ui/Table.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import QuickStats from './QuickStats.svelte';
  import QuickCurve from './QuickCurve.svelte';
  import { t } from '$lib/i18n';
  import { stats, unrealized, SIZE_MODES } from './quicktest.js';
  import { makePriceFmt, priceDigitsOfValues } from './format.js';

  let {
    trades = [],
    open = null,
    marks = [],
    last = null,
    size = $bindable(1),
    sizeMode = $bindable('units'),
    contractSize = $bindable(1),
    expanded = $bindable(false),
    height = $bindable(220),
    tab = $bindable('trades'),
    onremove,
    onreset,
    onundo,
    onclose
  } = $props();

  const MIN_H = 140;
  const s = $derived(stats(trades));
  const live = $derived(open ? unrealized(open, last) : 0);
  const total = $derived(s.pnl + live);

  // Prices in the list are written with the instrument's own precision, like the chart's.
  const digits = $derived(
    priceDigitsOfValues([
      ...trades.flatMap((x) => [x.entryAvg, x.exitAvg]),
      ...(open ? [open.avg] : []),
      ...(last != null ? [last] : [])
    ])
  );
  const fmtPrice = $derived(makePriceFmt(digits));
  const fmtPnl = (v) => (v >= 0 ? '+' : '') + v.toLocaleString(undefined, { maximumFractionDigits: 2 });
  const fmtTime = (ms) => {
    const d = new Date(ms);
    return Number.isFinite(ms) ? d.toISOString().slice(0, 16).replace('T', ' ') : '';
  };
  const fmtQty = (v) => Number(v.toFixed(6)).toString();

  const unitOptions = $derived(SIZE_MODES.map((m) => ({ value: m, label: $t(`histviz.quick.unit.${m}`) })));

  const tabs = $derived([
    { id: 'trades', label: $t('histviz.quick.tabTrades') },
    { id: 'stats', label: $t('histviz.quick.tabStats') },
    { id: 'curve', label: $t('histviz.quick.tabPerf') }
  ]);

  const columns = $derived([
    { key: 'side', label: $t('histviz.quick.side') },
    { key: 'qty', label: $t('histviz.quick.qty'), numeric: true },
    { key: 'entryAvg', label: $t('histviz.quick.entry'), numeric: true },
    { key: 'exitAvg', label: $t('histviz.quick.exit'), numeric: true },
    { key: 'openTs', label: $t('histviz.quick.opened') },
    { key: 'closeTs', label: $t('histviz.quick.closed') },
    { key: 'pnl', label: $t('histviz.quick.pnl'), numeric: true },
    { key: 'r', label: 'R', numeric: true },
    { key: 'act', label: '', width: '32px' }
  ]);

  // Newest first — the last trade you placed is the one you are looking at.
  const rows = $derived([...trades].reverse());

  // ── Resize ──
  // The panel is a window: drag its top edge and the chart above gives up the pixels. The
  // height is the parent's state, so it survives a reload with the rest of the session.
  let grip = $state(null);
  let drag = null;

  const clampH = (h) => Math.max(MIN_H, Math.min(h, Math.round((globalThis.innerHeight ?? 900) * 0.75)));

  function onGripDown(e) {
    if (e.button !== 0) return;
    drag = { y0: e.clientY, h0: height };
    grip?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onGripMove(e) {
    if (!drag) return;
    height = clampH(drag.h0 - (e.clientY - drag.y0));
  }
  function onGripUp(e) {
    if (!drag) return;
    drag = null;
    grip?.releasePointerCapture?.(e.pointerId);
  }
  function onGripKey(e) {
    const step = e.shiftKey ? 60 : 20;
    if (e.key === 'ArrowUp') height = clampH(height + step);
    else if (e.key === 'ArrowDown') height = clampH(height - step);
    else return;
    e.preventDefault();
  }
</script>

<section class="qp" class:expanded>
  {#if expanded}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="grip"
      class:dragging={!!drag}
      bind:this={grip}
      role="separator"
      aria-orientation="horizontal"
      aria-label={$t('histviz.quick.resize')}
      title={$t('histviz.quick.resize')}
      tabindex="0"
      onpointerdown={onGripDown}
      onpointermove={onGripMove}
      onpointerup={onGripUp}
      onpointercancel={onGripUp}
      onkeydown={onGripKey}
    ></div>
  {/if}

  <header>
    <button
      class="toggle"
      aria-expanded={expanded}
      title={expanded ? $t('histviz.quick.collapse') : $t('histviz.quick.expand')}
      onclick={() => (expanded = !expanded)}
    >
      <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={13} />
      <span class="ttl">{$t('histviz.quick.title')}</span>
    </button>

    <div class="chips">
      <span class="chip" class:up={total > 0} class:down={total < 0}>
        {$t('histviz.quick.pnl')} <b>{fmtPnl(total)}</b>
      </span>
      <span class="chip">{$t('histviz.quick.trades')} <b>{s.trades}</b></span>
      <span class="chip">{$t('histviz.quick.winRate')} <b>{s.winRate.toFixed(0)}%</b></span>
      <span class="chip" title={$t('histviz.quick.rHint')}>
        {$t('histviz.quick.avgR')} <b>{s.avgR == null ? '—' : s.avgR.toFixed(2)}</b>
      </span>
      {#if open}
        <span class="chip pos" class:short={open.dir < 0}>
          <Icon name={open.dir > 0 ? 'arrow-up' : 'arrow-down'} size={11} />
          {fmtQty(open.qty)} @ {fmtPrice(open.avg)}
          <b class:up={live > 0} class:down={live < 0}>{fmtPnl(live)}</b>
        </span>
      {/if}
    </div>

    <!-- Size: a number and the unit it is counted in. Contracts carry a point value, so the
         multiplier box appears with them; notional is an amount of quote currency, converted
         at the price of the click. -->
    <div class="size">
      <label>
        {$t('histviz.quick.qty')}
        <input type="number" min="0" step="any" bind:value={size} />
      </label>
      <div class="unit">
        <Dropdown bind:value={sizeMode} options={unitOptions} ariaLabel={$t('histviz.quick.unitLabel')} />
      </div>
      {#if sizeMode === 'contracts'}
        <label title={$t('histviz.quick.contractSizeHint')}>
          ×
          <input class="mult" type="number" min="0" step="any" bind:value={contractSize} />
        </label>
      {/if}
    </div>

    <!-- Undo takes back the last click on the chart, one fill at a time. -->
    <button
      class="act"
      disabled={!marks.length}
      onclick={() => onundo?.()}
      title={$t('histviz.quick.undoHint')}
      aria-label={$t('histviz.quick.undo')}
    >
      <Icon name="undo" size={12} /> {$t('histviz.quick.undo')}
    </button>
    <button class="act" disabled={!marks.length} onclick={() => onreset?.()} title={$t('histviz.quick.resetHint')}>
      <Icon name="rotate-ccw" size={12} /> {$t('histviz.quick.reset')}
    </button>
    <button class="act" onclick={() => onclose?.()} title={$t('histviz.quick.close')} aria-label={$t('histviz.quick.close')}>
      <Icon name="x" size={13} />
    </button>
  </header>

  {#if expanded}
    <div class="body" style:height="{height}px">
      <Tabs {tabs} bind:value={tab} ariaLabel={$t('histviz.quick.title')} />
      <div class="pane" class:flush={tab === 'curve'} role="tabpanel" id="panel-{tab}" aria-labelledby="tab-{tab}">
        {#if tab === 'trades'}
          <Table {columns} {rows} rowKey={(r) => r.id}>
            {#snippet cell(row, col)}
              {#if col.key === 'side'}
                <span class="side" class:short={row.dir < 0}>
                  <Icon name={row.dir > 0 ? 'arrow-up' : 'arrow-down'} size={11} />
                  {row.dir > 0 ? $t('histviz.quick.long') : $t('histviz.quick.short')}
                </span>
              {:else if col.key === 'qty'}
                {fmtQty(row.qty)}
              {:else if col.key === 'entryAvg'}
                {fmtPrice(row.entryAvg)}
              {:else if col.key === 'exitAvg'}
                {fmtPrice(row.exitAvg)}
              {:else if col.key === 'openTs'}
                {fmtTime(row.openTs)}
              {:else if col.key === 'closeTs'}
                {fmtTime(row.closeTs)}
              {:else if col.key === 'pnl'}
                <span class:up={row.pnl > 0} class:down={row.pnl < 0}>{fmtPnl(row.pnl)}</span>
              {:else if col.key === 'r'}
                {row.r == null ? '—' : row.r.toFixed(2)}
              {:else if col.key === 'act'}
                <button
                  class="row-del"
                  title={$t('histviz.quick.removeTrade')}
                  aria-label={$t('histviz.quick.removeTrade')}
                  onclick={() => onremove?.(row)}
                >
                  <Icon name="trash" size={12} />
                </button>
              {/if}
            {/snippet}
            {#snippet empty()}
              <span class="hint">{$t('histviz.quick.empty')}</span>
            {/snippet}
          </Table>
        {:else if tab === 'stats'}
          <QuickStats {s} {trades} {open} {live} />
        {:else}
          <QuickCurve {trades} />
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .qp {
    position: relative;
    flex: none;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  /* The top edge is the resize handle — the same grab strip a docked window has. */
  .grip {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 6px;
    /* Over the header, which raises itself above the table's sticky row: the top edge of the
       panel belongs to the resize grip. */
    z-index: calc(var(--z-sticky) + 2);
    cursor: row-resize;
    touch-action: none;
  }
  .grip::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 50%;
    width: 44px;
    height: 2px;
    transform: translateX(-50%);
    border-radius: 2px;
    background: var(--border);
  }
  .grip:hover::after,
  .grip:focus-visible::after,
  .grip.dragging::after {
    background: var(--accent);
  }
  .grip:focus-visible {
    outline: none;
  }
  header {
    /* Above the body: the trade table's sticky header sits on --z-sticky, and the size
       dropdown opens straight over it. */
    position: relative;
    z-index: calc(var(--z-sticky) + 1);
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    flex-wrap: wrap;
  }
  .expanded header {
    padding-top: var(--space-2);
  }
  /* One height for everything on the header row — chips, boxes, inputs and the unit menu —
     so the strip reads as a single line instead of a stack of different-sized boxes. */
  header {
    --qp-h: 26px;
  }
  .toggle,
  .act,
  .chips > *,
  .size label,
  .size input,
  .unit,
  .unit :global(.trigger) {
    height: var(--qp-h);
    box-sizing: border-box;
  }
  /* The two wrapping groups follow their children rather than being clipped by them. */
  .chips,
  .size {
    min-height: var(--qp-h);
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .toggle:hover {
    color: var(--text);
  }
  .chips {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .chip b {
    font-family: var(--mono);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .chip.up b,
  .up {
    color: var(--green);
  }
  .chip.down b,
  .down {
    color: var(--red);
  }
  /* The open position reads in the marker colours: blue long, red short. */
  .chip.pos {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border: 1px solid currentColor;
    border-radius: var(--radius);
    color: #3b82f6;
    font-family: var(--mono);
  }
  .chip.pos.short {
    color: #ef4444;
  }
  .size {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .size label {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .size input {
    width: 74px;
    padding: 1px var(--space-1);
    font-size: var(--text-xs);
  }
  .size input.mult {
    width: 56px;
  }
  .unit {
    width: 128px;
  }
  /* Collapsed, the panel is a single strip at the bottom of the page: the unit menu has no
     room under its button and the page clips what overflows, so it opens upward instead. */
  .qp:not(.expanded) .unit :global(.pop) {
    top: auto;
    bottom: calc(100% + 2px);
  }
  .act {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .act:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-control);
  }
  .act:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .body {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid var(--border);
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
    padding: 0 var(--space-2);
    overflow: hidden;
  }
  .pane {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  /* The curve fills its pane instead of scrolling inside it. */
  .pane.flush {
    overflow: hidden;
    padding: var(--space-1) 0 var(--space-2);
  }
  .side {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: #3b82f6;
  }
  .side.short {
    color: #ef4444;
  }
  .row-del {
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .row-del:hover {
    color: var(--red);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
