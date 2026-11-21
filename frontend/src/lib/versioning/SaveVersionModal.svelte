<script>
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { t } from '$lib/i18n';

  // Save a version: the date is stamped by the server, the note is optional.
  // props: open (bindable), name (what is being versioned), onsave(note) -> Promise
  let { open = $bindable(false), name = '', onsave } = $props();

  let note = $state('');
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    if (open) {
      note = '';
      error = '';
    }
  });

  async function save() {
    busy = true;
    error = '';
    try {
      await onsave?.(note.trim());
      open = false;
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={$t('versioning.save.title')}>
  {#if name}<p class="lead">{name}</p>{/if}
  <label class="field">
    <span>{$t('versioning.save.note')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <textarea
      bind:value={note}
      rows="3"
      maxlength="500"
      autofocus
      placeholder={$t('versioning.save.notePlaceholder')}
      onkeydown={(e) => (e.metaKey || e.ctrlKey) && e.key === 'Enter' && save()}
    ></textarea>
  </label>
  <ErrorText {error} />

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={busy} onclick={save}>
      {busy ? $t('common.saving') : $t('versioning.save.confirm')}
    </button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  textarea {
    resize: vertical;
    height: 280px;
    min-height: 72px;
    font: inherit;
    color: var(--text);
  }
</style>
