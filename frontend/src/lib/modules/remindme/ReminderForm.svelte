<script>
  // Add/edit a reminder: the shared ReminderInput fields plus the form's own actions. Used
  // by the RemindMe page, the bell button on other pages and the dashboard widget.
  import ReminderInput from './ReminderInput.svelte';
  import { blankReminder, dayRunTooLong, linkInvalid, reminderPayload } from './draft.js';
  import { t } from '$lib/i18n';

  let {
    initial = null,
    goals = [],
    todos = [],
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  // Seeded once from `initial`: a closing modal or a {#key} block remounts the form.
  const seed = () => blankReminder(initial);
  let r = $state(seed());

  const invalid = $derived(linkInvalid(r) || dayRunTooLong(r));
</script>

<form
  class="rem-form"
  onsubmit={(e) => {
    e.preventDefault();
    onsubmit(reminderPayload(r));
  }}
>
  <ReminderInput bind:value={r} {goals} {todos} />

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary" disabled={invalid}>{initial ? $t('common.save') : $t('remindme.addReminder')}</button>
  </div>
</form>

<style>
  .rem-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  @media (max-width: 767px) {
    .actions button { min-height: 44px; }
    .actions { flex-wrap: wrap; }
  }
</style>
