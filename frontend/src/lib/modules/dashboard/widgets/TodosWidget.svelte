<script>
  // ToDo widget: today's / upcoming open tasks as a short scrollable list, tickable inline.
  // Config: { limit }. Ticking a task calls the todos API and drops it from the list.
  import { todosApi, dueState } from '$lib/modules/todos/api.js';
  import { t } from '$lib/i18n';

  let { item, editing } = $props();
  const limit = $derived(item.config?.limit ?? 8);

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

  const open = $derived(
    (todos ?? [])
      .filter((t) => !t.done)
      .sort((a, b) => (a.due_date ?? '9999').localeCompare(b.due_date ?? '9999'))
      .slice(0, limit)
  );

  async function toggle(t) {
    if (busy.has(t.id)) return;
    busy = new Set(busy).add(t.id);
    try {
      await todosApi.setDone(t.id, true);
      todos = todos.map((x) => (x.id === t.id ? { ...x, done: true } : x));
    } catch (e) {
      err = e.message;
    } finally {
      const n = new Set(busy);
      n.delete(t.id);
      busy = n;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.todos.preview')}</p>
{:else if err}
  <p class="err">{err}</p>
{:else if todos === null}
  <p class="hint">{$t('common.loading')}</p>
{:else if open.length === 0}
  <p class="hint">{$t('dashboard.widgets.todos.empty')}</p>
{:else}
  <ul class="list">
    {#each open as td (td.id)}
      <li class="row" data-due={dueState(td.due_date)}>
        <button class="chk" disabled={busy.has(td.id)} onclick={() => toggle(td)} aria-label={$t('dashboard.widgets.todos.markDone')}></button>
        <span class="name">{td.name}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .hint,
  .err {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.85rem;
  }
  .row[data-due='overdue'] .name {
    color: var(--red);
  }
  .chk {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1.5px solid var(--border);
    background: var(--surface-2);
    cursor: pointer;
  }
  .chk:hover:not(:disabled) {
    border-color: var(--green);
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
