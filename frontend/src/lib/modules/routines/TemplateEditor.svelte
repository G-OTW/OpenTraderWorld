<script>
  // Create/edit a routine template. Three stacked sections, in the order a template is
  // actually thought about: what it is (name, category, description, notes), when it runs
  // (the schedule picker), and what it asks you to do (the checklist).
  //
  // A step is a checkbox with optional depth: a rich note explaining *how*, and one link.
  // Both are folded away behind a per-step toggle so a long checklist stays scannable —
  // the enrichment is there when authoring it and out of the way when it isn't.
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SchedulePicker from './SchedulePicker.svelte';
  import RichNote from '$lib/ui/RichNote.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { traderApi, defaultSchedule, scheduleOf } from './api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    routineId = null,
    categories = [],
    // Pre-selects a category when the page opens the editor from inside a category group.
    presetCategoryId = null,
    onsaved = () => {}
  } = $props();

  let name = $state('');
  let description = $state('');
  let notes = $state('');
  let categoryId = $state('');
  let schedule = $state(defaultSchedule());
  let startDate = $state('');
  let endDate = $state('');
  let active = $state(true);
  let items = $state([]); // [{ id?, label, note, url, link_label, _open }]

  let error = $state('');
  let saving = $state(false);
  let loading = $state(false);

  // `_open` is view state (is this step expanded) and never leaves the component.
  function blankItem() {
    return { label: '', note: '', url: '', link_label: '', _open: false };
  }

  $effect(() => {
    if (!open) return;
    error = '';
    if (routineId) {
      loading = true;
      traderApi
        .routineDetail(routineId)
        .then((r) => {
          name = r.routine.name;
          description = r.routine.description ?? '';
          notes = r.routine.notes ?? '';
          categoryId = r.routine.category_id ?? '';
          schedule = scheduleOf(r.routine);
          startDate = r.routine.start_date ?? '';
          endDate = r.routine.end_date ?? '';
          active = r.routine.active;
          items = r.items.map((i) => ({
            id: i.id,
            label: i.label,
            note: i.note ?? '',
            url: i.url ?? '',
            link_label: i.link_label ?? '',
            _open: false
          }));
        })
        .catch((e) => (error = e.message))
        .finally(() => (loading = false));
    } else {
      name = '';
      description = '';
      notes = '';
      categoryId = presetCategoryId ?? '';
      schedule = defaultSchedule();
      startDate = '';
      endDate = '';
      active = true;
      items = [blankItem()];
    }
  });

  function addItem() {
    items = [...items, blankItem()];
  }
  function removeItem(idx) {
    items = items.filter((_, i) => i !== idx);
  }
  function toggleItemDetail(idx) {
    items = items.map((it, i) => (i === idx ? { ...it, _open: !it._open } : it));
  }
  function move(idx, delta) {
    const to = idx + delta;
    if (to < 0 || to >= items.length) return;
    const next = [...items];
    [next[idx], next[to]] = [next[to], next[idx]];
    items = next;
  }

  // Enter in a step label adds the next step — a checklist is written in one pass.
  function onLabelKeydown(e, idx) {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    if (idx === items.length - 1) addItem();
  }

  const stepCount = $derived(items.filter((i) => i.label.trim()).length);

  async function save() {
    const cleaned = items
      .map((i) => ({
        id: i.id,
        label: i.label.trim(),
        note: i.note ?? '',
        url: (i.url ?? '').trim(),
        link_label: (i.link_label ?? '').trim()
      }))
      .filter((i) => i.label);

    if (!name.trim()) {
      error = $t('routines.editor.nameRequired');
      return;
    }
    if (cleaned.length === 0) {
      error = $t('routines.editor.addAtLeastOneStep');
      return;
    }
    if (startDate && endDate && endDate < startDate) {
      error = $t('routines.schedule.endBeforeStart');
      return;
    }

    saving = true;
    error = '';
    const payload = {
      name: name.trim(),
      description: description.trim(),
      notes,
      category_id: categoryId || null,
      schedule,
      start_date: startDate || null,
      end_date: endDate || null,
      items: cleaned
    };
    try {
      if (routineId) {
        await traderApi.updateRoutine(routineId, { ...payload, active });
      } else {
        await traderApi.createRoutine(payload);
      }
      open = false;
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Deleting a template takes its tick history with it — ConfirmModal, not confirm().
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

<Modal
  bind:open
  size="lg"
  title={routineId ? $t('routines.editor.editTemplate') : $t('routines.editor.newTemplate')}
>
  <div class="form">
    {#if loading}
      <p class="loading">{$t('common.loading')}</p>
    {/if}

    <!-- ── 1. Identity ── -->
    <section>
      <h3 class="shead">{$t('routines.editor.aboutSection')}</h3>
      <div class="row">
        <label class="grow">
          {$t('routines.editor.name')}
          <input placeholder={$t('routines.editor.namePlaceholder')} bind:value={name} />
        </label>
        <div class="fld">
          <span class="flbl">{$t('routines.editor.category')}</span>
          <Dropdown
            bind:value={categoryId}
            ariaLabel={$t('routines.editor.category')}
            options={[
              { value: '', label: $t('routines.editor.noCategory') },
              ...categories.map((c) => ({ value: c.id, label: c.name, color: c.color }))
            ]}
          />
        </div>
      </div>
      <label>
        {$t('routines.editor.description')}
        <input placeholder={$t('routines.editor.descriptionPlaceholder')} bind:value={description} />
      </label>
      <label class="stack">
        {$t('routines.editor.notes')}
        <RichNote bind:value={notes} placeholder={$t('routines.editor.notesPlaceholder')} />
      </label>
    </section>

    <!-- ── 2. Schedule ── -->
    <section>
      <h3 class="shead">{$t('routines.editor.whenSection')}</h3>
      <SchedulePicker bind:schedule bind:startDate bind:endDate />
    </section>

    <!-- ── 3. Checklist ── -->
    <section>
      <h3 class="shead">
        {$t('routines.editor.stepsSection')}
        <span class="count">{$t('routines.editor.stepCount', { n: stepCount })}</span>
      </h3>

      <ul class="steps">
        {#each items as item, i (item.id ?? i)}
          <li class="step" class:expanded={item._open}>
            <div class="srow">
              <span class="handle" aria-hidden="true"><Icon name="grip-vertical" size={13} /></span>
              <input
                class="slabel"
                placeholder={$t('routines.editor.stepPlaceholder')}
                bind:value={item.label}
                onkeydown={(e) => onLabelKeydown(e, i)}
              />
              <button
                type="button"
                class="sbtn"
                class:has={!!(item.note || item.url)}
                title={$t('routines.editor.stepDetails')}
                aria-label={$t('routines.editor.stepDetails')}
                aria-expanded={item._open}
                onclick={() => toggleItemDetail(i)}
              >
                <Icon name={item._open ? 'chevron-down' : 'chevron-right'} size={13} />
                <Icon name="file-text" size={13} />
              </button>
              <button type="button" class="sbtn" title={$t('routines.editor.moveUp')} aria-label={$t('routines.editor.moveUp')} onclick={() => move(i, -1)} disabled={i === 0}>
                <Icon name="arrow-up" size={13} />
              </button>
              <button type="button" class="sbtn" title={$t('routines.editor.moveDown')} aria-label={$t('routines.editor.moveDown')} onclick={() => move(i, 1)} disabled={i === items.length - 1}>
                <Icon name="arrow-down" size={13} />
              </button>
              <button type="button" class="sbtn del" title={$t('routines.editor.removeStep')} aria-label={$t('routines.editor.removeStep')} onclick={() => removeItem(i)}>
                <Icon name="x" size={13} />
              </button>
            </div>

            {#if item._open}
              <div class="sdetail">
                <RichNote
                  bind:value={item.note}
                  compact
                  placeholder={$t('routines.editor.stepNotePlaceholder')}
                />
                <div class="linkrow">
                  <label class="grow">
                    {$t('routines.editor.stepLink')}
                    <input placeholder="https://" bind:value={item.url} />
                  </label>
                  <label class="grow">
                    {$t('routines.editor.stepLinkLabel')}
                    <input
                      placeholder={$t('routines.editor.stepLinkLabelPlaceholder')}
                      bind:value={item.link_label}
                    />
                  </label>
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>

      <button type="button" class="add" onclick={addItem}>
        <Icon name="plus" size={13} /> {$t('routines.editor.addStep')}
      </button>
    </section>

    <ErrorText error={error} />

    <div class="foot">
      {#if routineId}
        <button type="button" class="btn danger" onclick={remove}>{$t('common.delete')}</button>
        <label class="pause">
          <input type="checkbox" bind:checked={active} />
          {$t('routines.editor.activeToggle')}
        </label>
      {/if}
      <div class="spacer"></div>
      <button type="button" class="btn" onclick={() => (open = false)}>{$t('common.cancel')}</button>
      <button type="button" class="btn primary" onclick={save} disabled={saving}>
        {saving ? $t('common.saving') : routineId ? $t('common.save') : $t('routines.editor.create')}
      </button>
    </div>
  </div>
</Modal>

<!-- After the host Modal in DOM order, so at equal --z-modal it stacks on top. -->
<ConfirmModal
  bind:open={confirmOpen}
  title={$t('routines.editor.deleteTemplate')}
  message={$t('routines.editor.confirmDelete')}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
    padding-bottom: var(--space-2);
  }
  .loading {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }

  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  /* The section title is a quiet caption, not a heading competing with the modal title:
     uppercase micro-type over a rule, so the eye reads the fields, not the labels. */
  .shead {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    padding-bottom: var(--space-2);
    border-bottom: 0.5px solid var(--border);
  }
  .count {
    letter-spacing: 0;
    text-transform: none;
    font-weight: 400;
  }

  /* .fld wraps a control that isn't a native input (the category Dropdown), so it can't
     live inside a <label>; it repeats the label's own column layout. */
  label,
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .flbl {
    line-height: var(--lh-tight, 1.2);
  }
  /* The Dropdown trigger is width:100%, so its wrapper needs a basis of its own or it
     collapses next to the flexible name field. */
  .row .fld {
    flex: 0 1 220px;
  }
  .row {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .grow {
    flex: 1 1 220px;
  }
  .stack {
    gap: var(--space-2);
  }

  /* ── Steps ── */
  .steps {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
  }
  .step {
    border: 0.5px solid transparent;
    border-radius: var(--radius);
  }
  /* Expanded steps get a frame, so the detail block reads as belonging to its row. */
  .step.expanded {
    border-color: var(--border);
    background: var(--surface-2);
    padding: var(--space-2);
  }
  .srow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  /* The row actions are a group, set apart from the field they act on. */
  .srow .sbtn:first-of-type {
    margin-left: var(--space-1);
  }
  .handle {
    color: var(--muted);
    display: inline-flex;
    flex: none;
  }
  .slabel {
    flex: 1;
    min-width: 0;
  }
  .sbtn {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 3px 4px;
    border-radius: var(--radius);
    display: inline-flex;
    align-items: center;
    flex: none;
  }
  .sbtn:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .sbtn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .sbtn.del:hover {
    color: var(--red);
  }
  /* A filled step announces itself, so enrichment isn't lost behind a closed fold. */
  .sbtn.has {
    color: var(--accent);
  }

  .sdetail {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-2) var(--space-1) var(--space-4);
  }
  .linkrow {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .add {
    align-self: flex-start;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .add:hover {
    color: var(--text);
  }

  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .pause {
    flex-direction: row;
    align-items: center;
    gap: var(--space-1);
    font-weight: 400;
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .btn.danger {
    color: var(--red);
    border-color: var(--red);
    background: transparent;
  }
</style>
