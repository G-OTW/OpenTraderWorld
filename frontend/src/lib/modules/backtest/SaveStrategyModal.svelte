<script>
  // Name + tags for the strategy being saved. Tags are what the library's search box matches
  // besides the name, so they are edited here rather than hidden behind a second screen.
  // Enter in the tag box adds a chip; Enter in the name box saves.
  import Modal from '$lib/ui/Modal.svelte';
  import TagInput from '$lib/ui/TagInput.svelte';
  import { KIND_LABEL_KEYS, settingsKind } from './api.js';
  import { t } from '$lib/i18n';

  // The draft is local: cancelling leaves the strategy exactly as it was, and `onsave` is what
  // hands the edited name/tags back to the page.
  let {
    open = $bindable(false),
    initialName = '',
    initialTags = [],
    allTags = [],
    error = '',
    kind = 'signals',
    // The strategy already exists in the library: saving is then a choice, not a default.
    // Overwriting it rewrites what every run and every session points at.
    existing = false,
    onsave
  } = $props();

  let name = $state('');
  let tags = $state([]);
  let tagBox = $state(null);
  let overwrite = $state(false);

  // Reseed from the strategy each time the modal opens. A named strategy defaults to a
  // copy: the destructive answer is never the one already selected.
  $effect(() => {
    if (!open) return;
    overwrite = false;
    name = existing ? copyName(initialName) : initialName;
    tags = [...initialTags];
  });

  /** "Name" to "Name (2)", "Name (2)" to "Name (3)". */
  function copyName(base) {
    const m = /^(.*?)\s*\((\d+)\)$/.exec(base ?? '');
    return m ? `${m[1]} (${Number(m[2]) + 1})` : `${base ?? ''} (2)`;
  }

  // Overwriting keeps the stored name: renaming and overwriting at once is two decisions
  // read as one.
  $effect(() => {
    if (overwrite) name = initialName;
  });

  const suggestions = $derived(allTags.filter((x) => !tags.some((v) => v.toLowerCase() === x.toLowerCase())));

  function save() {
    tagBox?.flush(); // fold a half-typed tag before saving
    onsave?.({ name: name.trim(), tags: [...tags], overwrite: existing ? overwrite : true });
  }
</script>

<Modal bind:open title={$t('backtest.save.title')}>
  <!-- Which engine is being saved: the name and the tags say nothing about it, and a signals
       strategy and a DCA plan are not interchangeable. -->
  <p class="kind">
    <span>{$t('backtest.save.kind')}</span>
    <b>{$t(KIND_LABEL_KEYS[settingsKind({ kind })])}</b>
  </p>

  {#if existing}
    <div class="field">
      <span>{$t('backtest.save.mode')}</span>
      <div class="seg" role="group">
        <button class:on={!overwrite} onclick={() => (overwrite = false)}>
          {$t('backtest.save.asNew')}
        </button>
        <button class:on={overwrite} onclick={() => (overwrite = true)}>
          {$t('backtest.save.overwrite')}
        </button>
      </div>
      <p class="hint">
        {overwrite ? $t('backtest.save.overwriteHint') : $t('backtest.save.asNewHint')}
      </p>
    </div>
  {/if}

  <label class="field">
    <span>{$t('backtest.save.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={name} autofocus disabled={overwrite}
      placeholder={$t('backtest.save.namePlaceholder')}
      onkeydown={(e) => e.key === 'Enter' && save()} />
  </label>

  <div class="field">
    <span>{$t('backtest.save.tags')}</span>
    <TagInput
      bind:this={tagBox}
      bind:tags
      {suggestions}
      listId="bt-strategy-tags"
      placeholder={$t('backtest.save.tagsPlaceholder')}
      removeLabel={(tag) => $t('backtest.save.removeTag', { tag })}
    />
    <p class="hint">{$t('backtest.save.tagsHint')}</p>
  </div>

  {#if error}<p class="err">{error}</p>{/if}

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={save}>{$t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
    margin-bottom: var(--space-3);
  }
  .kind {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    margin-bottom: var(--space-3);
  }
  .kind b {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .seg {
    display: inline-flex;
    border: var(--hairline) solid var(--border-control);
    width: fit-content;
  }
  .seg button {
    background: transparent;
    border: 0;
    border-left: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .seg button:first-child {
    border-left: 0;
  }
  .seg button.on {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
    font-style: italic;
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
  }
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .primary {
    background: color-mix(in srgb, var(--accent) 22%, var(--surface-2));
  }
</style>
