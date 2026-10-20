<script>
  // Reminder widgets — the cards of the RemindMe mockup, each a `variant`. Two reads:
  // `list()` for the reminders themselves and `notifications()` for what has already
  // fired. The RemindMe module is untouched.
  //
  // Config: { variant, limit, days }.
  import { remindApi, kindLabel } from '$lib/modules/remindme/api.js';
  import { fmtDateTime, dateKey, fmtNum, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import { dayLabels } from './parts/axis.js';
  import Donut from './parts/Donut.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'unread');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));
  const days = $derived(Math.max(7, Math.min(60, item.config?.days ?? 14)));

  const needsReminders = $derived(['upcoming', 'active', 'overdue', 'mostTriggered'].includes(variant));
  const needsNotifs = $derived(['unread', 'feed', 'mostTriggered'].includes(variant));

  let reminders = $state(null);
  let notifs = $state.raw(null);
  let err = $state('');

  const live = livePulse(LIVE.quotes);
  $effect(() => {
    live.n;
    if (editing || !needsReminders) return;
    let alive = true;
    remindApi.list()
      .then((r) => { if (alive) reminders = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });
  $effect(() => {
    live.n;
    if (editing || !needsNotifs) return;
    let alive = true;
    remindApi.notifications(200)
      .then((r) => { if (alive) notifs = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const all = $derived(reminders ?? []);
  const rows = $derived(notifs?.notifications ?? []);

  // One bar per day over the window: how many notifications landed that day.
  const perDay = $derived.by(() => {
    const counts = new Map();
    for (const n of rows) counts.set(dateKey(n.created_at), (counts.get(dateKey(n.created_at)) ?? 0) + 1);
    const out = [];
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    for (let i = days - 1; i >= 0; i--) {
      const day = new Date(d);
      day.setDate(d.getDate() - i);
      out.push(counts.get(dateKey(day)) ?? 0);
    }
    return out;
  });

  const upcoming = $derived(
    all
      .filter((r) => r.active && r.next_fire_at)
      .sort((a, b) => a.next_fire_at.localeCompare(b.next_fire_at))
      .slice(0, limit)
  );

  const byKind = $derived.by(() => {
    const counts = new Map();
    for (const r of all) counts.set(r.kind, (counts.get(r.kind) ?? 0) + 1);
    return [...counts].map(([key, value]) => ({ key, label: kindLabel(key), value }))
      .sort((a, b) => b.value - a.value);
  });

  // Overdue = should have fired and has not; exhausted = it has used up its allowance.
  const stalled = $derived.by(() => {
    const now = Date.now();
    return all
      .map((r) => {
        const exhausted = r.max_count != null && r.fired_count >= r.max_count;
        const late = r.active && r.next_fire_at && new Date(r.next_fire_at).getTime() < now;
        const days2 = late ? Math.floor((now - new Date(r.next_fire_at).getTime()) / 86400000) : 0;
        return { r, exhausted, late, days: days2 };
      })
      .filter((x) => x.exhausted || x.late)
      .sort((a, b) => b.days - a.days)
      .slice(0, limit);
  });

  const top = $derived([...all].sort((a, b) => b.fired_count - a.fired_count)[0] ?? null);
  // Bars for the winner: how often it landed per day over the window.
  const topBars = $derived.by(() => {
    if (!top) return [];
    const counts = new Map();
    for (const n of rows) {
      if (n.reminder_id !== top.id) continue;
      counts.set(dateKey(n.created_at), (counts.get(dateKey(n.created_at)) ?? 0) + 1);
    }
    const out = [];
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    for (let i = days - 1; i >= 0; i--) {
      const day = new Date(d);
      day.setDate(d.getDate() - i);
      out.push(counts.get(dateKey(day)) ?? 0);
    }
    return out;
  });

  const loading = $derived(
    (needsReminders && reminders === null) || (needsNotifs && notifs === null)
  );
</script>

<WidgetState
  {editing}
  error={err}
  loading={loading}
  preview={$t('dashboard.widgets.remind.preview')}
  rows={4}
>
  {#if variant === 'unread'}
    <div class="w-body">
      <Stat value={String(notifs.unread ?? 0)} note={$t('dashboard.widgets.remind.unread')} />
      <MiniBars values={perDay} labels={dayLabels(perDay.length)} tone="accent" height={52}
        valueFormat={(v) => fmtNum(v, 0)} label={$t('dashboard.widgets.remind.unread')} />
    </div>
  {:else if variant === 'upcoming'}
    {#if upcoming.length === 0}
      <p class="w-state">{$t('dashboard.widgets.remind.noUpcoming')}</p>
    {:else}
      <ul class="w-list">
        {#each upcoming as r (r.id)}
          <li class="w-row">
            <span class="w-dot"></span>
            <span class="w-name grow">{r.name}</span>
            <span class="w-sub when">{fmtDateTime(r.next_fire_at)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'active'}
    <div class="w-body">
      <Donut
        segments={[
          { label: $t('dashboard.widgets.remind.active'), value: all.filter((r) => r.active).length, color: 'var(--green)' },
          { label: $t('dashboard.widgets.remind.inactive'), value: all.filter((r) => !r.active).length, color: 'var(--chart-4)' }
        ]}
        center={String(all.length)}
        centerLabel={$t('dashboard.widgets.remind.total')}
        showPct={false}
      />
      <ul class="w-list">
        {#each byKind as k (k.key)}
          <li class="w-row">
            <span class="w-name grow">{k.label}</span>
            <span class="w-num">{k.value}</span>
          </li>
        {/each}
      </ul>
    </div>
  {:else if variant === 'overdue'}
    {#if stalled.length === 0}
      <p class="w-state">{$t('dashboard.widgets.remind.noStalled')}</p>
    {:else}
      <ul class="w-list">
        {#each stalled as x (x.r.id)}
          <li class="w-row late">
            <Icon name="alert-triangle" size={15} />
            <span class="w-name grow">{x.r.name}</span>
            <span class="w-num w-neg">
              {x.exhausted
                ? $t('dashboard.widgets.remind.exhausted')
                : $t('dashboard.widgets.remind.overdueDays', { days: x.days })}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'feed'}
    <ul class="w-list">
      {#each rows.slice(0, limit) as n (n.id)}
        <li class="w-row">
          <span class="w-name grow" class:unread={!n.read_at}>{n.name}</span>
          <span class="w-sub when">{$ago(n.created_at)}</span>
        </li>
      {/each}
    </ul>
  {:else if variant === 'mostTriggered'}
    {#if !top}
      <p class="w-state">{$t('dashboard.widgets.remind.noneYet')}</p>
    {:else}
      <div class="w-body">
        <span class="w-name topname">{top.name}</span>
        <Stat value={String(top.fired_count)} note={$t('dashboard.widgets.remind.timesTriggered')} />
        {#if topBars.some((v) => v > 0)}
          <MiniBars values={topBars} labels={dayLabels(topBars.length)} tone="pos" height={44}
            valueFormat={(v) => fmtNum(v, 0)} label={top.name} />
        {/if}
      </div>
    {/if}
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
  }
  .when {
    flex-shrink: 0;
  }
  .unread {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .late :global(svg) {
    color: var(--red);
    flex-shrink: 0;
  }
  .topname {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
</style>
