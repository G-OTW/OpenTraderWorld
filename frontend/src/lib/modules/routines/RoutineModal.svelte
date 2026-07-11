<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Create/edit a routine: name, session bucket, weekday toggles, and the checklist items
  // (each an input row; existing items keep their id so tick history survives edits).
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { traderApi, SESSIONS, WEEKDAYS } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { open = $bindable(false), routineId = null, onsaved = () => {} } = $props();

  let name = $state('');
  let session = $state('pre');
  let weekdays = $state(127); // default: every day (Mon–Sun) — markets run 24/7 (crypto/forex)
  let items = $state([]); // [{ id?, label }]
  let error = $state('');
  let saving = $state(false);

  $effect(() => {
    if (!open) return;
    error = '';
    if (routineId) {
      traderApi
        .routineDetail(routineId)
        .then((r) => {
          name = r.routine.name;
          session = r.routine.session;
          weekdays = r.routine.weekdays;
          items = r.items.map((i) => ({ id: i.id, label: i.label }));
        })
        .catch((e) => (error = e.message));
    } else {
      name = '';
      session = 'pre';
      weekdays = 127;
      items = [{ label: '' }];
    }
  });

  function toggleDay(bit) {
    const next = weekdays ^ bit;
    if (next !== 0) weekdays = next; // at least one weekday stays on
  }

  function addItem() {
    items = [...items, { label: '' }];
  }
  function removeItem(idx) {
    items = items.filter((_, i) => i !== idx);
  }

  async function save() {
    const cleaned = items.map((i) => ({ ...i, label: i.label.trim() })).filter((i) => i.label);
    if (!name.trim()) {
      error = $t('routines.modal.nameRequired');
      return;
    }
    if (cleaned.length === 0) {
      error = $t('routines.modal.addAtLeastOneItem');
      return;
    }
    saving = true;
    error = '';
    try {
      if (routineId) {
        await traderApi.updateRoutine(routineId, { name: name.trim(), session, weekdays, items: cleaned });
      } else {
        await traderApi.createRoutine({
          name: name.trim(),
          session,
          weekdays,
          items: cleaned.map((i) => i.label)
        });
      }
      open = false;
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Deleting a routine takes its history with it — ConfirmModal, not confirm().
  // Snapshot the id: confirming closes this modal, which may clear `routineId`.
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function remove() {
    if (!routineId) return;
    pendingDelete = routineId;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const id = pendingDelete;
    pendingDelete = null;
    if (!id) return;
    try {
      await traderApi.deleteRoutine(id);
      open = false;
      onsaved();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<Modal bind:open size="md" title={routineId ? $t('routines.modal.editRoutine') : $t('routines.modal.newRoutine')}>
  <div class="form">
    <label>
      {$t('routines.modal.name')}
      <input placeholder={$t('routines.modal.namePlaceholder')} bind:value={name} />
    </label>

    <div class="row">
      <label>
        {$t('routines.modal.session')}
        <select bind:value={session}>
          {#each SESSIONS as s (s.key)}<option value={s.key}>{s.icon} {s.label}</option>{/each}
        </select>
      </label>
      <div class="days">
        <span class="lbl">{$t('routines.modal.days')}</span>
        <div class="daybtns">
          {#each WEEKDAYS as d, i (i)}
            <button
              type="button"
              class:on={(weekdays & d.bit) !== 0}
              onclick={() => toggleDay(d.bit)}
              aria-label={$t('routines.modal.toggleWeekday')}
            >
              {d.label}
            </button>
          {/each}
        </div>
      </div>
    </div>

    <div class="items">
      <span class="lbl">{$t('routines.modal.checklist')}</span>
      {#each items as item, i (item.id ?? i)}
        <div class="item">
          <input placeholder={$t('routines.modal.checklistItemPlaceholder')} bind:value={item.label} />
          <button type="button" class="x" onclick={() => removeItem(i)} aria-label={$t('routines.modal.removeItem')}><Icon name="x" size={13} /></button>
        </div>
      {/each}
      <button type="button" class="add" onclick={addItem}>{$t('routines.modal.addItem')}</button>
    </div>

    <ErrorText error={error} />

    <div class="foot">
      {#if routineId}
        <button type="button" class="btn danger" onclick={remove}>{$t('routines.modal.delete')}</button>
      {/if}
      <div class="spacer"></div>
      <button type="button" class="btn" onclick={() => (open = false)}>{$t('common.cancel')}</button>
      <button type="button" class="btn primary" onclick={save} disabled={saving}>
        {saving ? $t('routines.modal.savingEllipsis') : routineId ? $t('common.save') : $t('routines.modal.create')}
      </button>
    </div>
  </div>
</Modal>

<!-- After the host Modal in DOM order, so at equal --z-modal it stacks on top. -->
<ConfirmModal
  bind:open={confirmOpen}
  title={$t('routines.modal.delete')}
  message={$t('routines.modal.confirmDelete')}
  confirmLabel={$t('routines.modal.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  label,
  .days {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .row label {
    flex: 1 1 180px;
  }
  .lbl {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .daybtns {
    display: flex;
    gap: var(--space-1);
  }
  .daybtns button {
    width: 30px;
    height: 30px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    cursor: pointer;
  }
  .daybtns button.on {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface);
  }
  .items {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .item {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .item input {
    flex: 1;
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover {
    color: var(--red);
  }
  .add {
    align-self: flex-start;
    background: transparent;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: var(--text-base);
    padding: 0;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--accent);
  }
  .btn.danger {
    color: var(--red);
    border-color: var(--red);
    background: transparent;
  }
</style>
