<script>
  // Add/edit a prompt: name, tag chips, and a large body editor. Enter (or a "+"
  // button) in the tag box adds a chip; existing tags autocomplete via a datalist.
  // When editing an existing prompt, a "History" button asks the parent to open the
  // version-history modal (rollback appends the chosen content as a new version).
  import Icon from '$lib/ui/Icon.svelte';
  import TagInput from '$lib/ui/TagInput.svelte';
  import { t as tr } from '$lib/i18n';

  let {
    initial = null,
    allTags = [],
    onsubmit = () => {},
    onhistory = () => {},
    oncancel = () => {}
  } = $props();

  let name = $state(initial?.name ?? '');
  let body = $state(initial?.body ?? '');
  let tags = $state([...(initial?.tags ?? [])]);
  let tagBox = $state(null);

  const suggestions = $derived(allTags.filter((t) => !tags.some((x) => x.toLowerCase() === t.toLowerCase())));

  function submit() {
    tagBox?.flush(); // fold a half-typed tag before saving
    onsubmit({ name: name.trim(), body, tags });
  }

  const canSave = $derived(name.trim().length > 0);
</script>

<form
  class="prompt-form"
  onsubmit={(e) => {
    e.preventDefault();
    if (canSave) submit();
  }}
>
  <label class="field">
    <span>{$tr('promptStore.form.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={name} autofocus placeholder={$tr('promptStore.form.namePlaceholder')} />
  </label>

  <div class="field">
    <span>{$tr('promptStore.form.tags')}</span>
    <TagInput
      bind:this={tagBox}
      bind:tags
      {suggestions}
      listId="prompt-tag-suggestions"
      placeholder={$tr('promptStore.form.tagsPlaceholder')}
      removeLabel={(tag) => $tr('promptStore.form.removeTag', { tag })}
      addTitle={$tr('promptStore.form.addTag')}
    />
  </div>

  <label class="field grow">
    <span>{$tr('promptStore.form.body')}</span>
    <textarea bind:value={body} rows="12" placeholder={$tr('promptStore.form.bodyPlaceholder')}></textarea>
  </label>

  <div class="actions">
    {#if initial?.id}
      <button type="button" class="ghost history" onclick={() => onhistory()}>
        <Icon name="rotate-ccw" size={13} /> {$tr('promptStore.form.history')}
      </button>
    {/if}
    <span class="spacer"></span>
    <button type="button" class="ghost" onclick={oncancel}>{$tr('common.cancel')}</button>
    <button type="submit" class="primary" disabled={!canSave}>
      {initial ? $tr('common.save') : $tr('promptStore.addPrompt')}
    </button>
  </div>
</form>

<style>
  .prompt-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field > span {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  input,
  textarea {
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 8px 10px;
    font: inherit;
    font-size: var(--text-base);
    width: 100%;
  }
  textarea {
    resize: vertical;
    min-height: 200px;
    line-height: 1.5;
    font-family: var(--mono);
    font-size: var(--text-base);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .history {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
</style>
