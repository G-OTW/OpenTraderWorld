<script>
  // Add/edit a prompt: name, tag chips, and a large body editor. Enter (or a "+"
  // button) in the tag box adds a chip; existing tags autocomplete via a datalist.
  // When editing an existing prompt, a "History" button asks the parent to open the
  // version-history modal (rollback appends the chosen content as a new version).
  import Icon from '$lib/ui/Icon.svelte';
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
  let tagDraft = $state('');

  const suggestions = $derived(allTags.filter((t) => !tags.some((x) => x.toLowerCase() === t.toLowerCase())));

  function addTag() {
    const v = tagDraft.trim();
    if (!v) return;
    if (!tags.some((x) => x.toLowerCase() === v.toLowerCase())) tags = [...tags, v];
    tagDraft = '';
  }
  function removeTag(t) {
    tags = tags.filter((x) => x !== t);
  }
  function onTagKey(e) {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      addTag();
    } else if (e.key === 'Backspace' && !tagDraft && tags.length) {
      tags = tags.slice(0, -1);
    }
  }

  function submit() {
    if (tagDraft.trim()) addTag(); // fold a half-typed tag before saving
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
    <div class="tag-input">
      {#each tags as t (t)}
        <span class="chip">
          {t}
          <button type="button" class="chip-x" onclick={() => removeTag(t)} aria-label={$tr('promptStore.form.removeTag', { tag: t })}>
            <Icon name="x" size={11} />
          </button>
        </span>
      {/each}
      <input
        class="tag-draft"
        bind:value={tagDraft}
        onkeydown={onTagKey}
        list="prompt-tag-suggestions"
        placeholder={tags.length ? '' : $tr('promptStore.form.tagsPlaceholder')}
      />
      <datalist id="prompt-tag-suggestions">
        {#each suggestions as s}<option value={s}></option>{/each}
      </datalist>
      <button type="button" class="tag-add" onclick={addTag} disabled={!tagDraft.trim()} title={$tr('promptStore.form.addTag')}>
        <Icon name="plus" size={13} />
      </button>
    </div>
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
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: var(--text-base);
  }
  .tag-input {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 8px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 2px 4px 2px 8px;
    font-size: var(--text-xs);
  }
  .chip-x {
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 1px;
    border-radius: var(--radius);
  }
  .chip-x:hover {
    color: var(--red);
  }
  .tag-draft {
    flex: 1;
    min-width: 100px;
    border: none;
    background: transparent;
    padding: 4px 2px;
    font-size: var(--text-base);
  }
  .tag-draft:focus {
    outline: none;
  }
  .tag-add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    width: 26px;
    height: 26px;
    cursor: pointer;
  }
  .tag-add:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent);
  }
  .tag-add:disabled {
    opacity: 0.4;
    cursor: default;
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
