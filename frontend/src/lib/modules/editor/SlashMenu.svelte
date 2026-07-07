<script>
  import { t } from '$lib/i18n';
  // state: { open, x, y, query }  — bound from parent
  // onpick(cmd) — selected command
  // onempty() — fired when the menu is open but the query matches nothing
  let { state = $bindable(), onpick, onempty = () => {} } = $props();

  // `run` receives a focused tiptap chain. Commands needing input declare
  // `prompt` (a label) — Editor will ask for a value and pass it as 2nd arg.
  // Labels/hints resolved via $t at render so they relabel on language change.
  const commands = [
    { labelKey: 'editor.slashMenu.heading1', hintKey: 'editor.slashMenu.heading1Hint', run: (c) => c.toggleHeading({ level: 1 }) },
    { labelKey: 'editor.slashMenu.heading2', hintKey: 'editor.slashMenu.heading2Hint', run: (c) => c.toggleHeading({ level: 2 }) },
    { labelKey: 'editor.slashMenu.heading3', hintKey: 'editor.slashMenu.heading3Hint', run: (c) => c.toggleHeading({ level: 3 }) },
    { labelKey: 'editor.slashMenu.bulletList', hintKey: 'editor.slashMenu.bulletListHint', run: (c) => c.toggleBulletList() },
    { labelKey: 'editor.slashMenu.numberedList', hintKey: 'editor.slashMenu.numberedListHint', run: (c) => c.toggleOrderedList() },
    { labelKey: 'editor.slashMenu.todoList', hintKey: 'editor.slashMenu.todoListHint', run: (c) => c.toggleTaskList() },
    { labelKey: 'editor.slashMenu.quote', hintKey: 'editor.slashMenu.quoteHint', run: (c) => c.toggleBlockquote() },
    { labelKey: 'editor.slashMenu.codeBlock', hintKey: 'editor.slashMenu.codeBlockHint', run: (c) => c.toggleCodeBlock() },
    { labelKey: 'editor.slashMenu.divider', hintKey: 'editor.slashMenu.dividerHint', run: (c) => c.setHorizontalRule() },
    { labelKey: 'editor.slashMenu.link', hintKey: 'editor.slashMenu.linkHint', promptKey: 'editor.slashMenu.linkUrl', run: (c, url) => c.extendMarkRange('link').setLink({ href: url }) },
    { labelKey: 'editor.slashMenu.imageUpload', hintKey: 'editor.slashMenu.imageUploadHint', upload: true, run: (c, url) => c.setImage({ src: url }) },
    { labelKey: 'editor.slashMenu.imageByUrl', hintKey: 'editor.slashMenu.imageByUrlHint', promptKey: 'editor.slashMenu.imageUrl', run: (c, url) => c.setImage({ src: url }) }
  ];

  const filtered = $derived(
    commands.map((c) => ({ ...c, label: $t(c.labelKey), hint: $t(c.hintKey), prompt: c.promptKey ? $t(c.promptKey) : undefined })).filter((c) => {
      const q = (state.query ?? '').toLowerCase().trim();
      if (!q) return true;
      return `${c.label} ${c.hint}`.toLowerCase().includes(q);
    })
  );

  // When the user has typed something that matches no command, stop suggesting
  // and let them keep typing freely (the parent closes the menu).
  $effect(() => {
    if (state.open && (state.query ?? '').length > 0 && filtered.length === 0) {
      onempty();
    }
  });

  function pick(cmd) {
    onpick(cmd);
  }

  function close() {
    state = { ...state, open: false };
  }
</script>

{#if state.open}
  <!-- Click-catcher: any click outside the menu closes it. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="slash-backdrop" onclick={close} role="presentation"></div>
  {#if filtered.length > 0}
    <div class="slash" style="left:{state.x}px; top:{state.y}px;" role="menu">
      {#each filtered as cmd}
        <button class="row" onclick={() => pick(cmd)} role="menuitem">
          <span class="lbl">{cmd.label}</span>
          <span class="hint">{cmd.hint}</span>
        </button>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .slash-backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
  }
  .slash {
    position: fixed;
    z-index: 100;
    width: 230px;
    max-height: 280px;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    padding: var(--space-1);
  }
  .row {
    display: flex;
    flex-direction: column;
    width: 100%;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .lbl {
    color: var(--text);
    font-size: 0.85rem;
    font-weight: 600;
  }
  .hint {
    color: var(--muted);
    font-size: 0.72rem;
  }
  .empty {
    color: var(--muted);
    font-size: 0.8rem;
    padding: var(--space-2) var(--space-3);
  }
</style>
