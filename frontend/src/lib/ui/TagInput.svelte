<script>
  // The app's tag field: chips and a free input inside one box.
  //
  // The box *is* the field, so it draws the only border there is and a chip inside it is a
  // tinted fill, never a second frame. Enter or a comma commits a tag, Backspace on an empty
  // input takes the last one back, duplicates are refused case-insensitively.
  //
  // A half-typed tag is not lost on save: `flush()` folds it in (call it from the parent's
  // submit, via bind:this) and so does leaving the field.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let {
    tags = $bindable([]),
    /** Other tags to offer as completions. */
    suggestions = [],
    placeholder = '',
    /** Unique per instance: two datalists cannot share an id on one page. */
    listId = 'tag-input',
    /** (tag) => label for the remove button; falls back to the generic one. */
    removeLabel = null,
    /** Add button, for pointer users who never press Enter. */
    addTitle = ''
  } = $props();

  let draft = $state('');

  /** Commit whatever is typed. Returns nothing: `tags` is the state. */
  export function flush() {
    const v = draft.trim();
    if (!v) return;
    if (!tags.some((x) => x.toLowerCase() === v.toLowerCase())) tags = [...tags, v];
    draft = '';
  }

  const remove = (tag) => (tags = tags.filter((x) => x !== tag));

  function onKey(e) {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      flush();
    } else if (e.key === 'Backspace' && !draft && tags.length) {
      tags = tags.slice(0, -1);
    }
  }
</script>

<div class="tag-box">
  {#each tags as tag (tag)}
    <span class="chip">
      {tag}
      <button
        type="button"
        class="chip-x"
        onclick={() => remove(tag)}
        aria-label={removeLabel ? removeLabel(tag) : $t('common.remove')}
      >
        <Icon name="x" size={10} />
      </button>
    </span>
  {/each}
  <input
    class="draft"
    bind:value={draft}
    list={listId}
    onkeydown={onKey}
    onblur={flush}
    placeholder={tags.length ? '' : placeholder}
  />
  <datalist id={listId}>
    {#each suggestions as s (s)}<option value={s}></option>{/each}
  </datalist>
  {#if draft.trim()}
    <button type="button" class="add" onclick={flush} title={addTitle || $t('common.add')}>
      <Icon name="plus" size={12} />
    </button>
  {/if}
</div>

<style>
  /* The same box every other field has: one hairline, square, accent on focus. */
  .tag-box {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    min-height: var(--control-h);
    padding: 3px var(--space-2);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    transition: border-color var(--dur-fast) var(--ease);
  }
  .tag-box:focus-within {
    border-color: var(--accent);
  }
  /* A chip is a fill, not a frame: inside a bordered field a second border is noise. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--space-2);
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--text);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .chip-x {
    display: inline-flex;
    background: transparent;
    border: none;
    padding: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .chip-x:hover {
    color: var(--red);
  }
  .draft {
    flex: 1;
    min-width: 110px;
    height: auto;
    padding: 2px;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
  }
  .draft:focus {
    outline: none;
    border: none;
  }
  .add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .add:hover {
    color: var(--text);
  }
</style>
