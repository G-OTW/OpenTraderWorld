<script>
  import Icon from '$lib/ui/Icon.svelte';
  // RemindMe module — reminders list. Each reminder fires in-app notifications on a
  // cadence; notifications live in the top-nav bell.
  import { onMount } from 'svelte';
  import { remindApi, freqLabel, kindLabel, fmtTime, fmtDate, KINDS } from '$lib/modules/remindme/api.js';
  import ReminderForm from '$lib/modules/remindme/ReminderForm.svelte';
  import ChannelsModal from '$lib/modules/remindme/ChannelsModal.svelte';
  import { goalsApi } from '$lib/modules/goals/api.js';
  import { todosApi } from '$lib/modules/todos/api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let reminders = $state([]);
  let goals = $state([]);
  let todos = $state([]);
  let loading = $state(true);

  let showForm = $state(false);
  let editing = $state(null);
  let showChannels = $state(false);

  // 'all' | 'upcoming' | 'passed' | 'paused'
  let filter = $state('all');
  // 'all' | kind id
  let typeFilter = $state('all');
  // 'desc' (newest start first) | 'asc'
  let sortDir = $state('desc');
  // collapsed month keys
  let collapsed = $state(new Set());

  function statusOf(r) {
    if (!r.active) return 'paused';
    if (!r.next_fire_at) return 'passed';
    return 'upcoming';
  }

  const view = $derived.by(() => {
    let list = reminders;
    if (filter !== 'all') list = list.filter((r) => statusOf(r) === filter);
    if (typeFilter !== 'all') list = list.filter((r) => r.kind === typeFilter);
    const dir = sortDir === 'asc' ? 1 : -1;
    return [...list].sort((a, b) => {
      const av = a.start_date ?? '';
      const bv = b.start_date ?? '';
      if (av === bv) return 0;
      return av < bv ? -dir : dir;
    });
  });

  // Group sorted view into [{ key, label, items }] by start month.
  const monthFmt = new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' });
  const groups = $derived.by(() => {
    const out = [];
    let cur = null;
    for (const r of view) {
      const key = r.start_date ? r.start_date.slice(0, 7) : 'none';
      if (!cur || cur.key !== key) {
        const label = r.start_date ? monthFmt.format(new Date(r.start_date + 'T00:00')) : 'No date';
        cur = { key, label, items: [] };
        out.push(cur);
      }
      cur.items.push(r);
    }
    return out;
  });

  function toggleMonth(key) {
    const next = new Set(collapsed);
    next.has(key) ? next.delete(key) : next.add(key);
    collapsed = next;
  }

  onMount(async () => {
    [reminders, goals, todos] = await Promise.all([
      remindApi.list(),
      goalsApi.list().catch(() => []),
      todosApi.list().catch(() => [])
    ]);
    loading = false;
  });

  async function reload() {
    reminders = await remindApi.list();
  }

  function openAdd() {
    editing = null;
    showForm = true;
  }
  function openEdit(r) {
    editing = r;
    showForm = true;
  }

  async function save(payload) {
    if (editing) await remindApi.update(editing.id, payload);
    else await remindApi.add(payload);
    showForm = false;
    await reload();
  }

  // Deleting a reminder is not undoable — ConfirmModal, not the browser's confirm().
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function del(r) {
    pendingDelete = r;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const r = pendingDelete;
    pendingDelete = null;
    if (!r) return;
    await remindApi.remove(r.id);
    await reload();
  }

  // Resolve a linked item's name for display.
  function linkedName(r) {
    if (r.kind === 'goal') return goals.find((g) => g.id === r.linked_id)?.name;
    if (r.kind === 'todo') return todos.find((t) => t.id === r.linked_id)?.name;
    return null;
  }

  function scheduleText(r) {
    if (!r.active) return $t('remindme.status.paused');
    if (!r.next_fire_at) return $t('remindme.status.finished');
    return $t('remindme.status.next', { time: fmtTime(r.next_fire_at) });
  }
</script>

<div class="rem">
  <header class="head">
    <div class="title">
      <h1>{$t('remindme.title')}</h1>
      <span class="sub">{$t('remindme.count', { count: reminders.length, s: reminders.length === 1 ? '' : 's' })}</span>
    </div>
    <div class="head-actions">
      <button class="ghost" onclick={() => (showChannels = true)}><Icon name="send" size={14} /> {$t('remindme.channels.button')}</button>
      <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('remindme.addReminder')}</button>
    </div>
  </header>

  {#if loading}
    <!-- Reminders render as month sections of rows; the skeleton is a plain row stack. -->
    <div class="sk-list" aria-busy="true">
      {#each Array.from({ length: 5 }, (_, i) => i) as i (i)}
        <div class="sk-row">
          <Skeleton height="1rem" width="50%" />
          <Skeleton height="0.8rem" width="30%" />
        </div>
      {/each}
    </div>
  {:else if reminders.length === 0}
    <EmptyState icon="bell" title={$t('remindme.emptyTitle')} description={$t('remindme.emptyBody')}>
      {#snippet action()}
        <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('remindme.addReminder')}</button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="controls">
      <div class="seg">
        {#each [['all', $t('remindme.filter.all')], ['upcoming', $t('remindme.filter.upcoming')], ['passed', $t('remindme.filter.passed')], ['paused', $t('remindme.filter.paused')]] as [v, label]}
          <button class="seg-btn" class:on={filter === v} onclick={() => (filter = v)}>{label}</button>
        {/each}
      </div>
      <select class="type-sel" bind:value={typeFilter} title={$t('remindme.filterByType')}>
        <option value="all">{$t('remindme.allTypes')}</option>
        {#each KINDS as k}
          <option value={k.id}>{k.label}</option>
        {/each}
      </select>
      <button
        class="sort"
        onclick={() => (sortDir = sortDir === 'asc' ? 'desc' : 'asc')}
        title={$t('remindme.sortByStartDate')}
      >
        {$t('remindme.startDate')} <Icon name={sortDir === 'asc' ? 'arrow-up' : 'arrow-down'} size={12} />
      </button>
      <div class="controls-right">
        <button class="sort" onclick={() => (collapsed = new Set())} title={$t('remindme.expandAllMonths')}>{$t('remindme.expandAll')}</button>
        <button class="sort" onclick={() => (collapsed = new Set(groups.map((g) => g.key)))} title={$t('remindme.collapseAllMonths')}>{$t('remindme.collapseAll')}</button>
      </div>
    </div>
    {#if view.length === 0}
      <EmptyState icon="filter" compact title={$t('remindme.noneForFilter')}>
        {#snippet action()}
          <button class="ghost" onclick={() => { filter = 'all'; typeFilter = 'all'; }}>{$t('common.clear')}</button>
        {/snippet}
      </EmptyState>
    {:else}
      {#each groups as g (g.key)}
        <section class="month">
          <button class="month-head" onclick={() => toggleMonth(g.key)}>
            <span class="chev"><Icon name={collapsed.has(g.key) ? 'chevron-right' : 'chevron-down'} size={13} /></span>
            <span class="month-label">{g.label}</span>
            <span class="month-count">{g.items.length}</span>
          </button>
          {#if !collapsed.has(g.key)}
            <div class="table-wrap">
              <table class="tbl">
                <thead>
                  <tr>
                    <th>{$t('remindme.table.name')}</th>
                    <th>{$t('remindme.table.type')}</th>
                    <th>{$t('remindme.table.frequency')}</th>
                    <th>{$t('remindme.table.start')}</th>
                    <th>{$t('remindme.table.limit')}</th>
                    <th>{$t('remindme.table.status')}</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {#each g.items as r (r.id)}
                    <tr class:inactive={!r.active}>
                      <td class="strong">
                        {r.name}
                        {#if linkedName(r)}<span class="linked">→ {linkedName(r)}</span>{/if}
                      </td>
                      <td>{kindLabel(r.kind)}</td>
                      <td>{freqLabel(r.frequency)}</td>
                      <td>{fmtDate(r.start_date) ?? '—'}{#if r.start_time && r.start_time !== '00:00'} · {r.start_time}{/if}</td>
                      <td>{r.max_count ? `${r.fired_count}/${r.max_count}` : (r.frequency === 'once' ? $t('remindme.once') : '∞')}</td>
                      <td class="status">{scheduleText(r)}</td>
                      <td class="row-actions">
                        <button class="icon" title={$t('remindme.edit')} onclick={() => openEdit(r)}><Icon name="pencil" size={14} /></button>
                        <button class="icon danger-hover" title={$t('remindme.delete')} onclick={() => del(r)}><Icon name="trash" size={14} /></button>
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </section>
      {/each}
    {/if}
  {/if}
</div>

<Modal bind:open={showForm} title={editing ? $t('remindme.form.editTitle') : $t('remindme.form.newTitle')} size="md">
  <ReminderForm {goals} {todos} initial={editing} onsubmit={save} oncancel={() => (showForm = false)} />
</Modal>

<Modal bind:open={showChannels} title={$t('remindme.channels.title')} size="md">
  {#if showChannels}<ChannelsModal />{/if}
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('remindme.delete')}
  message={pendingDelete ? $t('remindme.confirmDelete', { name: pendingDelete.name }) : ''}
  confirmLabel={$t('remindme.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .sk-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }
  .sk-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .rem {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
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
    font-weight: var(--fw-medium);
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
  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
    flex-wrap: wrap;
  }
  .controls-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
  }
  .seg {
    display: inline-flex;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg-btn {
    background: transparent;
    border: none;
    border-right: 0.5px solid var(--border);
    color: var(--muted);
    padding: 6px 12px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .seg-btn:last-child {
    border-right: none;
  }
  .seg-btn:hover {
    color: var(--text);
  }
  .seg-btn.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .sort {
    background: transparent;
    border: 0.5px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 6px 12px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .sort:hover {
    border-color: var(--border-control);
    background: var(--surface-2);
  }
  .type-sel {
    background: var(--surface);
    border: 0.5px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 6px 10px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .month {
    margin-bottom: var(--space-3);
  }
  .month-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 12px;
    cursor: pointer;
    color: var(--text);
    font-size: var(--text-base);
    margin-bottom: var(--space-2);
  }
  .month-head:hover {
    border-color: var(--border-control);
    background: var(--surface-2);
  }
  .chev {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .month-label {
    font-weight: var(--fw-medium);
  }
  .month-count {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-xs);
    background: var(--surface-2);
    border-radius: 0;
    padding: 0 8px;
  }
  .muted {
    color: var(--dim);
  }
  .table-wrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tr.inactive {
    opacity: 0.55;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .linked {
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-sm);
    margin-left: 6px;
  }
  .status {
    color: var(--muted);
  }
  .row-actions {
    white-space: nowrap;
  }
</style>
