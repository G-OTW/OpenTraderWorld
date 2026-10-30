<script>
  // Calendar widget: a compact scrollable list of events from today through the next 7 days,
  // grouped by day. Config: { limit } caps the number of events shown.
  import { calendarApi } from '$lib/modules/calendar/api.js';
  import { remindApi } from '$lib/modules/remindme/api.js';
  import { todosApi } from '$lib/modules/todos/api.js';
  import { goalsApi } from '$lib/modules/goals/api.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { tip } from '$lib/ui/tip.svelte.js';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'week');
  const limit = $derived(item.config?.limit ?? 12);
  const days = $derived(Math.max(1, Math.min(90, item.config?.days ?? 7)));
  // The cards that measure a period need the whole window, not the first N rows.
  const capped = $derived(['week', 'upcoming', 'dueSoon'].includes(variant));

  let events = $state(null);
  let err = $state('');

  // Only the newest request in flight writes: refreshes are seconds apart.
  let seq = 0;
  async function load() {
    const mine = ++seq;
    err = '';
    try {
      // The month grid needs the whole calendar month, past days included; every other
      // card looks forward from today.
      const now = new Date();
      now.setHours(0, 0, 0, 0);
      const start = variant === 'month' ? new Date(now.getFullYear(), now.getMonth(), 1) : now;
      const end =
        variant === 'month'
          ? new Date(now.getFullYear(), now.getMonth() + 1, 1)
          : new Date(now.getTime() + days * 86400000);
      const list = await calendarApi.list(start.toISOString(), end.toISOString());
      if (mine !== seq) return;
      const sorted = list.slice().sort((a, b) => new Date(a.start_at) - new Date(b.start_at));
      events = capped ? sorted.slice(0, limit) : sorted;
    } catch (e) {
      if (mine === seq) err = e.message;
    }
  }
  const live = livePulse(LIVE.agenda);
  $effect(() => {
    void variant;
    void days;
    live.n;
    if (!editing) load();
  });

  // The cross-module card is the calendar's overlay sources, read from the modules that
  // own them — the same three the calendar page overlays.
  let overlay = $state(null);
  $effect(() => {
    live.n;
    if (editing || variant !== 'dueSoon') return;
    let alive = true;
    Promise.all([remindApi.list(), todosApi.list(), goalsApi.list()])
      .then(([reminders, todos, goals]) => {
        if (!alive) return;
        const now = Date.now();
        const horizon = now + days * 86400000;
        const rows = [
          ...reminders
            .filter((r) => r.active && r.next_fire_at)
            .map((r) => ({ id: `r${r.id}`, name: r.name, at: new Date(r.next_fire_at).getTime(),
              kind: $t('dashboard.widgets.calendar.kindReminder'), icon: 'bell', href: '/remindme' })),
          ...todos
            .filter((td) => !td.done && td.due_date)
            .map((td) => ({ id: `t${td.id}`, name: td.name,
              at: new Date(`${td.due_date}T${td.due_time ?? '00:00'}`).getTime(),
              kind: $t('dashboard.widgets.calendar.kindTodo'), icon: 'check-square', href: '/todos' })),
          ...goals
            .filter((g) => g.deadline)
            .map((g) => ({ id: `g${g.id}`, name: g.name, at: new Date(`${g.deadline}T00:00`).getTime(),
              kind: $t('dashboard.widgets.calendar.kindGoal'), icon: 'target', href: '/goals' }))
        ];
        overlay = rows
          .filter((x) => x.at >= now - 86400000 && x.at <= horizon)
          .sort((a, b) => a.at - b.at)
          .slice(0, limit);
      })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const startOfToday = () => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d;
  };
  const todayCount = $derived.by(() => {
    const from = startOfToday().getTime();
    const to = from + 86400000;
    return (events ?? []).filter((e) => {
      const at = new Date(e.start_at).getTime();
      return at >= from && at < to;
    }).length;
  });
  const weekCount = $derived.by(() => {
    const from = startOfToday().getTime();
    const to = from + 7 * 86400000;
    return (events ?? []).filter((e) => {
      const at = new Date(e.start_at).getTime();
      return at >= from && at < to;
    }).length;
  });

  const categories = $derived.by(() => {
    const counts = new Map();
    for (const e of events ?? []) {
      const key = e.category?.trim() || $t('dashboard.widgets.calendar.uncategorized');
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
  });

  // Mini month: the visible month's weeks, Monday first, with a dot on any day that
  // carries an event inside the loaded window.
  const monthGrid = $derived.by(() => {
    const today = startOfToday();
    const first = new Date(today.getFullYear(), today.getMonth(), 1);
    const lead = (first.getDay() + 6) % 7;
    const cells = [];
    const byDay = new Map();
    for (const e of events ?? []) {
      const d = new Date(e.start_at);
      const k = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
      if (!byDay.has(k)) byDay.set(k, []);
      byDay.get(k).push(e);
    }
    for (let i = 0; i < 42; i++) {
      const d = new Date(first);
      d.setDate(1 - lead + i);
      const events_ = byDay.get(`${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`) ?? [];
      cells.push({
        key: d.toISOString().slice(0, 10),
        date: new Date(d),
        day: d.getDate(),
        other: d.getMonth() !== today.getMonth(),
        today: d.getTime() === today.getTime(),
        marked: events_.length > 0,
        events: events_
      });
    }
    return cells;
  });
  const monthLabel = $derived(
    new Date().toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  );
  const weekdayLabels = $derived.by(() => {
    // Monday-first initials, taken from a known Monday so the locale supplies the names.
    const base = new Date(2024, 0, 1);
    return Array.from({ length: 7 }, (_, i) => {
      const d = new Date(base);
      d.setDate(base.getDate() + i);
      return d.toLocaleDateString(undefined, { weekday: 'short' });
    });
  });

  function whenLabel(at) {
    const d = new Date(at);
    const today = startOfToday();
    const diff = Math.round((new Date(d).setHours(0, 0, 0, 0) - today) / 86400000);
    const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    if (diff === 0) return `${$t('dashboard.widgets.calendar.today')} ${time}`;
    if (diff === 1) return `${$t('dashboard.widgets.calendar.tomorrow')} ${time}`;
    return `${d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })} ${time}`;
  }

  function dayLabel(iso) {
    const d = new Date(iso);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const dd = new Date(d);
    dd.setHours(0, 0, 0, 0);
    const diff = Math.round((dd - today) / 86400000);
    if (diff === 0) return $t('dashboard.widgets.calendar.today');
    if (diff === 1) return $t('dashboard.widgets.calendar.tomorrow');
    return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
  }
  function timeLabel(e) {
    if (e.all_day) return $t('dashboard.widgets.calendar.allDay');
    return new Date(e.start_at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }

  // Hovering a day reads what sits on it: the date, then one row per event (time and
  // title), capped so a busy day cannot grow a tooltip past the card.
  const TIP_MAX = 6;
  let hoverDay = $state('');
  function dayTip(event, cell) {
    hoverDay = cell.key;
    const shown = cell.events.slice(0, TIP_MAX);
    const rows = shown.map((e) => ({
      color: e.color || 'var(--accent)',
      label: e.title,
      value: timeLabel(e)
    }));
    if (cell.events.length > shown.length) {
      rows.push({ label: '', value: `+${cell.events.length - shown.length}` });
    }
    tip.show(event, {
      title: cell.date.toLocaleDateString(undefined, {
        weekday: 'long', day: 'numeric', month: 'long', year: 'numeric'
      }),
      rows: rows.length ? rows : [{ label: '', value: $t('dashboard.widgets.calendar.empty') }]
    });
  }

  // Group consecutive events by day label.
  const groups = $derived.by(() => {
    const out = [];
    for (const e of events ?? []) {
      const lbl = dayLabel(e.start_at);
      let g = out[out.length - 1];
      if (!g || g.label !== lbl) out.push((g = { label: lbl, items: [] }));
      g.items.push(e);
    }
    return out;
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={events === null}
  empty={['week', 'upcoming'].includes(variant) && events?.length === 0}
  preview={$t('dashboard.widgets.calendar.preview')}
  emptyText={$t('dashboard.widgets.calendar.empty')}
  rows={4}
>
  {#if variant === 'upcoming'}
    <ul class="w-list">
      {#each (events ?? []).slice(0, limit) as e (e.id)}
        <li class="w-row">
          <span class="w-dot" style:background={e.color || 'var(--accent)'}></span>
          <span class="w-name grow">{e.title}</span>
          <span class="w-sub when">{whenLabel(e.start_at)}</span>
        </li>
      {/each}
    </ul>
  {:else if variant === 'counts'}
    <div class="w-split">
      <div class="cell">
        <span class="fig">{todayCount}</span>
        <span class="w-sub">{$t('dashboard.widgets.calendar.eventsToday')}</span>
      </div>
      <div class="cell">
        <span class="fig">{weekCount}</span>
        <span class="w-sub">{$t('dashboard.widgets.calendar.eventsThisWeek')}</span>
      </div>
    </div>
  {:else if variant === 'category'}
    <Donut segments={categories} centerLabel={$t('dashboard.widgets.calendar.events')} />
  {:else if variant === 'month'}
    <div class="w-body">
      <span class="w-eyebrow">{monthLabel}</span>
      <div class="month" role="group" aria-label={monthLabel}
        onpointerleave={() => { hoverDay = ''; tip.hide(); }}>
        {#each weekdayLabels as w (w)}<span class="wd">{w}</span>{/each}
        {#each monthGrid as c (c.key)}
          <span
            class="day"
            class:other={c.other}
            class:now={c.today}
            class:on={hoverDay === c.key}
            role="img"
            aria-label={`${c.key}: ${c.events.length}`}
            onpointerenter={(e) => dayTip(e, c)}
            onpointermove={(e) => tip.move(e)}
          >
            {c.day}
            {#if c.marked}<span class="mark"></span>{/if}
          </span>
        {/each}
      </div>
    </div>
  {:else if variant === 'dueSoon'}
    {#if overlay === null}
      <p class="w-state">…</p>
    {:else if overlay.length === 0}
      <p class="w-state">{$t('dashboard.widgets.calendar.empty')}</p>
    {:else}
      <div class="w-list">
        {#each overlay as x (x.id)}
          <a class="w-row" href={x.href}>
            <Icon name={x.icon} size={15} />
            <span class="w-name grow">{x.name}</span>
            <span class="w-sub kind">{x.kind}</span>
            <span class="w-sub when">{whenLabel(x.at)}</span>
          </a>
        {/each}
      </div>
    {/if}
  {:else}
  {#each groups as g (g.label)}
    <section class="w-group">
      <h4 class="w-eyebrow">{g.label}</h4>
      <ul class="w-list">
        {#each g.items as e (e.id)}
          <li class="w-row">
            <span class="w-dot" style:background={e.color || 'var(--accent)'}></span>
            <span class="w-num time">{timeLabel(e)}</span>
            <span class="w-name">{e.title}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
  {/if}
</WidgetState>

<style>
  h4 {
    margin: 0;
  }
  .grow {
    flex: 1;
  }
  .when,
  .kind {
    flex-shrink: 0;
  }
  .w-row :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  /* .w-split owns the direction and the separator: stacked in a narrow card, columns
     once the card is wide. */
  .cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 26px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.015em;
    color: var(--text);
  }
  /* Seven columns, weekday heads then six weeks — a month at a glance, not a date picker. */
  .month {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
    text-align: center;
  }
  .wd {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    padding-bottom: 2px;
  }
  .day {
    position: relative;
    padding: 4px 0 7px;
    border-radius: var(--radius-sm);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    color: var(--text);
  }
  .day.other {
    color: var(--faint);
  }
  .day.now {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .day.on:not(.now) {
    background: var(--surface-2);
  }
  .mark {
    position: absolute;
    left: 50%;
    bottom: 3px;
    transform: translateX(-50%);
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--accent);
  }
  .day.now .mark {
    background: var(--accent-contrast);
  }
  /* Times are a fixed-width gutter so titles start on one vertical line. */
  .time {
    min-width: 46px;
    text-align: left;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .w-name {
    flex: 1;
  }
</style>
