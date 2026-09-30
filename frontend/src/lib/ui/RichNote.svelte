<script>
  // A small rich-text field for template and step notes: bold/italic/underline, lists, a
  // link, and nothing else.
  //
  // Deliberately not the editor module's Tiptap instance — that is a document surface with
  // a slash menu, images and code blocks, and a template form can hold a dozen of these at
  // once. This is a contenteditable with a five-button toolbar, matching exactly the tag
  // set the API's sanitiser keeps, so what is typed is what is stored.
  import Icon from '$lib/ui/Icon.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import { t } from '$lib/i18n';

  let { value = $bindable(''), placeholder = '', compact = false } = $props();

  let el = $state(null);
  let focused = $state(false);
  let linkPrompt = $state(false);

  // The DOM is the source of truth while typing: writing `value` back into innerHTML on
  // every keystroke would reset the caret to the start. Sync only when the incoming value
  // diverges from what the element already shows (load, or an external reset).
  $effect(() => {
    const next = value ?? '';
    if (el && el.innerHTML !== next) el.innerHTML = next;
  });

  function sync() {
    if (!el) return;
    // An "empty" contenteditable still holds a stray <br> — normalise it to '' so the
    // placeholder shows and the field doesn't count as filled.
    const html = el.innerHTML;
    value = html === '<br>' || html === '<p><br></p>' ? '' : html;
  }

  // execCommand is deprecated but is still the only API that edits a contenteditable
  // selection without shipping an editor framework. The output is normalised by the
  // server's sanitiser, so the tag soup browsers emit never reaches storage.
  function cmd(name, arg = null) {
    el?.focus();
    document.execCommand(name, false, arg);
    sync();
  }

  function addLink(values) {
    const url = (values?.url ?? '').trim();
    if (!url) return;
    const safe = /^https?:\/\//i.test(url) ? url : `https://${url}`;
    cmd('createLink', safe);
  }

  // Enter inside a note should break the line, not submit the surrounding form.
  function onKeydown(e) {
    if (e.key === 'Enter') e.stopPropagation();
  }

  // Paste as plain text: pasting from a web page otherwise drags in styles and markup the
  // sanitiser will strip anyway, so the user would see it vanish on save.
  function onPaste(e) {
    e.preventDefault();
    const text = e.clipboardData?.getData('text/plain') ?? '';
    document.execCommand('insertText', false, text);
    sync();
  }

  const buttons = [
    { cmd: 'bold', icon: 'bold', key: 'routines.note.bold' },
    { cmd: 'italic', icon: 'italic', key: 'routines.note.italic' },
    { cmd: 'insertUnorderedList', icon: 'list', key: 'routines.note.bulletList' }
  ];
</script>

<div class="note" class:focused class:compact>
  <div class="toolbar">
    {#each buttons as b (b.cmd)}
      <button type="button" title={$t(b.key)} aria-label={$t(b.key)} onclick={() => cmd(b.cmd)}>
        <Icon name={b.icon} size={13} />
      </button>
    {/each}
    <button
      type="button"
      title={$t('routines.note.link')}
      aria-label={$t('routines.note.link')}
      onclick={() => (linkPrompt = true)}
    >
      <Icon name="link" size={13} />
    </button>
    <div class="spacer"></div>
    {#if value}
      <button
        type="button"
        class="clear"
        title={$t('routines.note.clear')}
        aria-label={$t('routines.note.clear')}
        onclick={() => {
          value = '';
          if (el) el.innerHTML = '';
        }}
      >
        <Icon name="x" size={12} />
      </button>
    {/if}
  </div>

  <div
    bind:this={el}
    class="body"
    role="textbox"
    tabindex="0"
    aria-multiline="true"
    aria-label={placeholder}
    contenteditable="true"
    data-placeholder={placeholder}
    class:blank={!value}
    oninput={sync}
    onblur={() => {
      focused = false;
      sync();
    }}
    onfocus={() => (focused = true)}
    onkeydown={onKeydown}
    onpaste={onPaste}
  ></div>
</div>

<PromptModal
  bind:open={linkPrompt}
  title={$t('routines.note.link')}
  fields={[{ key: 'url', label: $t('routines.note.linkUrl'), placeholder: 'https://', required: true }]}
  confirmLabel={$t('routines.note.insert')}
  onconfirm={addLink}
/>

<style>
  .note {
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    overflow: hidden;
  }
  .note.focused {
    border-color: var(--accent);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px var(--space-1);
    border-bottom: 0.5px solid var(--border);
  }
  .toolbar button {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 3px 5px;
    border-radius: var(--radius);
    display: inline-flex;
  }
  .toolbar button:hover {
    color: var(--text);
    background: var(--surface);
  }
  .toolbar .clear:hover {
    color: var(--red);
  }
  .spacer {
    flex: 1;
  }

  .body {
    padding: var(--space-2);
    min-height: 66px;
    font-size: var(--text-base);
    color: var(--text);
    line-height: var(--lh-normal, 1.5);
    outline: none;
    overflow-y: auto;
    max-height: 240px;
  }
  .compact .body {
    min-height: 44px;
  }

  /* The placeholder is drawn rather than typed, so it can't end up in the saved value. */
  .body.blank::before {
    content: attr(data-placeholder);
    color: var(--muted);
    pointer-events: none;
  }

  /* Nested content styling — the note body renders sanitised HTML, so reach it globally. */
  .body :global(p) {
    margin: 0 0 var(--space-1);
  }
  .body :global(p:last-child) {
    margin-bottom: 0;
  }
  .body :global(ul),
  .body :global(ol) {
    margin: 0 0 var(--space-1);
    padding-left: var(--space-4);
  }
  .body :global(a) {
    color: var(--accent);
  }
  .body :global(code) {
    background: var(--surface);
    border-radius: var(--radius);
    padding: 0 3px;
    font-size: 0.92em;
  }
</style>
