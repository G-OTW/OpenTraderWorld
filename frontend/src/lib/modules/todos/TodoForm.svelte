<script>
  // Add/edit a todo: name, optional due date, free-form details.
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { t as tr } from '$lib/i18n';

  let { initial = null, categories = [], onsubmit = () => {}, oncancel = () => {} } = $props();

  let t = $state(blank());

  function blank() {
    const base = { name: '', due_date: '', due_time: '', details: '', category: '', done: false };
    if (initial) {
      return {
        ...base,
        ...initial,
        due_date: initial.due_date ?? '',
        due_time: initial.due_time ?? '',
        details: initial.details ?? '',
        category: initial.category ?? ''
      };
    }
    return base;
  }

  function payload() {
    return {
      name: t.name,
      due_date: t.due_date || null,
      // A time without a date is meaningless; only send it when a date is set.
      due_time: t.due_date && t.due_time ? t.due_time : null,
      details: t.details,
      category: (t.category || '').trim(),
      done: !!t.done
    };
  }

  function submit() {
    onsubmit(payload());
  }

  // For the reminder button: ensure the task exists (saving on create) and return its
  // link info. Returns null if the task has no name yet.
  async function resolveLink() {
    if (!t.name.trim()) return null;
    if (initial?.id) return { linkedId: initial.id, linkedName: t.name, defaultName: t.name };
    const saved = await onsubmit(payload(), { keepOpen: true });
    if (!saved?.id) return null;
    return { linkedId: saved.id, linkedName: t.name, defaultName: t.name };
  }
</script>

<form
  class="todo-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <label class="field">
    <span>{$tr('todos.form.task')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={t.name} autofocus placeholder={$tr('todos.form.taskPlaceholder')} />
  </label>

  <div class="row">
    <label class="field">
      <span>{$tr('todos.form.dueDate')}</span>
      <input type="date" bind:value={t.due_date} />
    </label>
    <label class="field">
      <span>{$tr('todos.form.time')}</span>
      <input type="time" bind:value={t.due_time} disabled={!t.due_date} />
    </label>
  </div>

  <label class="field">
    <span>{$tr('todos.form.category')}</span>
    <input bind:value={t.category} list="todo-categories" placeholder={$tr('todos.form.categoryPlaceholder')} />
    <datalist id="todo-categories">
      {#each categories as c}<option value={c}></option>{/each}
    </datalist>
  </label>

  <label class="field">
    <span>{$tr('todos.form.details')}</span>
    <textarea bind:value={t.details} rows="4" placeholder={$tr('todos.form.detailsPlaceholder')}></textarea>
  </label>

  <QuickReminderButton variant="inline" title={$tr('todos.addReminder')} kind="todo" {resolveLink} />

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$tr('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $tr('common.save') : $tr('todos.addTask')}</button>
  </div>
</form>

<style>
  .todo-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
  }
  .row .field {
    flex: 1;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
