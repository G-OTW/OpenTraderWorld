<script>
  // Trading routine widget: today's due routine checklists, tickable inline. Uses the
  // routines board endpoint (which already scopes to today) and per-item check calls.
  import { traderApi } from '$lib/modules/routines/api.js';
  import { periodStats, fmtLocal } from '$lib/ui/consistency.js';
  import { dateKey, EM_DASH } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';
  import Buckets from './parts/Buckets.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import { dayLabels } from './parts/axis.js';
  import Stat from './parts/Stat.svelte';

  let { item, editing } = $props();
  const limit = $derived(Math.max(1, Math.min(50, item.config?.limit ?? 12)));
  const variant = $derived(item.config?.variant ?? 'today');
  const period = $derived(item.config?.period ?? 'week');

  // Read the day at call time, not at module init. A dashboard left open across midnight
  // would otherwise load yesterday's board — and, worse, `toggle()` would record the tick
  // against yesterday. dateKey() builds the key from local parts, so it does not lean on
  // 'en-CA' happening to order its parts as ISO does.
  let routines = $state(null);
  let tasks = $state([]);
  let marks = $state([]);
  let templates = $state(null);
  let err = $state('');
  let busy = $state(new Set());

  async function load() {
    err = '';
    try {
      const board = await traderApi.board(dateKey());
      routines = board.routines ?? [];
      tasks = board.tasks ?? [];
      marks = board.marks ?? [];
    } catch (e) {
      err = e.message;
    }
  }

  // The template counter is the only card that needs the templates themselves; the board
  // only carries the ones due today.
  $effect(() => {
    if (editing || variant !== 'templates') return;
    let alive = true;
    traderApi
      .listRoutines()
      .then((r) => { if (alive) templates = r.routines ?? r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // ── Derived cards ──
  const allItems = $derived((routines ?? []).flatMap((r) => r.items ?? []));
  const doneItems = $derived(allItems.filter((i) => i.checked).length);
  const todayPct = $derived(allItems.length ? Math.round((doneItems / allItems.length) * 100) : 0);

  const markDays = $derived(new Set((marks ?? []).map((m) => m.day)));
  // Consecutive marked days ending today (or yesterday, so a day still in progress does
  // not read as a broken streak).
  const streak = $derived.by(() => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    if (!markDays.has(fmtLocal(d))) d.setDate(d.getDate() - 1);
    let n = 0;
    while (markDays.has(fmtLocal(d))) {
      n++;
      d.setDate(d.getDate() - 1);
    }
    return n;
  });
  // One bar per day over the last four weeks: marked or not.
  const streakBars = $derived.by(() => {
    const out = [];
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    for (let i = 27; i >= 0; i--) {
      const day = new Date(d);
      day.setDate(d.getDate() - i);
      out.push(markDays.has(fmtLocal(day)) ? 1 : 0.15);
    }
    return out;
  });

  const stats = $derived(periodStats(marks, period));

  const taskCounts = $derived.by(() => {
    const today = dateKey();
    let open = 0;
    let overdue = 0;
    let done = 0;
    for (const t of tasks ?? []) {
      if (t.done) done++;
      else if (t.due_date && t.due_date < today) overdue++;
      else open++;
    }
    return { open, overdue, done };
  });
  $effect(() => {
    if (!editing) load();
  });

  async function toggle(r, it) {
    if (busy.has(it.id)) return;
    busy = new Set(busy).add(it.id);
    try {
      await traderApi.checkItem(it.id, dateKey(), !it.checked);
      routines = routines.map((x) =>
        x.id !== r.id ? x : { ...x, items: x.items.map((i) => (i.id === it.id ? { ...i, checked: !i.checked } : i)) }
      );
    } catch (e) {
      err = e.message;
    } finally {
      const n = new Set(busy);
      n.delete(it.id);
      busy = n;
    }
  }
</script>

<WidgetState
  {editing}
  error={err}
  loading={routines === null}
  empty={variant === 'today' && routines?.length === 0}
  preview={$t('dashboard.widgets.routines.preview')}
  emptyText={$t('dashboard.widgets.routines.empty')}
  rows={4}
>
  {#if variant === 'streak'}
    <div class="w-body">
      <Stat value={String(streak)} note={$t('dashboard.widgets.routines.daysActive')} />
      <MiniBars values={streakBars} labels={dayLabels(streakBars.length)} tone="pos" height={52}
        valueFormat={(v) => (v === 1 ? $t('common.done') : EM_DASH)}
        label={$t('dashboard.widgets.routines.daysActive')} />
    </div>
  {:else if variant === 'completion'}
    <Donut
      segments={[
        { label: $t('dashboard.widgets.routines.completed'), value: stats.done, color: 'var(--green)' },
        { label: $t('dashboard.widgets.routines.remaining'), value: Math.max(0, stats.total - stats.done), color: 'var(--chart-4)' }
      ]}
      extra={[{ label: $t('dashboard.widgets.routines.total'), value: stats.total }]}
      center={`${stats.total ? Math.round((stats.done / stats.total) * 100) : 0}%`}
      centerLabel={$t('dashboard.widgets.routines.completedShort')}
      showPct={false}
    />
  {:else if variant === 'templates'}
    {#if templates === null}
      <p class="w-state">…</p>
    {:else}
      <div class="w-body">
        <Stat value={String(templates.length)} note={$t('dashboard.widgets.routines.templates')} />
        <ul class="w-list">
          <li class="w-row">
            <span class="w-dot on"></span>
            <span class="w-name grow">{$t('dashboard.widgets.routines.active')}</span>
            <span class="w-num">{templates.filter((r) => r.active).length}</span>
          </li>
          <li class="w-row">
            <span class="w-dot"></span>
            <span class="w-name grow">{$t('dashboard.widgets.routines.inactive')}</span>
            <span class="w-num">{templates.filter((r) => !r.active).length}</span>
          </li>
        </ul>
      </div>
    {/if}
  {:else if variant === 'tasks'}
    <Buckets
      cells={[
        { value: taskCounts.open, label: $t('dashboard.widgets.routines.open') },
        { value: taskCounts.overdue, label: $t('dashboard.widgets.routines.overdue'), tone: 'neg' },
        { value: taskCounts.done, label: $t('dashboard.widgets.routines.done'), tone: 'pos' }
      ]}
    />
  {:else}
  {#if allItems.length > 0}
    <div class="overall">
      <span class="w-eyebrow">{$t('dashboard.widgets.routines.todayProgress', { done: doneItems, total: allItems.length })}</span>
      <div class="w-bar" class:pos={todayPct === 100} role="progressbar"
        aria-valuenow={todayPct} aria-valuemin="0" aria-valuemax="100">
        <span style:width={`${todayPct}%`}></span>
      </div>
    </div>
  {/if}
  {#each routines as r (r.id)}
    {@const done = r.items.filter((i) => i.checked).length}
    {@const pct = r.items.length ? Math.round((done / r.items.length) * 100) : 0}
    <section class="w-group">
      <div class="head">
        <h4 class="w-eyebrow">{r.name}</h4>
        <span class="w-num count">{done}/{r.items.length}</span>
      </div>
      <div class="w-bar" class:pos={pct === 100} role="progressbar"
        aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100" aria-label={r.name}>
        <span style:width={`${pct}%`}></span>
      </div>
      <ul class="w-list">
        {#each r.items.slice(0, limit) as it (it.id)}
          <li class="w-row">
            <label class:done={it.checked}>
              <input type="checkbox" checked={it.checked} disabled={busy.has(it.id)} onchange={() => toggle(r, it)} />
              <span>{it.label}</span>
            </label>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
  }
  .w-dot.on {
    background: var(--green);
  }
  .overall {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }
  h4 {
    margin: 0;
  }
  .count {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  /* Progress sits above its items, so the bar needs the row rhythm's breathing space. */
  .w-bar {
    margin-bottom: var(--space-1);
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-width: 0;
    cursor: pointer;
  }
  label span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A ticked item stays legible but stops competing: struck through AND dimmed, so
     the state does not rest on the line alone. */
  label.done span {
    color: var(--faint);
    text-decoration: line-through;
  }
</style>
