<script>
  // ToDo module. A task list with a check-to-complete design: open tasks first, then a
  // collapsed "done" section. Add/edit via a modal form.
  import { onMount } from 'svelte';
  import { todosApi, fmtDate, dueState } from '$lib/modules/todos/api.js';
  import TodoForm from '$lib/modules/todos/TodoForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let todos = $state([]);
  let loading = $state(true);

  let showForm = $state(false);
  let editing = $state(null);
  let showDone = $state(false);

  let statusFilter = $state('all'); // all | pending | done | overdue
  const statusFilters = ['all', 'pending', 'done', 'overdue'];
  let catFilter = $state('all'); // 'all' or a specific category
  let sortDir = $state('asc'); // due-date sort: 'asc' (soonest first) | 'desc'

  onMount(async () => {
    await reload();
    loading = false;
  });

  async function reload() {
    todos = await todosApi.list();
  }

  // Distinct, sorted category labels present across tasks (non-empty).
  const categories = $derived(
    [...new Set(todos.map((t) => (t.category || '').trim()).filter(Boolean))].sort()
  );

  const matchesCat = (t) => catFilter === 'all' || (t.category || '').trim() === catFilter;

  // Sort by due date; tasks with no due date sort last regardless of direction.
  function byDue(a, b) {
    const av = a.due_date ?? '';
    const bv = b.due_date ?? '';
    if (!av && !bv) return 0;
    if (!av) return 1;
    if (!bv) return -1;
    return sortDir === 'asc' ? av.localeCompare(bv) : bv.localeCompare(av);
  }

  const isOverdue = (t) => dueState(t.due_date) === 'overdue';
  const overdue = $derived(statusFilter === 'overdue');
  const matchesOverdue = (t) => !overdue || isOverdue(t);

  const open = $derived(
    todos.filter((t) => !t.done && matchesCat(t) && matchesOverdue(t)).sort(byDue)
  );
  const done = $derived(
    todos.filter((t) => t.done && matchesCat(t) && matchesOverdue(t)).sort(byDue)
  );

  // Overdue spans both pending and done; only Pending/Done hide the other section.
  const showOpen = $derived(statusFilter !== 'done');
  const showDoneSection = $derived(statusFilter !== 'pending');
  // Auto-expand the done list when the filter is Done or Overdue.
  const doneExpanded = $derived(statusFilter === 'done' || overdue || showDone);

  function openAdd() {
    editing = null;
    showForm = true;
  }
  function openEdit(t) {
    editing = t;
    showForm = true;
  }

  // Returns the saved task (with id) so the form can pre-link a reminder after create.
  async function save(payload, { keepOpen = false } = {}) {
    let saved;
    if (editing) {
      await todosApi.update(editing.id, payload);
      saved = { ...editing, ...payload };
    } else {
      saved = await todosApi.add(payload);
    }
    if (!keepOpen) showForm = false;
    else editing = saved; // promote create → edit so further saves update in place
    await reload();
    return saved;
  }

  async function toggle(t) {
    await todosApi.setDone(t.id, !t.done);
    await reload();
  }

  // Deleting a task is not undoable — ConfirmModal, not the browser's confirm().
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function del(task) {
    pendingDelete = task;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const task = pendingDelete;
    pendingDelete = null;
    if (!task) return;
    await todosApi.remove(task.id);
    await reload();
  }

  function dueLabelFor(ds) {
    if (ds === 'overdue') return $t('todos.due.overdue');
    if (ds === 'today') return $t('todos.due.today');
    if (ds === 'soon') return $t('todos.due.soon');
    return '';
  }

  function statusLabel(f) {
    if (f === 'all') return $t('todos.filter.all');
    if (f === 'pending') return $t('todos.filter.pending');
    if (f === 'done') return $t('todos.filter.done');
    return $t('todos.filter.overdue');
  }
</script>

<div class="todos">
  <header class="head">
    <div class="title">
      <h1>{$t('todos.title')}</h1>
      <span class="sub">{$t('todos.summary', { open: open.length, done: done.length })}</span>
    </div>
    <div class="head-actions">
      <QuickReminderButton title={$t('todos.addReminder')} />
      <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('todos.addTask')}</button>
    </div>
  </header>

  {#if loading}
    <!-- A list of checkbox rows, so the skeleton is a stack of rows at the list's spacing. -->
    <ul class="list" aria-busy="true">
      {#each Array.from({ length: 5 }, (_, i) => i) as i (i)}
        <li class="sk-item">
          <Skeleton circle size="18px" />
          <Skeleton height="1rem" width="55%" />
        </li>
      {/each}
    </ul>
  {:else if todos.length === 0}
    <EmptyState icon="check-square" title={$t('todos.emptyTitle')} description={$t('todos.emptyBody')}>
      {#snippet action()}
        <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('todos.addTask')}</button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="toolbar">
      <div class="chips">
        {#each statusFilters as f}
          <button
            class="chip"
            class:overdue={f === 'overdue'}
            class:active={statusFilter === f}
            onclick={() => (statusFilter = f)}
          >
            {statusLabel(f)}
          </button>
        {/each}
      </div>
      <button
        class="chip sort"
        onclick={() => (sortDir = sortDir === 'asc' ? 'desc' : 'asc')}
        title={$t('todos.sortByDueDate')}
      >
        {$t('todos.date')} <Icon name={sortDir === 'asc' ? 'arrow-up' : 'arrow-down'} size={12} />
      </button>
    </div>

    {#if categories.length > 0}
      <div class="chips cat">
        <button class="chip" class:active={catFilter === 'all'} onclick={() => (catFilter = 'all')}>
          {$t('todos.allCategories')}
        </button>
        {#each categories as c}
          <button class="chip" class:active={catFilter === c} onclick={() => (catFilter = c)}>
            {c}
          </button>
        {/each}
      </div>
    {/if}

    {#if showOpen}
    <ul class="list">
      {#each open as td (td.id)}
        {@const ds = dueState(td.due_date)}
        <li class="item" data-due={ds}>
          <button class="check" onclick={() => toggle(td)} aria-label={$t('todos.markDone')}></button>
          <div class="body">
            <div class="row1">
              <span class="name">{td.name}</span>
              {#if td.due_date}
                <span class="due" data-due={ds}>
                  {#if dueLabelFor(ds)}<b>{dueLabelFor(ds)}</b> · {/if}{fmtDate(td.due_date)}{#if td.due_time} · {td.due_time}{/if}
                </span>
              {/if}
            </div>
            {#if td.category}<span class="badge cat-tag">{td.category}</span>{/if}
            {#if td.details}<p class="details">{td.details}</p>{/if}
          </div>
          <div class="actions">
            <button class="icon" title={$t('todos.edit')} onclick={() => openEdit(td)}><Icon name="pencil" size={14} /></button>
            <button class="icon danger-hover" title={$t('todos.delete')} onclick={() => del(td)}><Icon name="trash" size={14} /></button>
          </div>
        </li>
      {/each}
    </ul>
    {/if}

    {#if showDoneSection && done.length > 0}
      {#if statusFilter !== 'done'}
        <button class="done-toggle" onclick={() => (showDone = !showDone)}>
          <Icon name={showDone ? 'chevron-down' : 'chevron-right'} size={13} /> {$t('todos.doneCount', { count: done.length })}
        </button>
      {/if}
      {#if doneExpanded}
        <ul class="list done-list">
          {#each done as td (td.id)}
            <li class="item done">
              <button class="check checked" onclick={() => toggle(td)} aria-label={$t('todos.markNotDone')}><Icon name="check" size={12} /></button>
              <div class="body">
                <div class="row1"><span class="name">{td.name}</span></div>
                {#if td.category}<span class="badge cat-tag">{td.category}</span>{/if}
              </div>
              <div class="actions">
                <button class="icon danger-hover" title={$t('todos.delete')} onclick={() => del(td)}><Icon name="trash" size={14} /></button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
</div>

<Modal bind:open={showForm} title={editing ? $t('todos.form.editTitle') : $t('todos.form.newTitle')} size="sm">
  <TodoForm initial={editing} {categories} onsubmit={save} oncancel={() => (showForm = false)} />
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('todos.delete')}
  message={pendingDelete ? $t('todos.confirmDelete', { name: pendingDelete.name }) : ''}
  confirmLabel={$t('todos.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .todos {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
    max-width: 760px;
    margin: 0 auto;
    width: 100%;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .sub {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .muted {
    color: var(--muted);
  }
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  /* Category chips: one row, scrolls sideways with no visible scrollbar. */
  .chips.cat {
    margin-bottom: var(--space-3);
    flex-wrap: nowrap;
    overflow-x: auto;
    scrollbar-width: none; /* Firefox */
    -ms-overflow-style: none; /* IE/Edge */
  }
  .chips.cat::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  .chips.cat .chip {
    flex: 0 0 auto;
  }
  .chip.overdue.active {
    background: var(--red);
    border-color: var(--red);
  }
  .cat-tag {
    margin-top: 4px;
  }
  .sk-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    border-left: 3px solid var(--border);
  }
  .item[data-due='overdue'] {
    border-left-color: var(--red);
  }
  .item[data-due='today'] {
    border-left-color: var(--amber);
  }
  .item[data-due='soon'] {
    border-left-color: var(--accent);
  }
  .check {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    margin-top: 2px;
    border-radius: 6px;
    border: 2px solid var(--border);
    background: transparent;
    cursor: pointer;
  }
  /* The tick is only ever drawn on the green fill, so its ink belongs here.
     White measured 3.5:1 light / 2.3:1 dark on --green; the dark ink clears both. */
  .check.checked {
    background: var(--green);
    color: var(--green-contrast);
    border-color: var(--green);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .row1 {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .name {
    font-weight: var(--fw-semibold);
    font-size: var(--text-base);
  }
  .done .name {
    text-decoration: line-through;
    color: var(--muted);
  }
  .due {
    flex: none;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .due[data-due='overdue'] {
    color: var(--red);
  }
  .due[data-due='today'] {
    color: var(--amber);
  }
  .details {
    margin-top: 4px;
    font-size: var(--text-sm);
    color: var(--muted);
    white-space: pre-wrap;
  }
  .actions {
    display: flex;
    gap: 2px;
  }
  .done-toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-4);
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) 0;
  }
  .done-list {
    margin-top: var(--space-2);
    opacity: 0.75;
  }
</style>
