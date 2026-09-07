<script>
  // Quick-backtest bar: the strip under the workspace, the way a market tool puts its
  // strategy tester there. It spans the whole page, not one pane, because a session is a
  // *portfolio* of clicks: every chart on screen posts its fills into the same session and
  // the numbers here are their sum.
  //
  // Collapsed it is one line of session KPIs; expanded it is a resizable window with three
  // tabs (trades, statistics, P&L curve). It owns nothing: the fills live with the pane that
  // was clicked, the engine derives the rest.
  //
  // The chart picker is the *target*: the chart whose sizing the box on the right edits, and
  // the chart a click lands on. It is the active pane, so picking here and clicking there are
  // the same act and there is no second notion of "current chart" to keep in step.
  import Icon from '$lib/ui/Icon.svelte';
  import Table from '$lib/ui/Table.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import TradeStats from '$lib/analysis/TradeStats.svelte';
  import TradeCurve from '$lib/analysis/TradeCurve.svelte';
  import { t } from '$lib/i18n';
  import { stats, unrealized, SIZE_MODES, formatSize } from './quicktest.js';
  import { makePriceFmt, priceDigitsOfValues } from './format.js';

  let {
    trades = [], // every chart's closed trades, newest last
    opens = [], // one entry per chart still holding a position
    charts = [], // [{ value, label }], the panes showing an instrument
    target = null, // the active pane's id
    size = 1,
    sizeMode = 'units',
    contractSize = 1,
    expanded = $bindable(false),
    height = $bindable(220),
    tab = $bindable('trades'),
    onpicktarget,
    onsizing,
    onremove,
    onreset,
    onundo,
    onclose
  } = $props();

  const MIN_H = 140;
  const s = $derived(stats(trades));
  const live = $derived(opens.reduce((a, o) => a + unrealized(o, o.last), 0));
  const total = $derived(s.pnl + live);
  const busy = $derived(trades.length > 0 || opens.length > 0);

  // Prices are written with each instrument's own precision: one digit count over a session
  // holding BTC and EURUSD would be wrong for both.
  const fmtBy = $derived.by(() => {
    const by = {};
    for (const x of trades) (by[x.symbol] ??= []).push(x.entryAvg, x.exitAvg);
    for (const o of opens) (by[o.symbol] ??= []).push(o.avg, o.last);
    const out = {};
    for (const k of Object.keys(by)) out[k] = makePriceFmt(priceDigitsOfValues(by[k]));
    return out;
  });
  const fallbackFmt = makePriceFmt(2);
  const fmtPrice = (symbol, v) => (fmtBy[symbol] ?? fallbackFmt)(v);

  const fmtPnl = (v) => (v >= 0 ? '+' : '') + v.toLocaleString(undefined, { maximumFractionDigits: 2 });
  const fmtTime = (ms) => {
    const d = new Date(ms);
    return Number.isFinite(ms) ? d.toISOString().slice(0, 16).replace('T', ' ') : '';
  };
  // Sizes read in the unit they were clicked in (`size`), never in the engine's units; a
  // session from before they were carried falls back to the quantity itself.
  const fmtQty = (row) => formatSize(row.size ?? row.qty);

  const unitOptions = $derived(SIZE_MODES.map((m) => ({ value: m, label: $t(`histviz.quick.unit.${m}`) })));
  const chartOptions = $derived(
    charts.length ? charts : [{ value: '', label: $t('histviz.quick.noChart') }]
  );

  /** The KPI strip. Same five numbers a strategy tester leads with, in the same order. */
  const kpis = $derived([
    { k: $t('histviz.quick.netPnl'), v: fmtPnl(total), tone: total > 0 ? 'up' : total < 0 ? 'down' : '' },
    { k: $t('analysis.trades'), v: String(s.trades) },
    { k: $t('analysis.winRate'), v: `${s.winRate.toFixed(0)}%` },
    { k: $t('analysis.avgR'), v: s.avgR == null ? '·' : s.avgR.toFixed(2) },
    {
      k: $t('analysis.maxDrawdown'),
      v: s.maxDrawdown ? fmtPnl(-Math.abs(s.maxDrawdown)) : '0',
      tone: s.maxDrawdown ? 'down' : ''
    }
  ]);

  // The size box edits the *target* chart, so it reloads when the target changes. A local
  // draft, committed on change rather than on every keystroke: a value read back through the
  // pane would swallow the "." of a decimal halfway through being typed.
  let qtyDraft = $state('');
  let multDraft = $state('');
  let sizeKey = null;
  $effect(() => {
    const key = `${target}|${size}|${contractSize}`;
    if (key === sizeKey) return;
    sizeKey = key;
    qtyDraft = String(size);
    multDraft = String(contractSize);
  });
  const commitQty = () => onsizing?.({ size: Number(qtyDraft) > 0 ? Number(qtyDraft) : 1 });
  const commitMult = () => onsizing?.({ contract: Number(multDraft) > 0 ? Number(multDraft) : 1 });

  const tabs = $derived([
    { id: 'trades', label: $t('histviz.quick.tabTrades') },
    { id: 'stats', label: $t('histviz.quick.tabStats') },
    { id: 'curve', label: $t('histviz.quick.tabPerf') }
  ]);

  const columns = $derived([
    { key: 'symbol', label: $t('histviz.quick.symbol') },
    { key: 'side', label: $t('histviz.quick.side') },
    { key: 'qty', label: $t('histviz.quick.qty'), numeric: true },
    { key: 'entryAvg', label: $t('histviz.quick.entry'), numeric: true },
    { key: 'exitAvg', label: $t('histviz.quick.exit'), numeric: true },
    { key: 'openTs', label: $t('histviz.quick.opened') },
    { key: 'closeTs', label: $t('histviz.quick.closed') },
    { key: 'pnl', label: $t('analysis.pnl'), numeric: true },
    { key: 'r', label: 'R', numeric: true },
    { key: 'act', label: '', width: '32px' }
  ]);

  // Newest first: the last trade you placed is the one you are looking at.
  const rows = $derived([...trades].reverse());

  // ── Resize ──
  // The panel is a window: drag its top edge and the workspace above gives up the pixels.
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
      <Icon name={expanded ? 'chevron-down' : 'chevron-up'} size={12} />
      <span class="ttl">{$t('histviz.quick.title')}</span>
    </button>

    <!-- Which chart a click lands on. It is the active pane, so this both says where the
         next trade goes and moves the workspace's focus there. -->
    <div class="chart-pick">
      <Dropdown
        value={target ?? ''}
        options={chartOptions}
        disabled={charts.length < 2}
        ariaLabel={$t('histviz.quick.targetChart')}
        title={$t('histviz.quick.targetChartHint')}
        float
        onpick={(id) => onpicktarget?.(id)}
      />
    </div>

    <span class="rule" aria-hidden="true"></span>

    <div class="kpis">
      {#each kpis as m (m.k)}
        <div class="kpi">
          <span class="k">{m.k}</span>
          <span class="v {m.tone ?? ''}">{m.v}</span>
        </div>
      {/each}
    </div>

    <!-- Open positions, one chip per chart still holding one, in the marker colours. -->
    <div class="opens">
      {#each opens as o (o.paneId)}
        <span class="pos" class:short={o.dir < 0} title={o.label}>
          <Icon name={o.dir > 0 ? 'arrow-up' : 'arrow-down'} size={10} />
          <span class="psym">{o.symbol}</span>
          {fmtQty(o)} @ {fmtPrice(o.symbol, o.avg)}
          <b class:up={unrealized(o, o.last) > 0} class:down={unrealized(o, o.last) < 0}
            >{fmtPnl(unrealized(o, o.last))}</b
          >
        </span>
      {/each}
    </div>

    <!-- Size: a number and the unit it is counted in, for the target chart. Contracts carry
         a point value, so the multiplier box appears with them; notional is an amount of
         quote currency, converted at the price of the click. -->
    <div class="size">
      <label>
        <span class="slbl">{$t('histviz.quick.qty')}</span>
        <input
          type="number"
          min="0"
          step="any"
          disabled={!target}
          bind:value={qtyDraft}
          onchange={commitQty}
        />
      </label>
      <div class="unit">
        <Dropdown
          value={sizeMode}
          options={unitOptions}
          disabled={!target}
          ariaLabel={$t('histviz.quick.unitLabel')}
          float
          onpick={(mode) => onsizing?.({ mode })}
        />
      </div>
      <label class="mult-slot" class:off={sizeMode !== 'contracts'} title={$t('histviz.quick.contractSizeHint')}>
        <span class="slbl">×</span>
        <input
          class="mult"
          type="number"
          min="0"
          step="any"
          disabled={sizeMode !== 'contracts' || !target}
          bind:value={multDraft}
          onchange={commitMult}
        />
      </label>
    </div>

    <span class="rule" aria-hidden="true"></span>

    <!-- Undo takes back the last click on the target chart, one fill at a time. -->
    <button
      class="act"
      disabled={!target}
      onclick={() => onundo?.()}
      title={$t('histviz.quick.undoHint')}
      aria-label={$t('histviz.quick.undo')}
    >
      <Icon name="undo" size={12} />
    </button>
    <button
      class="act"
      disabled={!busy}
      onclick={() => onreset?.()}
      title={$t('histviz.quick.resetHint')}
      aria-label={$t('histviz.quick.reset')}
    >
      <Icon name="rotate-ccw" size={12} />
    </button>
    <button
      class="act"
      onclick={() => onclose?.()}
      title={$t('histviz.quick.close')}
      aria-label={$t('histviz.quick.close')}
    >
      <Icon name="x" size={13} />
    </button>
  </header>

  {#if expanded}
    <div class="body" style:height="{height}px">
      <Tabs {tabs} bind:value={tab} ariaLabel={$t('histviz.quick.title')} />
      <div class="pane" class:flush={tab === 'curve'} role="tabpanel" id="panel-{tab}" aria-labelledby="tab-{tab}">
        {#if tab === 'trades'}
          <Table {columns} {rows} rowKey={(r) => r.key}>
            {#snippet cell(row, col)}
              {#if col.key === 'symbol'}
                <span class="rsym">{row.symbol}</span>
                <span class="rtf">{row.timeframe}</span>
              {:else if col.key === 'side'}
                <span class="side" class:short={row.dir < 0}>
                  <Icon name={row.dir > 0 ? 'arrow-up' : 'arrow-down'} size={11} />
                  {row.dir > 0 ? $t('analysis.long') : $t('analysis.short')}
                </span>
              {:else if col.key === 'qty'}
                {fmtQty(row)}
              {:else if col.key === 'entryAvg'}
                {fmtPrice(row.symbol, row.entryAvg)}
              {:else if col.key === 'exitAvg'}
                {fmtPrice(row.symbol, row.exitAvg)}
              {:else if col.key === 'openTs'}
                {fmtTime(row.openTs)}
              {:else if col.key === 'closeTs'}
                {fmtTime(row.closeTs)}
              {:else if col.key === 'pnl'}
                <span class:up={row.pnl > 0} class:down={row.pnl < 0}>{fmtPnl(row.pnl)}</span>
              {:else if col.key === 'r'}
                {row.r == null ? '·' : row.r.toFixed(2)}
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
          <TradeStats {s} {trades} open={opens.length === 1 ? opens[0] : null} {live} />
        {:else}
          <TradeCurve {trades} />
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
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  /* The top edge is the resize handle: the same grab strip a docked window has. */
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
    background: var(--accent);
  }
  .grip:focus-visible {
    outline: none;
  }

  /* ── Header ──
     One fixed-height line that never wraps: it is the page's status bar, and a bar that
     grows a second row steals it from the charts above. */
  header {
    --qp-h: 24px;
    position: relative;
    z-index: calc(var(--z-sticky) + 1);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 38px;
    padding: 0 var(--space-2);
    flex-wrap: nowrap;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  header::-webkit-scrollbar {
    display: none;
  }
  .toggle,
  .act,
  .size label,
  .size input,
  .unit,
  .chart-pick {
    height: var(--qp-h);
    box-sizing: border-box;
    flex: none;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    white-space: nowrap;
  }
  .toggle:hover {
    color: var(--text);
  }
  .chart-pick {
    --control-h: 24px;
    width: 168px;
  }
  .rule {
    flex: none;
    width: var(--hairline);
    height: 20px;
    background: var(--border);
  }

  /* ── KPIs ──
     Label over value, the way a tester writes its summary: the eye reads the row of values
     without having to parse a sentence per number. */
  .kpis {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex: none;
  }
  .kpi {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    min-width: 58px;
  }
  .kpi .k {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--faint);
    white-space: nowrap;
  }
  .kpi .v {
    font-family: var(--mono);
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    color: var(--text);
    white-space: nowrap;
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }

  .opens {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  /* The open positions read in the marker colours: blue long, red short. */
  .pos {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
    padding: 1px 6px;
    border: var(--hairline) solid currentColor;
    border-radius: var(--radius);
    color: #3b82f6;
    font-family: var(--mono);
    font-size: var(--fs-desc);
    white-space: nowrap;
  }
  .pos.short {
    color: #ef4444;
  }
  .psym {
    font-weight: var(--fw-medium);
  }

  .size {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: none;
  }
  .size label {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
  }
  .size input {
    width: 72px;
    padding: 0 var(--space-1);
    font-family: var(--mono);
    font-size: var(--fs-desc);
  }
  .size input.mult {
    width: 52px;
  }
  /* The multiplier keeps its slot in every mode: a box that appears with "contracts" would
     shove the buttons beside it sideways the moment the unit changes. */
  .mult-slot.off {
    visibility: hidden;
  }
  .unit {
    --control-h: 24px;
    width: 116px;
  }

  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .act:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .act:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .body {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: var(--hairline) solid var(--border);
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
  .rsym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .rtf {
    margin-left: var(--space-1);
    font-size: var(--fs-desc);
    color: var(--faint);
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
    font-size: var(--fs-desc);
  }
</style>
