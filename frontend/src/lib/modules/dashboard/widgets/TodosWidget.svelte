<script>
  // ToDo widgets. One component, six data presentations (the `variant` in the saved
  // config): the completion donut, the due-date buckets, the upcoming table, the overdue
  // list, the category donut and the undated backlog.
  //
  // Everything is derived from the one `todos.list()` call the module already exposes —
  // no new endpoint, no change to the ToDo module itself.
  import { todosApi, dueState } from '$lib/modules/todos/api.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';
  import Buckets from './parts/Buckets.svelte';
  import MiniBars from './parts/MiniBars.svelte';

  // "Mar 14" — a due date has to fit a pill at the end of a row, so it drops the
  // year that $lib/format's fmtDate carries.
  const shortDay = (iso) =>
    new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

  let { item, editing } = $props();
  const limit = $derived(item.config?.limit ?? 8);
  const variant = $derived(item.config?.variant ?? 'upcoming');
  // Older saved widgets stored the presentation in `scope`; honour it as the variant.
  const scope = $derived(item.config?.scope ?? variant);

  let todos = $state(null);
  let err = $state('');
  let busy = $state(new Set());

  async function load() {
    err = '';
    try {
      todos = await todosApi.list();
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const all = $derived(todos ?? []);
  const done = $derived(all.filter((td) => td.done).length);
  const openCount = $derived(all.length - done);
  const rate = $derived(all.length ? Math.round((done / all.length) * 100) : 0);

  const byDue = $derived.by(() => {
    const b = { overdue: 0, today: 0, soon: 0, undated: 0 };
    for (const td of all) {
      if (td.done) continue;
      if (!td.due_date) b.undated++;
      else if (dueState(td.due_date) === 'overdue') b.overdue++;
      else if (dueState(td.due_date) === 'today') b.today++;
      else if (dueState(td.due_date) === 'soon') b.soon++;
    }
    return b;
  });

  // Share of the whole list, which is what the mockup's small percentage under each
  // bucket reads as — not a share of the open tasks alone.
  const share = (n) => (all.length ? `${Math.round((n / all.length) * 100)}%` : '0%');

  const categories = $derived.by(() => {
    const counts = new Map();
    for (const td of all) {
      const key = td.category?.trim() || $t('dashboard.widgets.todos.uncategorized');
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
  });

  // Undated open tasks, grouped by category — the backlog's shape, not a fake time series.
  const backlog = $derived(all.filter((td) => !td.done && !td.due_date));
  const backlogBars = $derived.by(() => {
    const counts = new Map();
    for (const td of backlog) {
      const key = td.category?.trim() || '—';
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return [...counts]
      .map(([label, value]) => ({ label, value }))
      .sort((a, b) => b.value - a.value);
  });

  const open = $derived(
    all
      .filter((td) => !td.done && (scope !== 'overdue' || dueState(td.due_date) === 'overdue'))
      .sort((a, b) => (a.due_date ?? '9999').localeCompare(b.due_date ?? '9999'))
      .slice(0, limit)
  );

  async function toggle(td) {
    if (busy.has(td.id)) return;
    busy = new Set(busy).add(td.id);
    try {
      await todosApi.setDone(td.id, true);
      todos = todos.map((x) => (x.id === td.id ? { ...x, done: true } : x));
    } catch (e) {
      err = e.message;
    } finally {
      const n = new Set(busy);
      n.delete(td.id);
      busy = n;
    }
  }

  const emptyByVariant = $derived(
    scope === 'overdue' ? $t('dashboard.widgets.todos.emptyOverdue') : $t('dashboard.widgets.todos.empty')
  );
</script>

<WidgetState
  {editing}
  error={err}
  loading={todos === null}
  empty={all.length === 0 || (scope === 'upcoming' && open.length === 0) || (scope === 'overdue' && open.length === 0)}
  preview={$t('dashboard.widgets.todos.preview')}
  emptyText={emptyByVariant}
  rows={4}
>
  {#if scope === 'status'}
    <Donut
      segments={[
        { label: $t('dashboard.widgets.todos.done'), value: done, color: 'var(--green)' },
        { label: $t('dashboard.widgets.todos.open'), value: openCount, color: 'var(--chart-4)' }
      ]}
      extra={[{ label: $t('dashboard.widgets.todos.total'), value: all.length }]}
      center={`${rate}%`}
      centerLabel={$t('dashboard.widgets.todos.completed')}
      showPct={false}
    />
  {:else if scope === 'buckets'}
    <Buckets
      cells={[
        { value: byDue.overdue, label: $t('dashboard.widgets.todos.overdueLabel'), tone: 'neg', sub: share(byDue.overdue) },
        { value: byDue.today, label: $t('dashboard.widgets.todos.dueToday'), tone: 'warn', sub: share(byDue.today) },
        { value: byDue.soon, label: $t('dashboard.widgets.todos.dueSoon'), tone: 'accent', sub: share(byDue.soon) }
      ]}
    />
  {:else if scope === 'category'}
    <Donut segments={categories} centerLabel={$t('dashboard.widgets.todos.tasks')} />
  {:else if scope === 'backlog'}
    <div class="w-body">
      <div class="w-metric">
        <span class="w-metric-value">{backlog.length}</span>
        <span class="w-metric-note">{$t('dashboard.widgets.todos.noDueDate')}</span>
      </div>
      {#if backlogBars.length > 1}
        <div class="w-fill bars">
          <MiniBars
            values={backlogBars.map((b) => b.value)}
            labels={backlogBars.map((b) => (b.label === '—' ? $t('dashboard.widgets.todos.uncategorized') : b.label))}
            tone="accent"
            valueFormat={(v) => String(v)}
            label={$t('dashboard.widgets.todos.tasks')}
          />
        </div>
      {/if}
    </div>
  {:else if scope === 'upcoming'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th colspan="2">{$t('dashboard.widgets.todos.task')}</th>
          <th>{$t('dashboard.widgets.todos.dueDate')}</th>
          <th>{$t('dashboard.widgets.todos.category')}</th>
        </tr>
      </thead>
      <tbody>
        {#each open as td (td.id)}
          {@const due = dueState(td.due_date)}
          <tr>
            <td class="tick">
              <button class="w-check" disabled={busy.has(td.id)} onclick={() => toggle(td)}
                aria-label={$t('dashboard.widgets.todos.markDone')}></button>
            </td>
            <td class="w-name">{td.name}</td>
            <td class:w-neg={due === 'overdue' || due === 'today'}>
              {td.due_date ? shortDay(td.due_date) : '—'}
            </td>
            <td>
              {#if td.category}
                <span class="w-pill">{td.category}</span>
              {:else}
                <span class="w-sub">—</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <ul class="w-list">
      {#each open as td (td.id)}
        {@const due = dueState(td.due_date)}
        <li class="w-row">
          <button
            class="w-check"
            disabled={busy.has(td.id)}
            onclick={() => toggle(td)}
            aria-label={$t('dashboard.widgets.todos.markDone')}
          ></button>
          <span class="w-name grow">{td.name}</span>
          {#if td.due_date}
            <!-- Overdue is named as well as coloured: the pill carries the date either way. -->
            <span class="w-pill" class:neg={due === 'overdue'} class:warn={due === 'today'}>
              {shortDay(td.due_date)}
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
  }
  .tick {
    width: 24px;
    padding-right: 0;
  }
  /* The circular tick of the mockup's task table; the square .w-check stays the list one. */
  .tick .w-check {
    border-radius: 50%;
    background: transparent;
  }
  .bars {
    display: flex;
    align-items: flex-end;
  }
  .bars :global(> *) {
    width: 100%;
  }
</style>
