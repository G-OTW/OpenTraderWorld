<script>
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  // RemindMe module — reminders list. Each reminder fires in-app notifications on a
  // cadence; notifications live in the top-nav bell.
  import { onMount } from 'svelte';
  import { remindApi, freqLabel, intervalLabel, dayRunLabel, kindLabel, weekdaysLabel, fmtTime, fmtDate, KINDS } from '$lib/modules/remindme/api.js';
  import ReminderForm from '$lib/modules/remindme/ReminderForm.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
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
      <ChannelButton module="remindme" />
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
      <div class="type-sel">
        <Dropdown
          bind:value={typeFilter}
          title={$t('remindme.filterByType')}
          ariaLabel={$t('remindme.filterByType')}
          options={[
            { value: 'all', label: $t('remindme.allTypes') },
            ...KINDS.map((k) => ({ value: k.id, label: k.label }))
          ]}
        />
      </div>
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
      <!-- One table for every month: columns line up across groups, months are header rows. -->
      <div class="table-wrap">
        <table class="tbl rem-tbl">
          <colgroup>
            <col />
            <col class="c-type" />
            <col class="c-freq" />
            <col class="c-start" />
            <col class="c-limit" />
            <col class="c-status" />
            <col class="c-act" />
          </colgroup>
          <thead>
            <tr>
              <th>{$t('remindme.table.name')}</th>
              <th>{$t('remindme.table.type')}</th>
              <th>{$t('remindme.table.frequency')}</th>
              <th>{$t('remindme.table.start')}</th>
              <th class="ctr">{$t('remindme.table.limit')}</th>
              <th>{$t('remindme.table.status')}</th>
              <th></th>
            </tr>
          </thead>
          {#each groups as g (g.key)}
            <tbody>
              <tr class="grp">
                <td colspan="7">
                  <button class="month-head" onclick={() => toggleMonth(g.key)} aria-expanded={!collapsed.has(g.key)}>
                    <span class="chev"><Icon name={collapsed.has(g.key) ? 'chevron-right' : 'chevron-down'} size={13} /></span>
                    <span class="month-label">{g.label}</span>
                    <span class="month-count">{g.items.length}</span>
                  </button>
                </td>
              </tr>
              {#if !collapsed.has(g.key)}
                {#each g.items as r (r.id)}
                  <tr class="item" class:inactive={!r.active}>
                    <td class="strong">
                      <div class="name-cell">
                        <span class="name" title={r.name}>{r.name}</span>
                        {#if linkedName(r)}<span class="linked">→ {linkedName(r)}</span>{/if}
                        {#if r.url}
                          <a
                            class="urllink"
                            href={r.url}
                            target="_blank"
                            rel="noopener noreferrer nofollow"
                            title={r.link_label || r.url}
                          >
                            <Icon name="external-link" size={12} />
                            {r.link_label || $t('remindme.openLink')}
                          </a>
                        {/if}
                      </div>
                    </td>
                    <td data-label={$t('remindme.table.type')}><span class="kind">{kindLabel(r.kind)}</span></td>
                    <td data-label={$t('remindme.table.frequency')}>
                      <div class="freq">
                        <span>{r.frequency === 'interval' ? intervalLabel(r, $t) : freqLabel(r.frequency)}</span>
                        {#if (r.frequency === 'weekly' && r.weekdays) || r.repeat_count}
                          <span class="days">
                            {#if r.frequency === 'weekly' && r.weekdays}{weekdaysLabel(r.weekdays, $t)}{/if}{#if r.frequency === 'weekly' && r.weekdays && r.repeat_count} · {/if}{#if r.repeat_count}{dayRunLabel(r, $t)}{/if}
                          </span>
                        {/if}
                      </div>
                    </td>
                    <td class="tab" data-label={$t('remindme.table.start')}>{fmtDate(r.start_date) ?? '—'}{#if r.start_time && r.start_time !== '00:00'}<span class="time"> · {r.start_time}</span>{/if}</td>
                    <td class="tab ctr" data-label={$t('remindme.table.limit')}>{r.max_count ? `${r.fired_count}/${r.max_count}` : (r.frequency === 'once' ? $t('remindme.once') : '∞')}</td>
                    <td class="status st-{statusOf(r)}" data-label={$t('remindme.table.status')}><span class="dot"></span>{scheduleText(r)}</td>
                    <td class="row-actions">
                      <button class="icon" title={$t('remindme.edit')} onclick={() => openEdit(r)}><Icon name="pencil" size={14} /></button>
                      <button class="icon danger-hover" title={$t('remindme.delete')} onclick={() => del(r)}><Icon name="trash" size={14} /></button>
                    </td>
                  </tr>
                {/each}
              {/if}
            </tbody>
          {/each}
        </table>
      </div>
    {/if}
  {/if}
</div>

<Modal bind:open={showForm} title={editing ? $t('remindme.form.editTitle') : $t('remindme.form.newTitle')} size="md">
  <ReminderForm {goals} {todos} initial={editing} onsubmit={save} oncancel={() => (showForm = false)} />
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
  /* Dropdown brings its own control look; the wrapper only sets the filter's width. */
  .type-sel {
    min-width: 140px;
  }
  .table-wrap {
    overflow-x: auto;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  /* Fixed layout: every month shares the same column grid, so dates, limits and
     statuses sit on one vertical line down the whole list. */
  .rem-tbl {
    table-layout: fixed;
    min-width: 920px;
  }
  .rem-tbl .c-type { width: 110px; }
  .rem-tbl .c-freq { width: 26%; }
  .rem-tbl .c-start { width: 170px; }
  .rem-tbl .c-limit { width: 72px; }
  .rem-tbl .c-status { width: 180px; }
  .rem-tbl .c-act { width: 84px; }
  .rem-tbl td {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ctr {
    text-align: center;
  }
  .tab {
    font-variant-numeric: tabular-nums;
  }
  .time {
    color: var(--muted);
  }
  /* Month row: full-width band that separates groups and toggles them. */
  .rem-tbl tr.grp td {
    padding: 0;
    height: auto;
    background: var(--surface-2);
  }
  .rem-tbl tr.grp:hover {
    background: transparent;
  }
  .month-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: transparent;
    border: 0;
    border-radius: 0;
    padding: 6px var(--space-3);
    cursor: pointer;
    color: var(--text);
    font-size: var(--text-sm);
    text-transform: capitalize;
  }
  .month-head:hover {
    background: color-mix(in srgb, var(--text) 4%, transparent);
  }
  .chev {
    display: inline-flex;
    color: var(--muted);
  }
  .month-label {
    font-weight: var(--fw-medium);
  }
  .month-count {
    margin-left: auto;
    min-width: 22px;
    text-align: center;
    color: var(--muted);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    border: var(--hairline) solid var(--border);
    border-radius: 999px;
    padding: 0 6px;
  }
  .rem-tbl tbody:last-child tr:last-child td {
    border-bottom: none;
  }
  tr.inactive td:not(.row-actions) {
    opacity: 0.55;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .name-cell {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .linked {
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }
  .urllink {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
    font-size: var(--text-xs);
    font-weight: var(--fw-normal);
    color: var(--accent);
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind {
    display: inline-block;
    font-size: var(--text-xs);
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    padding: 1px 6px;
  }
  .freq {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .days {
    color: var(--muted);
    font-size: var(--text-xs);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    color: var(--muted);
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-right: var(--space-2);
    vertical-align: middle;
    background: var(--muted);
  }
  .st-upcoming { color: var(--text); }
  .st-upcoming .dot { background: var(--green); }
  .st-paused .dot { background: var(--amber); }
  .row-actions {
    text-align: right;
  }
  @media (max-width: 767px) {
    .rem { padding: var(--space-4) var(--space-4) 80px; }
    .head { align-items: flex-start; flex-wrap: wrap; gap: var(--space-2); }
    .title { flex-wrap: wrap; }
    .head-actions { width: 100%; flex-wrap: wrap; }
    .head-actions > :global(*) { max-width: 100%; }
    .head-actions button { min-height: 44px; white-space: normal; }
    .seg { flex-wrap: wrap; }
    .seg-btn, .sort, .month-head { min-height: 44px; }
    .controls-right { margin-left: 0; }
    /* One labelled record per reminder: name and link on top, schedule fields in two
       columns, edit and delete under them. No sideways panning to reach the actions. */
    .table-wrap { overflow-x: visible; border: 0; background: transparent; }
    .rem-tbl { min-width: 0; }
    .table-wrap table, .table-wrap tbody, .table-wrap td { display: block; width: 100%; }
    .table-wrap thead, .table-wrap colgroup { display: none; }
    .table-wrap tbody { display: flex; flex-direction: column; gap: var(--space-2); margin-bottom: var(--space-3); }
    .table-wrap tr.grp { display: block; }
    .table-wrap tr.grp td { border: var(--hairline) solid var(--border); border-radius: var(--radius); }
    .table-wrap tr.item { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-1) var(--space-3); border: var(--hairline) solid var(--border); border-radius: var(--radius); background: var(--surface); padding: var(--space-2) var(--space-3); }
    .table-wrap td { height: auto; border: 0; padding: 0; white-space: normal; min-width: 0; overflow-wrap: anywhere; text-align: left; }
    .table-wrap td[data-label]::before { content: attr(data-label); display: block; font-size: var(--text-xs); font-weight: var(--fw-normal); color: var(--muted); }
    .table-wrap td.strong, .table-wrap td.status, .table-wrap td.row-actions { grid-column: 1 / -1; }
    .name-cell { flex-wrap: wrap; }
    .freq { flex-wrap: wrap; }
    .urllink { max-width: 100%; min-height: 24px; }
    .table-wrap td.row-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
    .row-actions .icon { min-width: 44px; min-height: 44px; }
  }
</style>
