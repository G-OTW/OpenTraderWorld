<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Icon button that opens a modal to create a reminder inline on any page, without
  // navigating to the RemindMe module. Creates a custom reminder via remindApi.add.
  import { remindApi } from './api.js';
  import ReminderForm from './ReminderForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  // kind/linkedId pre-link the reminder to a goal/todo; defaultName seeds the name.
  // variant: 'icon' (header bell) | 'inline' (text button inside a form).
  // resolveLink: optional async () => { linkedId, linkedName, defaultName } resolved on click
  //   (e.g. the create modal saves the item first, then returns its new id).
  let {
    title = '',
    defaultName = '',
    kind = 'custom',
    linkedId = null,
    linkedName = '',
    variant = 'icon',
    resolveLink = null
  } = $props();

  let open = $state(false);
  let saving = $state(false);
  let busy = $state(false);
  // Resolved link info; falls back to the static props.
  let link = $state(null);

  const effTitle = $derived(title || $t('remindme.addReminder'));
  const effId = $derived(link?.linkedId ?? linkedId);
  const effName = $derived(link?.linkedName ?? linkedName);
  const effDefault = $derived(link?.defaultName ?? defaultName);
  const prelinked = $derived(kind !== 'custom' && effId != null);

  // Feed the form a one-item list so its linked-item <select> shows the pre-linked
  // goal/todo by name.
  const goals = $derived(prelinked && kind === 'goal' ? [{ id: effId, name: effName }] : []);
  const todos = $derived(prelinked && kind === 'todo' ? [{ id: effId, name: effName }] : []);

  const initial = $derived(
    prelinked || effDefault ? { name: effDefault, kind, linked_id: effId } : null
  );

  async function openModal() {
    if (resolveLink) {
      busy = true;
      try {
        link = await resolveLink();
        if (!link) return; // resolver aborted (e.g. validation failed)
      } finally {
        busy = false;
      }
    }
    open = true;
  }

  async function save(payload) {
    saving = true;
    try {
      await remindApi.add(payload);
      open = false;
    } finally {
      saving = false;
    }
  }
</script>

{#if variant === 'inline'}
  <button type="button" class="rem-inline" disabled={busy} onclick={openModal}><Icon name="bell" size={13} /> {effTitle}</button>
{:else}
  <button class="rem-icon" title={effTitle} aria-label={effTitle} disabled={busy} onclick={openModal}><Icon name="bell" size={15} /></button>
{/if}

<Modal bind:open title={$t('remindme.form.newTitle')} size="md">
  {#key open}
    <ReminderForm {initial} {goals} {todos} onsubmit={save} oncancel={() => (open = false)} />
  {/key}
</Modal>

<style>
  .rem-icon {
    width: 36px;
    height: 36px;
    border-radius: var(--radius);
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text);
    font-size: var(--text-md);
    cursor: pointer;
  }
  .rem-icon:hover {
    border-color: var(--accent);
  }
  .rem-inline {
    align-self: flex-start;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 6px 12px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .rem-inline:hover {
    border-color: var(--accent);
  }
</style>
