<script>
  // One trading day as a report, opened from a calendar cell: the day's figures in the
  // backtest's KPI grid, the intraday realized P&L curve, the check-ins condensed, the
  // routines owed and done, the positions and every fill stamped on the day.
  //
  // Read-only. Which trades belong to the day is the server's answer, on the same local
  // effective date the calendar buckets by; money comes from the analytics points (display
  // currency), prices and quantities from the trades themselves (their own units).
  import Icon from '$lib/ui/Icon.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import DayPnlChart from './DayPnlChart.svelte';
  import { journalApi, fmtSignedMoney, fmtMoney, fmtNum } from './api.js';
  import { mindsetApi } from '$lib/modules/mindset/api.js';
  import { traderApi } from '$lib/modules/routines/api.js';
  import { MINDSET_PREF } from './mindsetDays.js';
  import { dayBounds, dayStats, pnlCurve, byTicker, dayFills, mindsetDigest } from './dayReport.js';
  import { t, locale } from '$lib/i18n';
  import { pref } from '$lib/prefs.svelte.js';

  let {
    date = $bindable(''),
    categoryId = '',
    displayCurrency = 'USD',
    tags = [],
    onback = () => {},
    // Opens the editable Trades list pinned to this day.
    ontrades = () => {}
  } = $props();

  const money = (n) => fmtSignedMoney(n, displayCurrency);
  const nativeMoney = (n, ccy) => fmtSignedMoney(n, ccy);
  const num = (v, d = 2) => fmtNum(v, d);
  const qty = (v) => (v == null ? fmtNum(null) : Number(v).toLocaleString(undefined, { maximumFractionDigits: 8 }));
  const price = (v) => (v == null ? fmtNum(null) : Number(v).toLocaleString(undefined, { maximumFractionDigits: 6 }));
  const hhmm = (ms) => (ms == null ? '' : new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }));
  function hold(min) {
    if (min == null) return fmtNum(null);
    const m = Math.round(min);
    if (m < 60) return `${m}m`;
    if (m < 1440) return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
    return `${Math.floor(m / 1440)}d ${Math.floor((m % 1440) / 60)}h`;
  }

  const longDate = $derived(
    date
      ? new Date(`${date}T12:00:00`).toLocaleDateString($locale, {
          weekday: 'long',
          day: 'numeric',
          month: 'long',
          year: 'numeric'
        })
      : ''
  );

  // ── Data ──
  let loading = $state(true);
  let loadError = $state('');
  let analytics = $state(null);
  let trades = $state([]);
  let hist = $state(null);
  let routines = $state(null); // null = not read (no book picked), [] = none owed
  let tradedDays = $state([]); // sorted 'YYYY-MM-DD' with a trade, for the day arrows

  const bounds = $derived(dayBounds(date));

  // Only the latest read lands: a slower answer for a previous day or book is dropped.
  let daysSeq = 0;
  let readSeq = 0;

  // The traded days of the book, once per book: the arrows step to the next one.
  $effect(() => {
    const seq = ++daysSeq;
    const filter = { category_id: categoryId || undefined };
    journalApi
      .calendar(filter)
      .then((r) => seq === daysSeq && (tradedDays = (r.days ?? []).map((d) => d.date).sort()))
      .catch(() => seq === daysSeq && (tradedDays = []));
  });

  $effect(() => {
    const d = date;
    const cat = categoryId;
    const seq = ++readSeq;
    if (!d) return;
    const { since, until } = dayBounds(d);
    loading = true;
    loadError = '';
    const scope = { category_id: cat || undefined };
    // The fills of the day can belong to a position closed later, so the trades are read
    // from the day onward and placed client-side; the figures stay on the day's own set.
    Promise.all([
      journalApi.analytics({ ...scope, since, until }),
      journalApi.listTrades({ ...scope, since }),
      mindsetApi.history(d, d).catch(() => null),
      cat ? journalApi.routineStatus(cat, d, d).catch(() => []) : Promise.resolve(null)
    ])
      .then(([a, tr, h, rs]) => {
        if (seq !== readSeq) return;
        analytics = a;
        trades = tr ?? [];
        hist = h;
        routines = rs == null ? null : (rs.find((x) => x.date === d)?.routines ?? []);
      })
      .catch((e) => seq === readSeq && (loadError = e?.message ?? 'failed to load the day'))
      .finally(() => seq === readSeq && (loading = false));
  });

  const points = $derived(analytics?.points ?? []);
  const stats = $derived(dayStats(points));
  const curve = $derived(pnlCurve(points));
  const tickers = $derived(byTicker(points));
  const pointById = $derived(new Map(points.map((p) => [p.id, p])));
  const fills = $derived(dayFills(trades, bounds.start, bounds.end));
  const tagById = $derived(new Map(tags.map((tg) => [tg.id, tg])));

  // The positions of the day: the trades whose effective instant falls on it.
  const effectiveAt = (tr) => new Date(tr.exit_at ?? tr.entry_at ?? tr.created_at).getTime();
  const positions = $derived(
    trades
      .filter((tr) => {
        const at = effectiveAt(tr);
        return at >= bounds.start && at <= bounds.end;
      })
      .sort((a, b) => effectiveAt(a) - effectiveAt(b))
      .map((tr) => {
        const p = pointById.get(tr.id);
        const open = (tr.open_qty ?? 0) > 1e-12;
        return {
          tr,
          p,
          status: !open ? 'closed' : tr.net_pnl != null ? 'partial' : 'open',
          entryMs: tr.entry_at ? new Date(tr.entry_at).getTime() : null,
          exitMs: tr.exit_at ? new Date(tr.exit_at).getTime() : null,
          tags: (tr.tags ?? []).map((id) => tagById.get(id)).filter(Boolean)
        };
      })
  );
  const stillOpen = $derived(positions.filter((x) => x.status !== 'closed').length);

  // ── Mindset ──
  const mindPicks = pref(MINDSET_PREF, {});
  const picked = $derived(categoryId ? (mindPicks.value?.[categoryId] ?? []) : []);
  const mind = $derived(mindsetDigest(hist, date, picked));
  const traded = $derived(positions.length > 0);
  const tone = (score) => (score == null ? '' : score >= 67 ? 'pos' : score < 40 ? 'neg' : 'mid');

  const routinesDone = $derived((routines ?? []).filter((r) => r.done).length);

  // ── Detail modals: one check-in's answers, or one routine's checklist for the day ──
  // The day's mindset in one sheet: every check-in, the ones left unfilled and the note.
  // Opening it from one check-in scrolls that one into view.
  let ciOpen = $state(false);
  function openCheckin(tpl) {
    ciOpen = true;
    if (tpl?.id) {
      requestAnimationFrame(() =>
        document.getElementById(`dci-${tpl.id}`)?.scrollIntoView({ block: 'start', behavior: 'smooth' })
      );
    }
  }

  // The checklist and its ticks belong to the Trading routines module: its day board is
  // read once per day, on the first click.
  let rtOpen = $state(false);
  let rtRow = $state(null);
  let rtBoard = $state(null); // { date, routines } | null
  let rtError = $state('');
  const rtView = $derived(rtRow && rtBoard?.date === date ? (rtBoard.routines.find((r) => r.id === rtRow.routine_id) ?? null) : null);
  function openRoutine(row) {
    rtRow = row;
    rtOpen = true;
    rtError = '';
    if (rtBoard?.date === date) return;
    const d = date;
    traderApi
      .board(d)
      .then((b) => d === date && (rtBoard = { date: d, routines: b.routines ?? [] }))
      .catch((e) => (rtError = e?.message ?? 'failed to load the routine'));
  }

  // ── Day arrows ──
  const prevDay = $derived(tradedDays.filter((x) => x < date).at(-1) ?? null);
  const nextDay = $derived(tradedDays.find((x) => x > date) ?? null);

  // ── KPI tiles: the backtest's hero grid, read on one day ──
  const hero = $derived([
    {
      label: $t('journal.day.kpi.net'),
      value: money(stats.net),
      signed: stats.net,
      sub: $t('journal.day.kpi.netSub', { gross: money(stats.gross), fees: money(-stats.fees) })
    },
    {
      label: $t('journal.day.kpi.winRate'),
      value: stats.winRate == null ? fmtNum(null) : `${num(stats.winRate, 0)}%`,
      sub: $t('journal.day.kpi.winRateSub', { wins: stats.wins, losses: stats.losses, trades: stats.trades })
    },
    {
      label: $t('journal.day.kpi.pf'),
      value: stats.profitFactor === Infinity ? $t('journal.day.kpi.noLoss') : num(stats.profitFactor),
      signed: stats.profitFactor == null ? null : stats.profitFactor - 1,
      sub: $t('journal.day.kpi.pfSub', { value: money(stats.expectancy) })
    },
    {
      label: $t('journal.day.kpi.payoff'),
      value: num(stats.payoff),
      sub: $t('journal.day.kpi.payoffSub', { win: money(stats.avgWin), loss: money(stats.avgLoss) })
    },
    {
      label: $t('journal.day.kpi.drawdown'),
      value: stats.drawdown > 0 ? money(-stats.drawdown) : money(0),
      signed: -stats.drawdown,
      sub: $t('journal.day.kpi.drawdownSub', { low: money(curve.low), high: money(curve.high) })
    },
    stats.rCount > 0
      ? {
          label: $t('journal.day.kpi.r'),
          value: `${stats.rTotal >= 0 ? '+' : ''}${num(stats.rTotal)}R`,
          signed: stats.rTotal,
          sub: $t('journal.day.kpi.rSub', { avg: num(stats.avgR), count: stats.rCount })
        }
      : {
          label: $t('journal.day.kpi.hold'),
          value: hold(stats.avgHold),
          sub: $t('journal.day.kpi.holdSub', { count: stats.trades })
        }
  ]);

  const details = $derived([
    { k: $t('journal.day.detail.best'), v: money(stats.best), s: stats.best },
    { k: $t('journal.day.detail.worst'), v: money(stats.worst), s: stats.worst },
    { k: $t('journal.side.long'), v: $t('journal.day.detail.sideValue', { count: stats.long.n, net: money(stats.long.net) }), s: stats.long.n ? stats.long.net : null },
    { k: $t('journal.side.short'), v: $t('journal.day.detail.sideValue', { count: stats.short.n, net: money(stats.short.net) }), s: stats.short.n ? stats.short.net : null },
    { k: $t('journal.day.detail.hold'), v: hold(stats.avgHold) },
    { k: $t('journal.day.detail.fills'), v: String(fills.length) },
    { k: $t('journal.day.detail.open'), v: String(stillOpen) },
    {
      k: $t('journal.day.detail.window'),
      v: fills.length ? `${hhmm(fills[0].at)} · ${hhmm(fills[fills.length - 1].at)}` : fmtNum(null)
    },
    ...(analytics?.mistakes?.tagged
      ? [
          {
            k: $t('journal.day.detail.mistakes'),
            v: $t('journal.day.detail.mistakesValue', {
              count: analytics.mistakes.tagged,
              net: money(analytics.mistakes.tagged_net)
            }),
            s: analytics.mistakes.tagged_net
          }
        ]
      : [])
  ]);
</script>

{#snippet answerList(list, full)}
  <dl class="ans" class:full>
    {#each list as a (a.id)}
      <div class:wide={a.kind === 'text' || a.kind === 'tags'}>
        <dt>{a.label}</dt>
        <dd>
          {#if a.kind === 'scale'}
            <span class="pips {tone(a.norm)}" title="{a.value}/5">
              {#each [1, 2, 3, 4, 5] as i (i)}<span class:on={i <= a.value}></span>{/each}
            </span>
            {#if full}<span class="sv">{a.value}/5</span>{/if}
          {:else if a.kind === 'tags'}
            <span class="chips">
              {#each Array.isArray(a.value) ? a.value : [a.value] as tg (tg)}<span class="chip">{tg}</span>{/each}
            </span>
          {:else if a.kind === 'text'}
            <span class="quote">{a.value}</span>
          {:else}
            <span class={tone(a.norm)}>{a.value}</span>
          {/if}
        </dd>
      </div>
    {/each}
  </dl>
{/snippet}

<div class="day">
  <header class="head">
    <div class="nav">
      <button class="btn" onclick={onback}>
        <Icon name="arrow-left" size={15} />
        {$t('journal.day.back')}
      </button>
      <button class="navbtn" disabled={!prevDay} onclick={() => prevDay && (date = prevDay)} title={$t('journal.day.prevDay')} aria-label={$t('journal.day.prevDay')}>
        <Icon name="chevron-left" size={18} />
      </button>
      <div class="title">
        <h2>{longDate}</h2>
        <span class="sub">
          {$t('journal.calendar.cell.trades', { count: positions.length })}
          {#if analytics?.unconverted_trades}
            · {$t('journal.day.unconverted', { count: analytics.unconverted_trades })}
          {/if}
        </span>
      </div>
      <button class="navbtn" disabled={!nextDay} onclick={() => nextDay && (date = nextDay)} title={$t('journal.day.nextDay')} aria-label={$t('journal.day.nextDay')}>
        <Icon name="chevron-right" size={18} />
      </button>
    </div>
    <div class="actions">
      <a class="btn" href="/mindset?date={date}">
        <Icon name="brain" size={15} />
        {$t('journal.day.mindset.open')}
      </a>
      <button class="btn" onclick={() => ontrades(date)}>
        <Icon name="receipt" size={15} />
        {$t('journal.day.openTrades')}
      </button>
    </div>
  </header>

  {#if loading && !analytics}
    <Skeleton height="96px" />
    <Skeleton height="280px" />
  {:else if loadError}
    <div class="err">{loadError}</div>
  {:else}
    <div class="hero" class:stale={loading}>
      {#each hero as c (c.label)}
        <div class="kpi" class:pos={c.signed > 0} class:neg={c.signed < 0}>
          <span class="label">{c.label}</span>
          <span class="value">{c.value}</span>
          {#if c.sub}<span class="ksub">{c.sub}</span>{/if}
        </div>
      {/each}
    </div>

    <div class="cols">
      <section class="panel chart">
        <h3 class="ph">{$t('journal.day.chart.title')}</h3>
        {#if curve.steps.length}
          <DayPnlChart {curve} fmt={money} axis={(n) => fmtMoney(n, displayCurrency, 0)} />
        {:else}
          <p class="muted pad">{$t('journal.day.chart.empty')}</p>
        {/if}
      </section>

      <aside class="side">
        <section class="panel">
          <h3 class="ph">
            <button class="phbtn" onclick={() => openCheckin(null)} disabled={mind.templates.length === 0 && mind.missing.length === 0 && !mind.note}>
              {$t('journal.day.mindset.title')}
              <Icon name="maximize" size={12} />
            </button>
            {#if mind.score != null}
              <span class="score {tone(mind.score)}">{Math.round(mind.score)}</span>
            {/if}
          </h3>
          {#if mind.score != null}
            <div class="meter" aria-hidden="true">
              <span class="fill {tone(mind.score)}" style="width: {Math.max(2, mind.score)}%"></span>
            </div>
          {/if}
          {#if mind.templates.length === 0 && mind.missing.length === 0}
            <p class="muted">{$t('journal.day.mindset.none')}</p>
          {/if}
          <ul class="ci">
            {#each mind.templates as tpl (tpl.id)}
              <li>
                <button class="row" onclick={() => openCheckin(tpl)}>
                  <span class="mk done"><Icon name="check" size={12} /></span>
                  <span class="nm">{tpl.name}</span>
                  <span class="val">{tpl.score == null ? '' : Math.round(tpl.score)}</span>
                  <Icon name="chevron-right" size={14} />
                </button>
              </li>
            {/each}
            {#each mind.missing as tpl (tpl.id)}
              <li class="missing" class:hard={traded}>
                <button class="row" onclick={() => openCheckin(tpl)}>
                  <span class="mk"><Icon name="x" size={12} /></span>
                  <span class="nm">{tpl.name}</span>
                  <span class="val">{$t('journal.day.mindset.notFilled')}</span>
                  <Icon name="chevron-right" size={14} />
                </button>
              </li>
            {/each}
          </ul>
        </section>

        <section class="panel">
          <h3 class="ph">
            {$t('journal.day.routines.title')}
            {#if routines?.length}
              <span class="score" class:pos={routinesDone === routines.length} class:neg={routinesDone === 0 && traded} class:mid={routinesDone > 0 && routinesDone < routines.length}>
                {$t('journal.day.routines.summary', { done: routinesDone, total: routines.length })}
              </span>
            {/if}
          </h3>
          {#if routines == null}
            <p class="muted">{$t('journal.day.routines.pickBook')}</p>
          {:else if routines.length === 0}
            <p class="muted">{$t('journal.day.routines.none')}</p>
          {:else}
            <ul class="ci">
              {#each routines as r (r.link_id)}
                <li class:missing={!r.done} class:hard={!r.done && traded}>
                  <button class="row" onclick={() => openRoutine(r)}>
                    <span class="mk" class:done={r.done}><Icon name={r.done ? 'check' : 'x'} size={12} /></span>
                    <span class="nm">{r.name}</span>
                    <span class="val">{r.done ? $t('journal.routines.done') : $t('journal.routines.notDone')}</span>
                    <Icon name="chevron-right" size={14} />
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      </aside>
    </div>

    <div class="cols even">
      <section class="panel">
        <h3 class="ph">{$t('journal.day.detail.title')}</h3>
        <dl class="kv">
          {#each details as d (d.k)}
            <div>
              <dt>{d.k}</dt>
              <dd class:pos={d.s > 0} class:neg={d.s < 0}>{d.v}</dd>
            </div>
          {/each}
        </dl>
        {#if tickers.length}
          <table class="tbl compact">
            <thead>
              <tr>
                <th>{$t('journal.day.col.instrument')}</th>
                <th class="r">{$t('journal.day.col.trades')}</th>
                <th class="r">{$t('journal.day.col.win')}</th>
                <th class="r">{$t('journal.day.col.net')}</th>
              </tr>
            </thead>
            <tbody>
              {#each tickers as row (row.ticker)}
                <tr>
                  <td class="tk">{row.ticker}</td>
                  <td class="r">{row.trades}</td>
                  <td class="r">{row.wins}/{row.trades}</td>
                  <td class="r mono" class:pos={row.net > 0} class:neg={row.net < 0}>{money(row.net)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>

      <section class="panel">
        <h3 class="ph">{$t('journal.day.mindset.answers')}</h3>
        {#if mind.templates.length === 0 && !mind.note}
          <p class="muted">{$t('journal.day.mindset.none')}</p>
        {/if}
        {#each mind.templates as tpl (tpl.id)}
          <div class="tpl">
            <div class="tpl-head">
              <button class="tpl-name link" onclick={() => openCheckin(tpl)}>{tpl.name}</button>
              {#if tpl.phase}<span class="phase">{$t(`mindset.phase.${tpl.phase}`)}</span>{/if}
              {#if tpl.score != null}<span class="score {tone(tpl.score)}">{Math.round(tpl.score)}</span>{/if}
            </div>
            {@render answerList(tpl.answers, false)}
          </div>
        {/each}
        {#if mind.note}
          <div class="tpl">
            <div class="tpl-head"><span class="tpl-name">{$t('journal.day.mindset.note')}</span></div>
            <p class="quote note">{mind.note}</p>
          </div>
        {/if}
      </section>
    </div>

    <section class="panel">
      <h3 class="ph">{$t('journal.day.positions.title')}</h3>
      {#if positions.length === 0}
        <EmptyState icon="receipt" title={$t('journal.day.positions.empty')} compact />
      {:else}
        <div class="scroll">
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('journal.day.col.time')}</th>
                <th>{$t('journal.day.col.instrument')}</th>
                <th>{$t('journal.day.col.side')}</th>
                <th class="r">{$t('journal.day.col.qty')}</th>
                <th class="r">{$t('journal.day.col.entry')}</th>
                <th class="r">{$t('journal.day.col.exit')}</th>
                <th class="r">{$t('journal.day.col.hold')}</th>
                <th class="r">R</th>
                <th class="r">{$t('journal.day.col.net')}</th>
              </tr>
            </thead>
            <tbody>
              {#each positions as row (row.tr.id)}
                {@const net = row.p ? row.p.net : row.tr.net_pnl}
                <tr>
                  <td class="mono dim">
                    {hhmm(row.entryMs)}{#if row.exitMs} · {hhmm(row.exitMs)}{/if}
                  </td>
                  <td>
                    <span class="tk">{row.tr.ticker}</span>
                    {#if row.status !== 'closed'}<span class="badge">{$t(`journal.day.status.${row.status}`)}</span>{/if}
                    {#each row.tags as tg (tg.id)}<span class="chip tag" class:mistake={tg.kind === 'mistake'}>{tg.name}</span>{/each}
                    {#if row.p?.strategy}<span class="strat">{row.p.strategy}</span>{/if}
                  </td>
                  <td><span class="side {row.tr.side}">{$t(`journal.side.${row.tr.side}`)}</span></td>
                  <td class="r mono">{qty(row.tr.quantity)}</td>
                  <td class="r mono">{price(row.tr.avg_entry ?? row.tr.entry_price)}</td>
                  <td class="r mono">{price(row.tr.exit_price)}</td>
                  <td class="r mono dim">{hold(row.p?.hold_min)}</td>
                  <td class="r mono" class:pos={row.p?.r > 0} class:neg={row.p?.r < 0}>{row.p?.r == null ? '' : num(row.p.r)}</td>
                  <td class="r mono strong" class:pos={net > 0} class:neg={net < 0}>
                    {net == null ? '' : row.p ? money(net) : nativeMoney(net, row.tr.currency)}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>

    <section class="panel">
      <h3 class="ph">{$t('journal.day.fills.title')}</h3>
      {#if fills.length === 0}
        <EmptyState icon="list" title={$t('journal.day.fills.empty')} compact />
      {:else}
        <div class="scroll">
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('journal.day.col.time')}</th>
                <th>{$t('journal.day.col.instrument')}</th>
                <th>{$t('journal.day.col.action')}</th>
                <th class="r">{$t('journal.day.col.qty')}</th>
                <th class="r">{$t('journal.day.col.price')}</th>
                <th class="r">{$t('journal.day.col.fees')}</th>
              </tr>
            </thead>
            <tbody>
              {#each fills as f (f.key)}
                <tr>
                  <td class="mono dim">{new Date(f.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</td>
                  <td>
                    <span class="tk">{f.ticker}</span>
                    {#if f.signal}<span class="strat">{f.signal}</span>{/if}
                  </td>
                  <td>
                    <span class="act {f.action}">{$t(`journal.day.fills.${f.action}`)}</span>
                    <span class="dim">{$t(`journal.day.fills.${f.effect}`)}</span>
                  </td>
                  <td class="r mono">{qty(f.qty)}</td>
                  <td class="r mono">{price(f.price)}</td>
                  <td class="r mono dim">{f.fees ? `${num(f.fees)} ${f.currency}` : ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>
  {/if}
</div>

<!-- The day's mindset in full: score, every check-in answer by answer, the unfilled ones
     and the day note. Answering stays in the Mindset module. -->
<Modal bind:open={ciOpen} title={$t('journal.day.mindset.sheetTitle', { date: longDate })} size="lg" fit>
  {@const filled = mind.templates.length}
  {@const owed = filled + mind.missing.length}
  <div class="sheet">
    <div class="msum">
      <div class="mtile">
        <span class="label">{$t('journal.day.mindset.score')}</span>
        <span class="mbig {tone(mind.score)}">{mind.score == null ? fmtNum(null) : Math.round(mind.score)}<small>/100</small></span>
        <div class="meter" aria-hidden="true">
          <span class="fill {tone(mind.score)}" style="width: {mind.score == null ? 0 : Math.max(2, mind.score)}%"></span>
        </div>
      </div>
      <div class="mtile">
        <span class="label">{$t('journal.day.mindset.filled')}</span>
        <span class="mbig" class:pos={owed && filled === owed} class:mid={filled > 0 && filled < owed} class:neg={filled === 0 && traded}>{filled}<small>/{owed}</small></span>
        <span class="ksub">{$t('journal.day.mindset.answerCount', { count: mind.templates.reduce((n, tpl) => n + tpl.answers.length, 0) })}</span>
      </div>
      <div class="mtile">
        <span class="label">{$t('journal.day.kpi.net')}</span>
        <span class="mbig" class:pos={stats.net > 0} class:neg={stats.net < 0}>{money(stats.net)}</span>
        <span class="ksub">{$t('journal.day.kpi.winRateSub', { wins: stats.wins, losses: stats.losses, trades: stats.trades })}</span>
      </div>
    </div>

    <div class="cards">
      {#each mind.templates as tpl (tpl.id)}
        <section class="card" id="dci-{tpl.id}">
          <header class="card-head">
            <span class="card-title">{tpl.name}</span>
            {#if tpl.phase}<span class="card-phase">{$t(`mindset.phase.${tpl.phase}`)}</span>{/if}
            {#if tpl.score != null}<span class="score {tone(tpl.score)}">{Math.round(tpl.score)}</span>{/if}
          </header>
          {#if tpl.score != null}
            <div class="meter thin" aria-hidden="true">
              <span class="fill {tone(tpl.score)}" style="width: {Math.max(2, tpl.score)}%"></span>
            </div>
          {/if}
          {#if tpl.answers.length}
            <ul class="qa">
              {#each tpl.answers as a (a.id)}
                <li>
                  <span class="q">{a.label}</span>
                  <span class="a">
                    {#if a.kind === 'scale'}
                      <span class="pips {tone(a.norm)}">
                        {#each [1, 2, 3, 4, 5] as i (i)}<span class:on={i <= a.value}></span>{/each}
                      </span>
                      <span class="sv">{a.value}/5</span>
                    {:else if a.kind === 'tags'}
                      <span class="chips">
                        {#each Array.isArray(a.value) ? a.value : [a.value] as tg (tg)}<span class="chip">{tg}</span>{/each}
                      </span>
                    {:else if a.kind === 'text'}
                      <span class="txt">{a.value}</span>
                    {:else}
                      <span class="opt {tone(a.norm)}">{a.value}</span>
                    {/if}
                  </span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="muted">{$t('journal.day.mindset.none')}</p>
          {/if}
        </section>
      {/each}
      {#each mind.missing as tpl (tpl.id)}
        <section class="card empty" class:hard={traded} id="dci-{tpl.id}">
          <header class="card-head">
            <span class="card-title">{tpl.name}</span>
            {#if tpl.phase}<span class="card-phase">{$t(`mindset.phase.${tpl.phase}`)}</span>{/if}
            <span class="score" class:neg={traded}>{$t('journal.day.mindset.notFilled')}</span>
          </header>
          <a class="fill-link" href="/mindset?date={date}">
            <Icon name="pencil" size={13} />
            {$t('journal.day.mindset.fill')}
          </a>
        </section>
      {/each}
    </div>

    {#if mind.note}
      <section class="card note">
        <header class="card-head"><span class="card-title">{$t('journal.day.mindset.note')}</span></header>
        <p class="txt">{mind.note}</p>
      </section>
    {/if}
  </div>
  {#snippet footer()}
    <a class="btn" href="/mindset?date={date}">
      <Icon name="brain" size={15} />
      {$t('journal.day.mindset.open')}
    </a>
  {/snippet}
</Modal>

<!-- One routine's checklist as ticked that day, read from the Trading routines board. -->
<Modal bind:open={rtOpen} title={rtRow?.name ?? ''} size="md" fit>
  <div class="sheet">
    {#if rtError}
      <div class="err">{rtError}</div>
    {:else if !rtBoard || rtBoard.date !== date}
      <Skeleton height="120px" />
    {:else if !rtView}
      <p class="muted">{$t('journal.day.routines.gone')}</p>
    {:else}
      {@const ticked = rtView.items.filter((it) => it.checked).length}
      {@const total = rtView.items.length}
      {@const rtTone = total && ticked === total ? 'pos' : ticked === 0 ? (traded ? 'neg' : '') : 'mid'}
      <div class="sheet-head">
        <span class="sheet-meta">{longDate}</span>
        <span class="score {rtTone}">{$t('journal.day.routines.steps', { done: ticked, total })}</span>
      </div>
      <div class="meter" aria-hidden="true">
        <span class="fill {rtTone}" style="width: {total ? Math.max(2, (ticked / total) * 100) : 0}%"></span>
      </div>
      {#if rtView.description}<p class="sheet-desc">{rtView.description}</p>{/if}
      <ul class="steps">
        {#each rtView.items as it (it.id)}
          <li class:done={it.checked}>
            <span class="box" class:on={it.checked}>{#if it.checked}<Icon name="check" size={11} />{/if}</span>
            <span class="stxt">
              <span class="slabel">{it.label}</span>
              {#if it.note}<span class="snote">{it.note}</span>{/if}
              {#if it.url}
                <a class="slink" href={it.url} target="_blank" rel="noopener noreferrer">
                  <Icon name="external-link" size={11} />
                  {it.link_label || it.url}
                </a>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
      {#if rtView.notes}<p class="sheet-notes">{rtView.notes}</p>{/if}
    {/if}
  </div>
  {#snippet footer()}
    <a class="btn" href="/routines?date={date}">
      <Icon name="check-square" size={15} />
      {$t('journal.day.routines.open')}
    </a>
  {/snippet}
</Modal>

<style>
  /* Clickable checklist rows: the whole line opens its detail. */
  .ci .row {
    text-decoration: none;
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 2px 0;
    background: none;
    border: none;
    border-radius: 0;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .ci .row :global(svg:last-child) {
    color: var(--dim);
  }
  .ci .row:hover .nm {
    color: var(--accent);
  }
  .ci li:has(> .row) {
    display: block;
  }
  .tpl-name.link {
    padding: 0;
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .tpl-name.link:hover {
    color: var(--accent);
  }
  .phbtn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0;
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }
  .phbtn:hover:not(:disabled) {
    color: var(--accent);
  }
  .phbtn:disabled {
    cursor: default;
  }
  .phbtn:disabled :global(svg) {
    display: none;
  }

  /* Day mindset sheet: three summary tiles on the KPI filet grid, then one card per
     check-in. */
  .msum {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .mtile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px 14px;
    background: var(--bg);
    min-width: 0;
  }
  .mbig {
    font-family: var(--mono);
    font-size: 20px;
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
  }
  .mbig small {
    font-size: 12px;
    color: var(--dim);
    font-weight: var(--fw-normal);
    margin-left: 2px;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: var(--surface);
    border: 0.5px solid var(--border);
    min-width: 0;
    scroll-margin-top: var(--space-4);
  }
  .card.empty {
    background: transparent;
    border-style: dashed;
  }
  .card.empty.hard {
    border-color: color-mix(in srgb, var(--red) 45%, transparent);
  }
  .card-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .card-title {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .card-phase {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .card-head .score {
    margin-left: auto;
  }
  .card .qa {
    border-top: none;
  }
  .card .qa li:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }
  .meter.thin {
    height: 2px;
  }
  .fill-link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    width: fit-content;
    font-size: var(--text-sm);
    color: var(--accent);
    text-decoration: none;
  }
  .fill-link:hover {
    text-decoration: underline;
  }
  .card.note .txt {
    margin: 0;
  }

  /* Detail sheets (check-in, routine): a dated head with its score, a thin meter, then
     one hairline-separated row per answer or step. */
  .sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .sheet-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .sheet-meta {
    font-size: var(--text-xs);
    color: var(--dim);
    text-transform: capitalize;
  }
  .sheet-head .score {
    margin-left: auto;
  }
  .sheet-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .sheet-notes {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
    white-space: pre-wrap;
    background: var(--surface-2);
  }
  .qa,
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 0.5px solid var(--border);
  }
  .qa li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-4);
    padding: 9px 0;
    border-bottom: 0.5px solid var(--border);
  }
  .qa li:has(.txt) {
    grid-template-columns: minmax(0, 1fr);
    gap: 4px;
  }
  .q {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .a {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    min-width: 0;
  }
  .opt {
    font-weight: var(--fw-medium);
  }
  .txt {
    font-size: var(--text-sm);
    color: var(--text);
    white-space: pre-wrap;
  }
  .steps li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: start;
    gap: var(--space-3);
    padding: 8px 0;
    border-bottom: 0.5px solid var(--border);
  }
  .box {
    display: grid;
    place-items: center;
    width: 15px;
    height: 15px;
    margin-top: 2px;
    border: 1px solid var(--border-control);
    color: var(--bg);
  }
  .box.on {
    background: var(--green);
    border-color: var(--green);
  }
  .stxt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .slabel {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .steps li:not(.done) .slabel {
    color: var(--muted);
  }
  .snote {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .slink {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    width: fit-content;
    font-size: var(--text-xs);
    color: var(--accent);
    text-decoration: none;
  }
  .slink:hover {
    text-decoration: underline;
  }
  a.btn {
    text-decoration: none;
  }
  .day {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .nav,
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title {
    display: flex;
    flex-direction: column;
    min-width: 13rem;
    text-align: center;
  }
  .title h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    text-transform: capitalize;
  }
  .title .sub {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .navbtn {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    cursor: pointer;
  }
  .navbtn:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .navbtn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  a.btn {
    text-decoration: none;
  }

  /* KPI row: the backtest StatsGrid's filet grid, cells on --bg separated by hairlines. */
  .hero {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
    transition: opacity var(--dur-fast) var(--ease);
  }
  .hero.stale {
    opacity: 0.6;
  }
  .kpi {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--bg);
    padding: 14px 16px;
    min-width: 0;
  }
  .label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .value {
    font-family: var(--mono);
    font-size: 15px;
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .kpi.pos .value {
    color: var(--green);
  }
  .kpi.neg .value {
    color: var(--red);
  }
  .ksub {
    font-size: 11.5px;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cols {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
    gap: var(--space-4);
    align-items: start;
  }
  .cols.even {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    background: var(--bg);
    border: 0.5px solid var(--border);
    min-width: 0;
  }
  .ph {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: 10px;
    font-weight: var(--fw-normal);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .ph .score {
    margin-left: auto;
  }
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .muted.pad {
    padding: var(--space-6) 0;
    text-align: center;
  }
  .err {
    color: var(--red);
    padding: var(--space-4);
  }

  .mono {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .dim {
    color: var(--dim);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .mid {
    color: var(--amber);
  }

  /* Score chip: the day's or a check-in's 0..100, coloured by band. */
  .score {
    font-family: var(--mono);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    letter-spacing: 0;
    text-transform: none;
    padding: 0 6px;
    border: 0.5px solid var(--border-control);
    color: var(--text);
  }
  .score.pos {
    color: var(--green);
    border-color: color-mix(in srgb, var(--green) 55%, transparent);
  }
  .score.neg {
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 55%, transparent);
  }
  .score.mid {
    color: var(--amber);
    border-color: color-mix(in srgb, var(--amber) 55%, transparent);
  }
  .meter {
    height: 4px;
    background: var(--surface-2);
  }
  .meter .fill {
    display: block;
    height: 100%;
    background: var(--chart-1);
  }
  .meter .fill.pos {
    background: var(--green);
  }
  .meter .fill.neg {
    background: var(--red);
  }
  .meter .fill.mid {
    background: var(--amber);
  }

  /* Checklist rows: check-ins and routines read the same way. */
  .ci {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .ci li {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .ci .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ci .val {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .mk {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    color: var(--muted);
    border: 0.5px solid var(--border-control);
  }
  .mk.done {
    color: var(--green);
    border-color: color-mix(in srgb, var(--green) 55%, transparent);
  }
  .ci li.missing .nm {
    color: var(--muted);
  }
  .ci li.missing.hard .mk,
  .ci li.missing.hard .val {
    color: var(--red);
  }
  .ci li.missing.hard .mk {
    border-color: color-mix(in srgb, var(--red) 55%, transparent);
  }

  /* Key / value list: PerfTable's two-column read. */
  .kv {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.5px;
    margin: 0;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .kv div {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 8px 10px;
    background: var(--bg);
    min-width: 0;
  }
  .kv dt {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .kv dd {
    margin: 0;
    font-family: var(--mono);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  /* Condensed check-in answers. */
  .tpl {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
    border-bottom: 0.5px solid var(--border);
  }
  .tpl:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }
  .tpl-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .tpl-name {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
  }
  .phase {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .tpl-head .score {
    margin-left: auto;
  }
  .ans {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 4px var(--space-4);
    margin: 0;
  }
  .ans div {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .ans div.wide {
    grid-column: 1 / -1;
    flex-direction: column;
    align-items: flex-start;
  }
  .ans dt {
    font-size: var(--text-sm);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ans dd {
    margin: 0;
    font-size: var(--text-sm);
  }
  .pips {
    display: inline-flex;
    gap: 2px;
  }
  .pips span {
    width: 8px;
    height: 8px;
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
  }
  .pips span.on {
    background: var(--chart-1);
    border-color: var(--chart-1);
  }
  .pips.pos span.on {
    background: var(--green);
    border-color: var(--green);
  }
  .pips.neg span.on {
    background: var(--red);
    border-color: var(--red);
  }
  .pips.mid span.on {
    background: var(--amber);
    border-color: var(--amber);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    font-size: var(--text-xs);
    padding: 0 6px;
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    color: var(--text);
  }
  .chip.tag {
    margin-left: 6px;
  }
  .chip.mistake {
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 55%, transparent);
  }
  .quote {
    font-size: var(--text-sm);
    color: var(--text);
    font-style: italic;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .quote.note {
    margin: 0;
    padding-left: var(--space-3);
    border-left: 2px solid var(--border-control);
  }

  /* Tables */
  .scroll {
    overflow-x: auto;
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  .tbl th {
    font-size: 10px;
    font-weight: var(--fw-normal);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    text-align: left;
    padding: 6px 8px;
    border-bottom: 0.5px solid var(--border);
    white-space: nowrap;
  }
  .tbl td {
    padding: 7px 8px;
    border-bottom: 0.5px solid var(--border);
    white-space: nowrap;
  }
  .tbl tbody tr:last-child td {
    border-bottom: none;
  }
  .tbl tbody tr:hover td {
    background: var(--surface-2);
  }
  .tbl.compact td {
    padding: 5px 8px;
  }
  .r {
    text-align: right !important;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .tk {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .strat {
    margin-left: 6px;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .badge {
    margin-left: 6px;
    font-size: var(--text-xs);
    padding: 0 5px;
    color: var(--amber);
    border: 0.5px solid color-mix(in srgb, var(--amber) 55%, transparent);
  }
  .side.long,
  .act.buy {
    color: var(--green);
  }
  .side.short,
  .act.sell {
    color: var(--red);
  }
  .act {
    font-weight: var(--fw-medium);
    margin-right: 4px;
  }

  @media (max-width: 960px) {
    .cols,
    .cols.even {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 640px) {
    .head {
      flex-direction: column;
      align-items: stretch;
    }
    .nav {
      justify-content: space-between;
    }
    .title {
      min-width: 0;
      flex: 1;
    }
    .kv,
    .ans {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
