<script>
  import Icon from '$lib/ui/Icon.svelte';
  // PnL Calendar — classic month-grid heatmap of daily realized PnL. Green/red cells
  // scaled to the month's largest |day|, a weekly-total column, and click-through to
  // that day's trades. Read-side only: pulls the pre-aggregated daily buckets from
  // /journal/calendar (same filter set as the breakdown) and lays them onto a Monday-
  // first month grid entirely on the client. No schema change.
  import { journalApi, fmtSignedMoney } from './api.js';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let {
    categoryId = '',
    displayCurrency = 'USD',
    // Called with a 'YYYY-MM-DD' string when the user clicks through to a day's trades.
    onviewday = () => {}
  } = $props();

  const signedMoney = (n) => fmtSignedMoney(n, displayCurrency);
  // Dense cells show a whole-currency signed figure (no cents) to stay legible; the
  // tooltip carries the full-precision value.
  const compact = (n) => fmtSignedMoney(n, displayCurrency, 0);

  // ── Month cursor (persisted per browser) ──
  const MONTH_KEY = 'otw.journal.calendar.month.v1';
  const now = new Date();
  let cursor = $state({ year: now.getFullYear(), month: now.getMonth() }); // month 0-11

  (function loadCursor() {
    try {
      const p = JSON.parse(localStorage.getItem(MONTH_KEY) || 'null');
      if (p && Number.isInteger(p.year) && p.month >= 0 && p.month <= 11) cursor = p;
    } catch {
      /* ignore */
    }
  })();
  $effect(() => {
    try {
      localStorage.setItem(MONTH_KEY, JSON.stringify(cursor));
    } catch {
      /* non-fatal */
    }
  });

  function shiftMonth(delta) {
    let y = cursor.year;
    let m = cursor.month + delta;
    if (m < 0) {
      m = 11;
      y -= 1;
    } else if (m > 11) {
      m = 0;
      y += 1;
    }
    cursor = { year: y, month: m };
  }
  function goToday() {
    const d = new Date();
    cursor = { year: d.getFullYear(), month: d.getMonth() };
  }

  const isCurrentMonth = $derived(
    cursor.year === now.getFullYear() && cursor.month === now.getMonth()
  );

  const MONTH_NAMES = [
    'journal.calendar.month.jan', 'journal.calendar.month.feb', 'journal.calendar.month.mar', 'journal.calendar.month.apr',
    'journal.calendar.month.may', 'journal.calendar.month.jun', 'journal.calendar.month.jul', 'journal.calendar.month.aug',
    'journal.calendar.month.sep', 'journal.calendar.month.oct', 'journal.calendar.month.nov', 'journal.calendar.month.dec'
  ];
  // Monday-first weekday headers.
  const WEEKDAYS = [
    'journal.calendar.weekday.mon', 'journal.calendar.weekday.tue', 'journal.calendar.weekday.wed', 'journal.calendar.weekday.thu',
    'journal.calendar.weekday.fri', 'journal.calendar.weekday.sat', 'journal.calendar.weekday.sun'
  ];

  const monthLabel = $derived(`${$t(MONTH_NAMES[cursor.month])} ${cursor.year}`);

  // ── Data ──
  // The endpoint returns the whole (filtered) history; we key days by date and slice the
  // visible month client-side, so navigating months costs nothing after the first load.
  let byDate = $state(new Map()); // 'YYYY-MM-DD' -> { net_pnl, trades, wins, losses }
  let loading = $state(true);
  let loadError = $state('');

  $effect(() => {
    const filter = { category_id: categoryId || undefined };
    loading = true;
    loadError = '';
    journalApi
      .calendar(filter)
      .then((r) => {
        const m = new Map();
        for (const d of r.days ?? []) m.set(d.date, d);
        byDate = m;
      })
      .catch((e) => {
        loadError = e?.message ?? 'failed to load calendar';
      })
      .finally(() => (loading = false));
  });

  const pad = (n) => String(n).padStart(2, '0');
  const iso = (y, m, d) => `${y}-${pad(m + 1)}-${pad(d)}`;

  // ── Month grid ──
  // Rows of 7 cells, Monday-first, with leading/trailing blanks. Each real cell carries
  // its date + the day's bucket (or null for a no-trade day). A trailing "week total" is
  // computed per row.
  const grid = $derived.by(() => {
    const { year, month } = cursor;
    const first = new Date(year, month, 1);
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    // JS getDay(): 0=Sun..6=Sat → Monday-first offset.
    const lead = (first.getDay() + 6) % 7;

    const cells = [];
    for (let i = 0; i < lead; i++) cells.push(null);
    for (let d = 1; d <= daysInMonth; d++) {
      const key = iso(year, month, d);
      cells.push({ day: d, date: key, bucket: byDate.get(key) ?? null });
    }
    while (cells.length % 7 !== 0) cells.push(null);

    const weeks = [];
    for (let i = 0; i < cells.length; i += 7) {
      const row = cells.slice(i, i + 7);
      let total = 0;
      let has = false;
      for (const c of row) {
        if (c?.bucket) {
          total += c.bucket.net_pnl;
          has = true;
        }
      }
      weeks.push({ cells: row, total, has });
    }
    return weeks;
  });

  // Largest |net| across the visible month — the scale for colour intensity, so a quiet
  // month still shows contrast and a big month doesn't blow out every cell.
  const monthMax = $derived.by(() => {
    let max = 0;
    for (const w of grid) {
      for (const c of w.cells) {
        if (c?.bucket) max = Math.max(max, Math.abs(c.bucket.net_pnl));
      }
    }
    return max;
  });

  // Cell tint: green for gains, red for losses, opacity scaled by |net| / monthMax with a
  // floor so a non-zero day is always visible. Zero (scratch) days stay neutral.
  function cellStyle(net) {
    if (!net || monthMax === 0) return '';
    const frac = Math.min(1, Math.abs(net) / monthMax);
    const alpha = (0.14 + 0.66 * frac).toFixed(3);
    const c = net > 0 ? 'var(--green)' : 'var(--red)';
    return `background: color-mix(in srgb, ${c} calc(${alpha} * 100%), transparent);`;
  }

  // ── Month summary (visible-month totals) ──
  const summary = $derived.by(() => {
    let net = 0;
    let trades = 0;
    let wins = 0;
    let losses = 0;
    let green = 0;
    let red = 0;
    for (const w of grid) {
      for (const c of w.cells) {
        if (!c?.bucket) continue;
        net += c.bucket.net_pnl;
        trades += c.bucket.trades;
        wins += c.bucket.wins;
        losses += c.bucket.losses;
        if (c.bucket.net_pnl > 0) green += 1;
        else if (c.bucket.net_pnl < 0) red += 1;
      }
    }
    const decided = wins + losses;
    return {
      net,
      trades,
      green,
      red,
      winRate: decided > 0 ? (wins / decided) * 100 : null
    };
  });

  const todayIso = iso(now.getFullYear(), now.getMonth(), now.getDate());

  function cellTitle(c) {
    if (!c?.bucket) return '';
    const b = c.bucket;
    return `${c.date} · ${signedMoney(b.net_pnl)} · ${$t('journal.calendar.cell.trades', { count: b.trades })}`;
  }
</script>

<div class="cal">
  <header class="cal-head">
    <div class="nav">
      <button class="navbtn" onclick={() => shiftMonth(-1)} title={$t('journal.calendar.prevMonth')} aria-label={$t('journal.calendar.prevMonth')}>
        <Icon name="chevron-left" size={18} />
      </button>
      <h2 class="month">{monthLabel}</h2>
      <button class="navbtn" onclick={() => shiftMonth(1)} title={$t('journal.calendar.nextMonth')} aria-label={$t('journal.calendar.nextMonth')}>
        <Icon name="chevron-right" size={18} />
      </button>
      {#if !isCurrentMonth}
        <button class="today" onclick={goToday}>{$t('journal.calendar.today')}</button>
      {/if}
    </div>

    <div class="summary">
      <span class="sum net" class:pos={summary.net > 0} class:neg={summary.net < 0}>
        {signedMoney(summary.net)}
      </span>
      <span class="sum-sub">
        {$t('journal.calendar.summary.days', { green: summary.green, red: summary.red })}
        · {$t('journal.calendar.summary.trades', { count: summary.trades })}
        {#if summary.winRate != null}· {summary.winRate.toFixed(0)}% {$t('journal.calendar.summary.win')}{/if}
      </span>
    </div>
  </header>

  {#if loading}
    <div class="skeleton"><Skeleton height="340px" /></div>
  {:else if loadError}
    <div class="err">{loadError}</div>
  {:else}
    <div class="grid" role="grid" aria-label={monthLabel}>
      <div class="row head" role="row">
        {#each WEEKDAYS as w}
          <div class="wd" role="columnheader">{$t(w)}</div>
        {/each}
        <div class="wd wt-head" role="columnheader">{$t('journal.calendar.weekTotal')}</div>
      </div>

      {#each grid as week}
        <div class="row" role="row">
          {#each week.cells as c}
            {#if c}
              <button
                class="cell"
                class:today={c.date === todayIso}
                class:has={!!c.bucket}
                style={cellStyle(c.bucket?.net_pnl)}
                title={cellTitle(c)}
                disabled={!c.bucket}
                onclick={() => c.bucket && onviewday(c.date)}
                role="gridcell"
              >
                <span class="daynum">{c.day}</span>
                {#if c.bucket}
                  <span class="pnl" class:pos={c.bucket.net_pnl > 0} class:neg={c.bucket.net_pnl < 0}>
                    {c.bucket.net_pnl === 0 ? $t('journal.calendar.scratch') : compact(c.bucket.net_pnl)}
                  </span>
                  <span class="tcount">{$t('journal.calendar.cell.trades', { count: c.bucket.trades })}</span>
                {/if}
              </button>
            {:else}
              <div class="cell blank" role="gridcell" aria-hidden="true"></div>
            {/if}
          {/each}
          <div class="cell wtotal" class:pos={week.total > 0} class:neg={week.total < 0} role="gridcell">
            {#if week.has}
              <span class="wt-label">{$t('journal.calendar.weekTotal')}</span>
              <span class="wt-val">{signedMoney(week.total)}</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <p class="legend">
      <span class="sw neg"></span>{$t('journal.calendar.legend.loss')}
      <span class="sw zero"></span>{$t('journal.calendar.legend.scratch')}
      <span class="sw pos"></span>{$t('journal.calendar.legend.win')}
      <span class="legend-hint">{$t('journal.calendar.legend.hint')}</span>
    </p>
  {/if}
</div>

<style>
  .cal {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .cal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .navbtn {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
  }
  .navbtn:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .month {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
    letter-spacing: -0.01em;
    min-width: 9.5rem;
    text-align: center;
  }
  .today {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 4px 10px;
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .today:hover {
    color: var(--accent);
    border-color: var(--accent);
  }
  .summary {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
  }
  .sum.net {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
  }
  .sum.net.pos {
    color: var(--green);
  }
  .sum.net.neg {
    color: var(--red);
  }
  .sum-sub {
    font-size: var(--text-sm);
    color: var(--muted);
  }

  .grid {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .row {
    display: grid;
    grid-template-columns: repeat(7, 1fr) 1.15fr;
    gap: 6px;
  }
  .row.head {
    gap: 6px;
  }
  .wd {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    text-align: center;
    padding-bottom: 2px;
    font-weight: var(--fw-semibold);
  }
  .wt-head {
    text-align: right;
    padding-right: 4px;
  }

  .cell {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    min-height: 74px;
    padding: 6px 8px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    text-align: left;
    color: var(--text);
    font: inherit;
    overflow: hidden;
  }
  .cell.blank {
    background: transparent;
    border-color: transparent;
  }
  button.cell.has {
    cursor: pointer;
    transition: transform 0.08s ease, box-shadow 0.08s ease;
  }
  button.cell.has:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18);
  }
  button.cell:disabled {
    cursor: default;
  }
  .cell.today {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .daynum {
    font-size: 0.72rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .cell.today .daynum {
    color: var(--accent);
    font-weight: var(--fw-semibold);
  }
  .pnl {
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    line-height: 1.1;
    margin-top: auto;
  }
  .pnl.pos {
    color: var(--green);
  }
  .pnl.neg {
    color: var(--red);
  }
  .tcount {
    font-size: 0.62rem;
    color: var(--muted);
  }

  .wtotal {
    align-items: flex-end;
    justify-content: center;
    background: var(--surface-2, var(--surface));
    min-height: 74px;
  }
  .wt-label {
    font-size: 0.6rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .wt-val {
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
  }
  .wtotal.pos .wt-val {
    color: var(--green);
  }
  .wtotal.neg .wt-val {
    color: var(--red);
  }

  .legend {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
    flex-wrap: wrap;
  }
  .sw {
    display: inline-block;
    width: 12px;
    height: 12px;
    border-radius: 3px;
    border: 1px solid var(--border);
  }
  .sw:not(:first-child) {
    margin-left: var(--space-3);
  }
  .sw.neg {
    background: color-mix(in srgb, var(--red) 55%, transparent);
  }
  .sw.zero {
    background: var(--surface);
  }
  .sw.pos {
    background: color-mix(in srgb, var(--green) 55%, transparent);
  }
  .legend-hint {
    margin-left: auto;
    font-style: italic;
  }

  .err {
    color: var(--red);
    padding: var(--space-4);
  }
  .skeleton {
    padding: var(--space-2) 0;
  }

  @media (max-width: 640px) {
    .cell,
    .wtotal {
      min-height: 58px;
      padding: 4px 5px;
    }
    .tcount {
      display: none;
    }
  }
</style>
