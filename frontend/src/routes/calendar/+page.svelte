<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Calendar module — a custom, DB-backed personal calendar (FullCalendar) with
  // year/month/week/day zoom. The economic calendar lives in its own module now.
  import { goto } from '$app/navigation';
  import {
    calendarApi,
    toFcEvent,
    reminderToFcEvent,
    todoToFcEvent,
    goalToFcEvent
  } from '$lib/modules/calendar/api.js';
  import { remindApi } from '$lib/modules/remindme/api.js';
  import { todosApi } from '$lib/modules/todos/api.js';
  import { goalsApi } from '$lib/modules/goals/api.js';
  import PersonalCalendar from '$lib/modules/calendar/PersonalCalendar.svelte';
  import EventForm from '$lib/modules/calendar/EventForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { t } from '$lib/i18n';

  let baseEvents = $state([]); // personal calendar events
  let overlayEvents = $state([]); // reminders/todos/goals (read-only)
  const events = $derived([...baseEvents, ...overlayEvents]);
  let range = $state(null); // { from, to }
  let showForm = $state(false);
  let editing = $state(null); // raw event row or null
  let formInitial = $state(null); // prefill for the form

  // Overlay toggles — persisted in localStorage so the choice sticks across visits.
  const OVERLAY_KEY = 'otw.calendar.overlays';
  let overlays = $state(loadOverlays());

  function loadOverlays() {
    const def = { reminder: false, todo: false, goal: false };
    if (typeof localStorage === 'undefined') return def;
    try {
      return { ...def, ...JSON.parse(localStorage.getItem(OVERLAY_KEY) || '{}') };
    } catch {
      return def;
    }
  }

  function toggleOverlay(key) {
    overlays = { ...overlays, [key]: !overlays[key] };
    if (typeof localStorage !== 'undefined')
      localStorage.setItem(OVERLAY_KEY, JSON.stringify(overlays));
    loadOverlayEvents();
  }

  // Distinct category labels across personal events (for the form datalist).
  const categories = $derived(
    [...new Set(baseEvents.map((e) => (e.extendedProps?.category || '').trim()).filter(Boolean))].sort()
  );

  async function loadRange(from, to) {
    range = { from, to };
    const rows = await calendarApi.list(from, to);
    baseEvents = rows.map(toFcEvent);
  }

  // Reload only the enabled overlay sources and rebuild the merged overlay list.
  // Sources are fetched in full (small datasets); FullCalendar windows them.
  async function loadOverlayEvents() {
    const tasks = [];
    if (overlays.reminder)
      tasks.push(remindApi.list().then((rs) => rs.map(reminderToFcEvent)));
    if (overlays.todo) tasks.push(todosApi.list().then((ts) => ts.map(todoToFcEvent)));
    if (overlays.goal) tasks.push(goalsApi.list().then((gs) => gs.map(goalToFcEvent)));
    const results = await Promise.all(tasks);
    overlayEvents = results.flat().filter(Boolean);
  }

  $effect(() => {
    loadOverlayEvents();
  });

  function openSlot({ startStr, endStr, allDay }) {
    editing = null;
    formInitial = {
      title: '',
      start_at: startStr,
      end_at: endStr,
      all_day: allDay,
      category: '',
      color: '',
      location: '',
      notes: ''
    };
    showForm = true;
  }

  let existingReminder = $state(null); // reminder linked to the event being edited

  async function openEvent(fcEvent) {
    // Overlay events (reminders/todos/goals) are read-only here — jump to their module.
    const src = fcEvent.extendedProps?.source;
    if (src === 'reminder') return goto('/remindme');
    if (src === 'todo') return goto('/todos');
    if (src === 'goal') return goto('/goals');
    // Personal event -> fetch the full row for accurate notes/location.
    const row = await calendarApi.get(fcEvent.id);
    editing = row;
    formInitial = row;
    existingReminder = await findReminderForEvent(row.id);
    showForm = true;
  }

  function openAdd() {
    editing = null;
    formInitial = null;
    existingReminder = null;
    showForm = true;
  }

  // A reminder created from an event is a `custom` reminder whose linked_id is the
  // event id — that's how we detect "a reminder already exists" and avoid dupes.
  async function findReminderForEvent(eventId) {
    if (!eventId) return null;
    const all = await remindApi.list();
    return all.find((r) => r.kind === 'custom' && r.linked_id === eventId) ?? null;
  }

  async function save(payload, alsoRemind = false) {
    let eventId;
    if (editing) {
      await calendarApi.update(editing.id, payload);
      eventId = editing.id;
    } else {
      const created = await calendarApi.add(payload);
      eventId = created?.id;
    }
    // Create the linked reminder only if requested AND one doesn't already exist;
    // otherwise keep it in sync so its name/time follow the event.
    if (payload.start_at && eventId) {
      if (alsoRemind && !existingReminder) await createReminderForEvent(eventId, payload);
      else if (existingReminder) await syncReminderForEvent(existingReminder, payload);
    }
    showForm = false;
    if (range) await loadRange(range.from, range.to);
    if (overlays.reminder) await loadOverlayEvents();
  }

  // Derive reminder schedule fields from an event payload.
  function reminderFieldsFrom(ev) {
    const d = new Date(ev.start_at);
    const pad = (n) => String(n).padStart(2, '0');
    const start_date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    // All-day events have no meaningful clock time → default to 09:00 local.
    const start_time = ev.all_day ? '09:00' : `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    return {
      name: ev.title || $t('calendar.page.reminderDefaultName'),
      kind: 'custom',
      frequency: 'once',
      start_date,
      start_time,
      tz_offset_minutes: -new Date().getTimezoneOffset(),
      details: ev.notes || ''
    };
  }

  async function createReminderForEvent(eventId, ev) {
    await remindApi.add({ ...reminderFieldsFrom(ev), linked_id: eventId });
  }

  // Keep an existing linked reminder's name/schedule aligned with the event.
  async function syncReminderForEvent(reminder, ev) {
    await remindApi.update(reminder.id, {
      ...reminder,
      ...reminderFieldsFrom(ev),
      linked_id: reminder.linked_id
    });
  }

  // Deleting an event is not undoable — ConfirmModal, not the browser's confirm().
  // Snapshot `editing` now: the form stays open behind the dialog and could change it.
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function del() {
    if (!editing) return;
    pendingDelete = editing;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const ev = pendingDelete;
    pendingDelete = null;
    if (!ev) return;
    await calendarApi.remove(ev.id);
    showForm = false;
    if (range) await loadRange(range.from, range.to);
  }

  // Drag/resize: persist the new times, keeping the rest of the row intact.
  async function moved(fcEvent) {
    const row = await calendarApi.get(fcEvent.id);
    await calendarApi.update(fcEvent.id, {
      ...row,
      start_at: fcEvent.start ? fcEvent.start.toISOString() : row.start_at,
      end_at: fcEvent.end ? fcEvent.end.toISOString() : null,
      all_day: fcEvent.allDay
    });
    if (range) await loadRange(range.from, range.to);
  }
</script>

<div class="cal-page">
  <header class="head">
    <h1>{$t('calendar.page.title')}</h1>
    <div class="overlay-toggles" role="group" aria-label={$t('calendar.page.showOnCalendar')}>
      <button
        class="ov reminder"
        class:on={overlays.reminder}
        aria-pressed={overlays.reminder}
        onclick={() => toggleOverlay('reminder')}
        title={$t('calendar.page.showRemindersTitle')}
      >
        <span class="dot"></span>{$t('calendar.page.reminders')}
      </button>
      <button
        class="ov todo"
        class:on={overlays.todo}
        aria-pressed={overlays.todo}
        onclick={() => toggleOverlay('todo')}
        title={$t('calendar.page.showTodosTitle')}
      >
        <span class="dot"></span>{$t('calendar.page.todos')}
      </button>
      <button
        class="ov goal"
        class:on={overlays.goal}
        aria-pressed={overlays.goal}
        onclick={() => toggleOverlay('goal')}
        title={$t('calendar.page.showGoalsTitle')}
      >
        <span class="dot"></span>{$t('calendar.page.goals')}
      </button>
    </div>
    <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('calendar.page.addEvent')}</button>
  </header>

  <div class="content">
    <PersonalCalendar
      {events}
      onrange={loadRange}
      oneventclick={openEvent}
      onselect={openSlot}
      onchange={moved}
    />
  </div>
</div>

<Modal bind:open={showForm} title={editing ? $t('calendar.page.editEvent') : $t('calendar.page.newEvent')} size="md">
  <EventForm
    initial={formInitial}
    isNew={!editing}
    hasReminder={!!existingReminder}
    {categories}
    onsubmit={save}
    oncancel={() => (showForm = false)}
    ondelete={editing ? del : null}
  />
</Modal>

<!-- Renders after the form's Modal, so at equal --z-modal it stacks on top. -->
<ConfirmModal
  bind:open={confirmOpen}
  title={$t('calendar.form.delete')}
  message={pendingDelete ? $t('calendar.page.confirmDelete', { title: pendingDelete.title }) : ''}
  confirmLabel={$t('calendar.form.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .cal-page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
  }
  /* Overlay source toggles — pill chips with a status dot. */
  .overlay-toggles {
    margin-left: var(--space-4);
    display: inline-flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .ov {
    --c: var(--muted);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    color: var(--muted);
    border-radius: 0;
    padding: 6px 12px;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    cursor: pointer;
    transition: color 0.12s, border-color 0.12s, background 0.12s;
  }
  .ov.reminder {
    --c: var(--amber);
  }
  .ov.todo {
    --c: var(--green);
  }
  /* --c is only ever a dot fill / border / tint, never text — the 3:1 fill bar applies.
     The violet had no token; --chart-5 is the ramp's violet, validated in both themes. */
  .ov.goal {
    --c: var(--chart-5);
  }
  .ov .dot {
    width: 8px;
    height: 8px;
    border-radius: 0;
    background: var(--c);
    opacity: 0.35;
    transition: opacity 0.12s;
  }
  .ov:hover {
    color: var(--text);
    border-color: color-mix(in srgb, var(--c) 50%, var(--border));
  }
  .ov.on {
    color: var(--text);
    border-color: color-mix(in srgb, var(--c) 60%, var(--border));
    background: color-mix(in srgb, var(--c) 14%, var(--surface-2));
  }
  .ov.on .dot {
    opacity: 1;
  }

  .content {
    flex: 1;
    min-height: 0;
  }
</style>
