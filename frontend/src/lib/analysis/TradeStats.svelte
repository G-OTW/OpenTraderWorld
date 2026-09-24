<script>
  // Statistics tab of the quick backtest, laid out the way a strategy tester reads: the
  // headline numbers, the two gross sides split by direction, the streaks, then the two
  // distributions. Everything is derived from the closed trades — nothing is stored.
  import { t } from '$lib/i18n';
  import TradeDist from './TradeDist.svelte';
  import { sideStats, streaks } from './trades.js';

  let { s = {}, trades = [], open = null, live = 0 } = $props();

  const by = $derived(sideStats(trades));
  const runs = $derived(streaks(trades));

  const num = (v, d = 2) =>
    v == null || !Number.isFinite(v) ? '—' : v.toLocaleString(undefined, { maximumFractionDigits: d });
  const signed = (v, d = 2) => (v == null || !Number.isFinite(v) ? '—' : (v >= 0 ? '+' : '') + num(v, d));
  const pct = (v) => (v == null || !Number.isFinite(v) ? '—' : `${num(v, 2)}%`);
  // Ratios keep their decimals: a profit factor of 0.0004 must not print as a bare 0.
  const fixed = (v, d = 2) =>
    v == null || !Number.isFinite(v)
      ? '—'
      : v.toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d });

  /** A hold time reads in the unit that fits it — minutes for a scalp, days for a swing. */
  function duration(ms) {
    if (ms == null || !Number.isFinite(ms) || ms <= 0) return '—';
    const m = ms / 60000;
    if (m < 60) return `${num(m, 0)} min`;
    const h = m / 60;
    if (h < 48) return `${num(h, 1)} h`;
    return `${num(h / 24, 1)} d`;
  }

  // ── Headline ──
  const heads = $derived([
    { label: $t('analysis.netPnl'), value: signed(s.pnl), tone: s.pnl },
    { label: $t('analysis.maxDrawdown'), value: num(s.maxDrawdown), tone: s.maxDrawdown ? -1 : 0 },
    {
      label: $t('analysis.profitableTrades'),
      value: pct(s.winRate),
      sub: `${s.wins ?? 0}/${s.trades ?? 0}`
    },
    { label: $t('analysis.profitFactor'), value: fixed(s.profitFactor), tone: (s.profitFactor ?? 1) - 1 }
  ]);

  // ── Performance analysis ──
  const perf = $derived([
    { label: $t('analysis.grossProfit'), value: num(s.grossWin), tone: 1 },
    { label: $t('analysis.grossLoss'), value: num(s.grossLoss), tone: -1 },
    { label: $t('analysis.expectancy'), value: signed(s.expectancy), tone: s.expectancy },
    { label: $t('analysis.avgR'), value: fixed(s.avgR), tone: s.avgR, title: $t('analysis.rHint') },
    { label: $t('analysis.best'), value: signed(s.best), tone: s.best },
    { label: $t('analysis.worst'), value: signed(s.worst), tone: s.worst },
    { label: $t('analysis.avgWin'), value: signed(s.avgWin), tone: 1 },
    { label: $t('analysis.avgLoss'), value: signed(s.avgLoss), tone: -1 },
    { label: $t('analysis.avgRunup'), value: signed(s.avgRunup), tone: 1 },
    { label: $t('analysis.avgMae'), value: signed(s.avgMae), tone: -1 },
    { label: $t('analysis.avgHold'), value: duration(s.avgHoldMs) },
    { label: $t('analysis.avgPnlPct'), value: s.avgPnlPct == null ? '—' : `${signed(s.avgPnlPct)}%`, tone: s.avgPnlPct }
  ]);

  // ── Profits and losses, by side ──
  // One bar per side: the losing gross on the left of the zero mark, the winning gross on its
  // right, each scaled against the biggest gross of the three so the rows compare.
  const scale = $derived(
    Math.max(by.all.grossWin, by.all.grossLoss, by.long.grossWin, by.long.grossLoss, by.short.grossWin, by.short.grossLoss, 1e-9)
  );
  const bars = $derived(
    [
      { key: 'all', label: $t('analysis.allSides'), d: by.all },
      { key: 'long', label: $t('analysis.long'), d: by.long },
      { key: 'short', label: $t('analysis.short'), d: by.short }
    ].filter((r) => r.key === 'all' || r.d.trades > 0)
  );

  // Hovering a bar tells what it is made of — the two grosses that build it and the net they
  // come to. The card follows the pointer inside the list, which is the positioned parent.
  let tip = $state(null); // { x, y, row }
  function onRowMove(e, row) {
    const box = e.currentTarget.parentElement.getBoundingClientRect();
    tip = { x: e.clientX - box.left, y: e.clientY - box.top, row };
  }
</script>

<div class="wrap">
  <div class="heads">
    {#if open}
      <!-- The live position sits with the numbers it will change once it is closed. -->
      <div class="head live">
        <span class="lbl">{$t('analysis.openPosition')}</span>
        <b class="val" class:up={live > 0} class:down={live < 0}>{signed(live)}</b>
        <span class="sub">{open.dir > 0 ? $t('analysis.long') : $t('analysis.short')} {num(open.qty, 6)}</span>
      </div>
    {/if}
    {#each heads as x (x.label)}
      <div class="head">
        <span class="lbl">{x.label}</span>
        <b class="val" class:up={x.tone > 0} class:down={x.tone < 0}>{x.value}</b>
        {#if x.sub}<span class="sub">{x.sub}</span>{/if}
      </div>
    {/each}
  </div>

  <section>
    <h4>{$t('analysis.performanceAnalysis')}</h4>
    <div class="cells">
      {#each perf as x (x.label)}
        <div class="cell" title={x.title ?? ''}>
          <span class="lbl">{x.label}</span>
          <b class="val" class:up={x.tone > 0} class:down={x.tone < 0}>{x.value}</b>
        </div>
      {/each}
    </div>
  </section>

  <section>
    <h4>{$t('analysis.profitsLosses')}</h4>
    <ul class="pl">
      {#each bars as r (r.key)}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <li
          onpointermove={(e) => onRowMove(e, r)}
          onpointerleave={() => (tip = null)}
          title="{r.label} · {signed(r.d.pnl)}"
        >
          <span class="side-lbl">{r.label}</span>
          <div class="track">
            <div class="half loss">
              <span class="fill" style:width="{(r.d.grossLoss / scale) * 100}%"></span>
            </div>
            <div class="half win">
              <span class="fill" style:width="{(r.d.grossWin / scale) * 100}%"></span>
            </div>
          </div>
          <b class="net" class:up={r.d.pnl > 0} class:down={r.d.pnl < 0}>{signed(r.d.pnl)}</b>
        </li>
      {/each}

      {#if tip}
        <div class="tip" style:left="{tip.x}px" style:top="{tip.y}px" aria-hidden="true">
          <span class="t-ttl">{tip.row.label}</span>
          <span class="t-row">
            <i>{$t('analysis.grossProfit')}</i><b class="up">{signed(tip.row.d.grossWin)}</b>
          </span>
          <span class="t-row">
            <i>{$t('analysis.grossLoss')}</i><b class="down">{signed(-tip.row.d.grossLoss)}</b>
          </span>
          <span class="t-row net-row">
            <i>{$t('analysis.netPnl')}</i>
            <b class:up={tip.row.d.pnl > 0} class:down={tip.row.d.pnl < 0}>{signed(tip.row.d.pnl)}</b>
          </span>
          <span class="t-row">
            <i>{$t('analysis.trades')}</i><b>{tip.row.d.trades}</b>
          </span>
          <span class="t-row">
            <i>{$t('analysis.winRate')}</i><b>{pct(tip.row.d.winRate)}</b>
          </span>
        </div>
      {/if}
    </ul>
    <div class="streaks">
      <span>{$t('analysis.winStreak')} <b class="up">{runs.maxWins}</b></span>
      <span>{$t('analysis.lossStreak')} <b class="down">{runs.maxLosses}</b></span>
      <span>
        {$t('analysis.currentStreak')}
        <b class:up={runs.current > 0} class:down={runs.current < 0}>
          {runs.current > 0 ? `+${runs.current}` : runs.current}
        </b>
      </span>
    </div>
  </section>

  <TradeDist {trades} {s} />
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-3) 0 var(--space-4);
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .heads {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: var(--space-3);
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .head.live {
    padding-left: var(--space-2);
    border-left: 2px solid var(--accent);
  }
  .head .val {
    font-family: var(--mono);
    font-size: var(--text-lg);
    color: var(--text);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  .cells {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-2);
  }
  .cell {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    min-width: 0;
  }
  .cell .val {
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text);
  }
  /* Profits and losses: a mirrored bar per side, losses growing leftwards from the middle. */
  .pl {
    position: relative;
    list-style: none;
    margin: 0 0 var(--space-2);
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .pl li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }
  .pl li:last-child {
    border-bottom: none;
  }
  .side-lbl {
    flex: 0 0 84px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .track {
    flex: 1;
    display: flex;
    min-width: 0;
  }
  .half {
    flex: 1;
    display: flex;
    min-width: 0;
  }
  .half.loss {
    justify-content: flex-end;
  }
  .half .fill {
    height: 6px;
    border-radius: 3px;
  }
  .half.loss .fill {
    background: var(--red);
  }
  .half.win .fill {
    background: var(--green);
  }
  .net {
    flex: 0 0 92px;
    text-align: right;
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--text);
  }
  .pl li:hover .half .fill {
    filter: brightness(1.15);
  }
  /* Hover card: rides the pointer, above the row, and never eats the pointer itself. */
  .tip {
    position: absolute;
    z-index: var(--z-dropdown, 50);
    transform: translate(-50%, calc(-100% - 12px));
    display: grid;
    gap: 2px;
    min-width: 168px;
    padding: var(--space-2);
    background: var(--surface);
    border: 1px solid var(--border-control, var(--border));
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg, 0 8px 24px rgb(0 0 0 / 0.35));
    font-size: var(--text-xs);
    color: var(--muted);
    pointer-events: none;
    white-space: nowrap;
  }
  .t-ttl {
    color: var(--text);
    font-weight: var(--fw-medium);
    margin-bottom: 2px;
  }
  .t-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .t-row b {
    font-family: var(--mono);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .t-row.net-row {
    padding-top: 3px;
    margin-top: 1px;
    border-top: 1px solid var(--border);
  }
  .streaks {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .streaks b {
    font-family: var(--mono);
    color: var(--text);
  }
  .up,
  .head .val.up,
  .cell .val.up,
  .streaks b.up {
    color: var(--green);
  }
  .down,
  .head .val.down,
  .cell .val.down,
  .streaks b.down {
    color: var(--red);
  }
</style>
