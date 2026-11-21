<script>
  // History modal body: every run, newest first, grouped by day. A run identical to the one
  // just before it (same instruments, settings, window and result) folds into it as "×N", so
  // re-running the same thing does not bury the list. Hovering a row swaps its numbers for its
  // actions. The star pins a run, which the 300-run cap never removes.
  //
  // Only the list scrolls: the filters, the column heads and the foot stay put, and the modal
  // body itself never overflows, so a wheel gesture always lands on one single scroller.
  // Truncated text clips with `overflow: clip`, never `hidden`: a hidden box is a scroll
  // container, and with the modal's `overscroll-behavior: none` on every descendant a wheel
  // gesture starting over it is swallowed instead of reaching the list.
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtNum, fmtSignedPct, fmtTime, fmtDay, dateKey } from '$lib/format.js';
  import { KIND_LABEL_KEYS, settingsKind } from './api.js';
  import { t } from '$lib/i18n';

  let { runs = [], activeId = null, onload, onreport, onremove, onstar, onclear } = $props();

  let query = $state('');
  let filter = $state('all'); // 'all' | a kind | 'starred'

  const MONTHS = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];

  /** "MNQ.20261218@CME" -> root "MNQ", contract "DEC 26". Anything else is its own root. */
  function splitTicker(tk = '') {
    const m = /^([^.@\s]+)\.(\d{4})(\d{2})\d{2}(?:@.*)?$/.exec(tk);
    if (!m) return { root: tk, contract: '' };
    return { root: m[1], contract: `${MONTHS[Number(m[3]) - 1] ?? m[3]} ${m[2].slice(2)}` };
  }

  /** What makes two runs the same run: same inputs, same output. */
  const sameRun = (a, b) =>
    !a.pinned &&
    !b.pinned &&
    a.ticker === b.ticker &&
    a.timeframe === b.timeframe &&
    (a.bars_from ?? '') === (b.bars_from ?? '') &&
    (a.bars_to ?? '') === (b.bars_to ?? '') &&
    JSON.stringify(a.dataset_ids ?? [a.dataset_id]) === JSON.stringify(b.dataset_ids ?? [b.dataset_id]) &&
    a.stats?.return_pct === b.stats?.return_pct &&
    a.stats?.trades === b.stats?.trades &&
    JSON.stringify(a.settings) === JSON.stringify(b.settings);

  const kinds = $derived(Object.keys(KIND_LABEL_KEYS).filter((k) => runs.some((r) => settingsKind(r.settings) === k)));

  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return runs.filter((r) => {
      if (filter === 'starred' && !r.pinned) return false;
      if (filter !== 'all' && filter !== 'starred' && settingsKind(r.settings) !== filter) return false;
      return !q || r.ticker.toLowerCase().includes(q) || r.name.toLowerCase().includes(q);
    });
  });

  /** Day groups of folded rows: [{ day, label, rows: [{ run, ids }] }]. */
  const groups = $derived.by(() => {
    const out = [];
    for (const r of visible) {
      const day = dateKey(r.created_at);
      let g = out.at(-1);
      if (!g || g.day !== day) out.push((g = { day, label: fmtDay(r.created_at), rows: [] }));
      const last = g.rows.at(-1);
      if (last && sameRun(last.run, r)) last.ids.push(r.id);
      else g.rows.push({ run: r, ids: [r.id] });
    }
    return out;
  });
</script>

<div class="hist">
  <div class="filters">
    <label class="search">
      <Icon name="search" size={13} />
      <input bind:value={query} placeholder={$t('backtest.history.search')} />
    </label>
    <div class="seg" role="tablist">
      <button role="tab" aria-selected={filter === 'all'} onclick={() => (filter = 'all')}>{$t('backtest.history.all')}</button>
      {#each kinds as k (k)}
        <button role="tab" aria-selected={filter === k} onclick={() => (filter = k)}>{$t(KIND_LABEL_KEYS[k])}</button>
      {/each}
      <button role="tab" aria-selected={filter === 'starred'} onclick={() => (filter = 'starred')}>
        <Icon name="star" size={12} /> {$t('backtest.history.starred')}
      </button>
    </div>
  </div>

  <div class="cols row-grid">
    <span>{$t('backtest.history.colRun')}</span>
    <span>{$t('backtest.history.colMode')}</span>
    <span class="num">{$t('backtest.history.colReturn')}</span>
    <span class="num">{$t('backtest.history.colTrades')}</span>
    <span class="num">{$t('backtest.history.colTime')}</span>
  </div>

  <div class="list">
    {#if !runs.length}
      <p class="empty">{$t('backtest.page.noSavedRuns')}</p>
    {:else if !groups.length}
      <p class="empty">{$t('backtest.history.noMatch')}</p>
    {:else}
      {#each groups as g (g.day)}
        <div class="day">{g.label}</div>
        {#each g.rows as { run: r, ids } (r.id)}
          {@const tk = splitTicker(r.ticker)}
          {@const ret = r.stats?.return_pct}
          <div class="row row-grid" class:on={ids.includes(activeId)}>
            <button class="run" onclick={() => onload?.(r)} title={$t('backtest.dock.loadRun')}>
              {#if r.pinned}<span class="pin" title={$t('backtest.history.starredOn')}><Icon name="star" size={12} /></span>{/if}
              <span class="tk">{tk.root}</span>
              {#if tk.contract}<span class="ct">{tk.contract}</span>{/if}
              <span class="tf">{r.timeframe}</span>
              {#if ids.length > 1}<span class="dup">×{ids.length}</span>{/if}
              {#if r.pinned}<span class="nm">{r.name}</span>{/if}
            </button>
            <span class="mode">{$t(KIND_LABEL_KEYS[settingsKind(r.settings)])}</span>
            <span class="num ret" class:pos={ret > 0} class:neg={ret < 0}>{fmtSignedPct(ret)}</span>
            <span class="num trades">{fmtNum(r.stats?.trades, 0)}</span>
            <span class="num time">{fmtTime(r.created_at)}</span>
            <div class="acts">
              <button class="load" onclick={() => onload?.(r)}><Icon name="rotate-ccw" size={13} /> {$t('backtest.history.load')}</button>
              <button onclick={() => onreport?.(r.id)}><Icon name="file-text" size={13} /> {$t('backtest.history.report')}</button>
              {#if !r.pinned}
                <button class="ico" title={$t('backtest.history.star')} aria-label={$t('backtest.history.star')} onclick={() => onstar?.(r)}><Icon name="star" size={13} /></button>
              {/if}
              <button class="ico danger" title={$t('backtest.history.remove')} aria-label={$t('backtest.history.remove')} onclick={() => onremove?.(ids)}><Icon name="trash" size={13} /></button>
            </div>
          </div>
        {/each}
      {/each}
    {/if}
  </div>

  <div class="foot">
    <span>{$t('backtest.history.foot')}</span>
    {#if runs.length}
      <button class="clear" onclick={() => onclear?.()}>{$t('backtest.page.clearHistory')}</button>
    {/if}
  </div>
</div>

<style>
  /* Fills the modal body edge to edge (rows hover full width), and exactly: the body never
     overflows, only `.list` scrolls. */
  .hist {
    display: flex;
    flex-direction: column;
    height: calc(100% + 2 * var(--space-4));
    margin: calc(-1 * var(--space-4));
    min-height: 0;
  }
  .filters {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-3) var(--space-6) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 240px;
    max-width: 100%;
    color: var(--muted);
    border-bottom: var(--hairline) solid var(--border-control);
    margin-bottom: var(--space-2);
  }
  .search:focus-within {
    border-bottom-color: var(--accent);
  }
  /* The underline is the frame: the field inside has none, and no focus halo. */
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    outline: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-sm);
  }
  .seg {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2) 0 calc(var(--space-2) + 1px);
    cursor: pointer;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button[aria-selected='true'] {
    color: var(--text);
    box-shadow: inset 0 -1px 0 var(--text);
  }

  .row-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 110px 120px 90px 80px;
    align-items: center;
    gap: var(--space-4);
    padding: 0 var(--space-6);
  }
  .cols {
    flex: none;
    height: 34px;
    font-size: var(--text-xs);
    color: var(--muted);
    border-bottom: var(--hairline) solid var(--border);
  }
  .num {
    text-align: right;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-4) var(--space-6);
  }
  .day {
    font-size: var(--text-xs);
    color: var(--muted);
    padding: var(--space-4) var(--space-6) var(--space-2);
  }
  .row {
    position: relative;
    height: 40px;
    border-bottom: var(--hairline) solid color-mix(in srgb, var(--border) 50%, transparent);
  }
  .row:hover,
  .row:focus-within {
    background: var(--surface-2);
  }
  .row.on {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .run {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    height: 100%;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    overflow: clip;
  }
  .tk,
  .ct,
  .tf,
  .dup,
  .ret,
  .trades,
  .time {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .tk {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .ct {
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .tf,
  .dup {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .pin {
    display: inline-flex;
    color: var(--amber);
  }
  .nm {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: clip;
    text-overflow: ellipsis;
  }
  .mode {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .ret {
    font-size: var(--text-sm);
  }
  .ret.pos {
    color: var(--green);
  }
  .ret.neg {
    color: var(--red);
  }
  .trades {
    font-size: var(--text-sm);
  }
  .time {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  /* Hover (or keyboard focus) trades the numbers for the actions, in the same columns. */
  .acts {
    display: none;
    grid-column: 3 / -1;
    grid-row: 1;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-3);
  }
  .row:hover .acts,
  .row:focus-within .acts {
    display: flex;
  }
  .row:hover :is(.ret, .trades, .time),
  .row:focus-within :is(.ret, .trades, .time) {
    visibility: hidden;
  }
  .acts button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
  }
  .acts button:hover {
    color: var(--accent);
  }
  .acts .load {
    color: var(--green);
  }
  .acts .ico {
    color: var(--muted);
  }
  .acts .ico:hover {
    color: var(--amber);
  }
  .acts .danger:hover {
    color: var(--red);
  }
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-6);
    border-top: var(--hairline) solid var(--border);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .foot .clear {
    background: transparent;
    border: none;
    padding: 0;
    color: var(--red);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .foot .clear:hover {
    text-decoration: underline;
  }
  @media (max-width: 767px) {
    .hist {
      height: auto;
    }
    .filters {
      flex-wrap: wrap;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-4) 0;
    }
    .row-grid {
      grid-template-columns: minmax(0, 1fr) 90px 60px;
      padding: 0 var(--space-4);
    }
    .cols span:nth-child(2),
    .mode,
    .trades,
    .cols span:nth-child(4) {
      display: none;
    }
    .acts {
      grid-column: 2 / -1;
    }
    .acts button:not(.ico) {
      font-size: var(--text-xs);
    }
  }
</style>
