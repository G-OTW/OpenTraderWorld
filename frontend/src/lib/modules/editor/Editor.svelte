<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { onMount, onDestroy } from 'svelte';
  import { Editor } from '@tiptap/core';
  import StarterKit from '@tiptap/starter-kit';
  import Placeholder from '@tiptap/extension-placeholder';
  import { TaskList } from '@tiptap/extension-task-list';
  import { TaskItem } from '@tiptap/extension-task-item';
  import { ResizableImage as Image } from './ResizableImage.js';
  import Link from '@tiptap/extension-link';
  import { TextStyle, Color, FontSize } from '@tiptap/extension-text-style';
  import Highlight from '@tiptap/extension-highlight';
  import SlashMenu from './SlashMenu.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { uploadFile, pickFile } from './files-api.js';
  import { t } from '$lib/i18n';

  // props: content (ProseMirror JSON | null), onChange(json), titleSlot (snippet),
  // layout ('normal' | 'wide'), onLayoutChange(layout)
  let {
    content = null,
    onChange = () => {},
    titleSlot = null,
    layout = 'normal',
    onLayoutChange = () => {}
  } = $props();

  let element;
  let editor = $state(null);
  let slash = $state({ open: false, x: 0, y: 0, query: '' });
  // Document position right after the '/' that opened the menu (start of query).
  let slashFrom = 0;

  // ── Modals ──
  // A single prompt modal, reconfigured per call-to-action.
  let prompt = $state({ open: false, title: '', fields: [], confirmLabel: 'OK', onconfirm: () => {} });
  let errorMsg = $state(''); // non-empty => error modal shown

  function openPrompt(cfg) {
    prompt = { open: true, confirmLabel: 'OK', onconfirm: () => {}, ...cfg };
  }

  onMount(() => {
    editor = new Editor({
      element,
      extensions: [
        StarterKit.configure({
          heading: { levels: [1, 2, 3] }
        }),
        Placeholder.configure({
          placeholder: $t('editor.editor.placeholder')
        }),
        TaskList,
        TaskItem.configure({ nested: true }),
        Image,
        Link.configure({ openOnClick: false }),
        TextStyle,
        Color,
        FontSize,
        Highlight.configure({ multicolor: true })
      ],
      content: content ?? '',
      onUpdate: ({ editor }) => {
        onChange(editor.getJSON());
      },
      onTransaction: () => {
        // force reactive refresh of toolbar active states
        editor = editor;
        // Keep the slash query in sync with what's typed after the '/'.
        syncSlashQuery();
      }
    });

    // Slash-menu trigger: open when the user types '/' at the start of an empty block.
    element.addEventListener('keydown', onKeydown, true);
  });

  onDestroy(() => {
    editor?.destroy();
    element?.removeEventListener('keydown', onKeydown, true);
  });

  // Close any open toolbar popover when clicking outside the toolbar.
  $effect(() => {
    if (!openMenu) return;
    const onDoc = (e) => {
      if (!e.target.closest('.menu-wrap')) openMenu = null;
    };
    window.addEventListener('mousedown', onDoc);
    return () => window.removeEventListener('mousedown', onDoc);
  });

  // Replace content when switching documents.
  export function setContent(json) {
    if (editor) editor.commands.setContent(json ?? '');
  }

  // Faithful rendered HTML of the current doc — used when submitting for publication.
  export function getHTML() {
    return editor ? editor.getHTML() : '';
  }

  // ProseMirror JSON, kept alongside the HTML so a submission can be re-edited later.
  export function getJSON() {
    return editor ? editor.getJSON() : null;
  }

  // True when the document has no meaningful content (guards empty submissions).
  export function isEmpty() {
    return editor ? editor.isEmpty : true;
  }

  function onKeydown(e) {
    if (e.key === '/' && !slash.open) {
      // Open the slash menu near the caret. The query starts right after the '/'.
      requestAnimationFrame(() => {
        const sel = window.getSelection();
        if (!sel || sel.rangeCount === 0) return;
        const rect = sel.getRangeAt(0).getBoundingClientRect();
        slashFrom = editor.state.selection.from; // just past the inserted '/'
        slash = { open: true, x: rect.left, y: rect.bottom + 4, query: '' };
      });
    } else if (slash.open && e.key === 'Escape') {
      closeSlash();
    }
  }

  function closeSlash() {
    slash = { ...slash, open: false, query: '' };
  }

  // Read the text typed between the '/' and the caret, and keep the menu in sync.
  // Closes the menu if the caret leaves the query range or the query has no match.
  function syncSlashQuery() {
    if (!slash.open || !editor) return;
    const to = editor.state.selection.from;
    if (to < slashFrom) return closeSlash(); // caret moved before the query start
    const text = editor.state.doc.textBetween(slashFrom, to, '\n', '\n');
    // A space or newline ends the slash interaction — let the user type freely.
    if (/[\s]/.test(text)) return closeSlash();
    if (text !== slash.query) slash = { ...slash, query: text };
  }

  // Apply a slash command, removing the '/' and any typed query first.
  function applyCommand(cmd, arg) {
    if (!editor) return;
    const chain = editor.chain().focus();
    // Delete from the '/' (slashFrom - 1) through the current caret.
    chain.deleteRange({ from: slashFrom - 1, to: editor.state.selection.from });
    cmd.run(chain, arg).run();
  }

  async function runCommand(cmd) {
    slash = { ...slash, open: false };
    if (!editor) return;

    if (cmd.upload) {
      const file = await pickFile('image/*');
      if (!file) return editor.commands.focus();
      try {
        const { url } = await uploadFile(file);
        applyCommand(cmd, url);
      } catch (e) {
        errorMsg = $t('editor.editor.imageUploadFailed', { message: e.message });
      }
    } else if (cmd.prompt) {
      openPrompt({
        title: cmd.label,
        fields: [{ key: 'url', label: cmd.prompt, placeholder: 'https://…', type: 'url', required: true }],
        confirmLabel: $t('editor.editor.insert'),
        onconfirm: ({ url }) => applyCommand(cmd, url)
      });
    } else {
      applyCommand(cmd, undefined);
    }
  }

  // Toolbar helpers
  const is = (name, attrs) => editor?.isActive(name, attrs) ?? false;

  // ── Text styling (color / highlight / font size) ──
  // Which toolbar popover is open: 'color' | 'highlight' | null.
  let openMenu = $state(null);
  const toggleMenu = (name) => (openMenu = openMenu === name ? null : name);

  // User-selectable text/highlight colours — desaturated to the institutional family
  // (brick / gold / sage / slate / mauve / khaki) rather than saturated hues.
  const TEXT_COLORS = ['#c9776b', '#c9a45c', '#7fb894', '#7d8a99', '#9a95a3', '#b7a878'];
  const HIGHLIGHTS = ['#3a3320', '#2a3328', '#20303a', '#332028', '#2c2833', '#33302a'];
  const FONT_SIZES = ['12px', '14px', '16px', '18px', '24px', '32px'];

  function setColor(c) {
    editor?.chain().focus().setColor(c).run();
    openMenu = null;
  }
  function clearColor() {
    editor?.chain().focus().unsetColor().run();
    openMenu = null;
  }
  function setHighlight(c) {
    editor?.chain().focus().toggleHighlight({ color: c }).run();
    openMenu = null;
  }
  function clearHighlight() {
    editor?.chain().focus().unsetHighlight().run();
    openMenu = null;
  }
  function setFontSize(e) {
    const v = e.target.value;
    const chain = editor?.chain().focus();
    if (!chain) return;
    v ? chain.setFontSize(v).run() : chain.unsetFontSize().run();
  }
  // Current font size attr for the size <select>, '' when default.
  const currentFontSize = $derived(editor?.getAttributes('textStyle')?.fontSize ?? '');

  function addLink() {
    if (!editor) return;
    const prev = editor.getAttributes('link').href ?? '';
    openPrompt({
      title: $t('editor.editor.link'),
      fields: [{ key: 'url', label: $t('editor.editor.linkUrlLabel'), placeholder: 'https://…', type: 'url', value: prev }],
      confirmLabel: $t('editor.editor.apply'),
      onconfirm: ({ url }) => {
        const chain = editor.chain().focus().extendMarkRange('link');
        if (url && url.trim()) chain.setLink({ href: url.trim() }).run();
        else chain.unsetLink().run();
      }
    });
  }

  // Upload a local image file and insert it.
  async function addImage() {
    if (!editor) return;
    const file = await pickFile('image/*');
    if (!file) return;
    try {
      const { url } = await uploadFile(file);
      editor.chain().focus().setImage({ src: url }).run();
    } catch (e) {
      errorMsg = $t('editor.editor.imageUploadFailed', { message: e.message });
    }
  }

  // Insert an image from a URL (remote images / pasted links).
  function addImageUrl() {
    if (!editor) return;
    openPrompt({
      title: $t('editor.editor.imageFromUrl'),
      fields: [{ key: 'url', label: $t('editor.editor.imageUrl'), placeholder: 'https://…', type: 'url', required: true }],
      confirmLabel: $t('editor.editor.insert'),
      onconfirm: ({ url }) => editor.chain().focus().setImage({ src: url }).run()
    });
  }
</script>

<div class="editor-wrap">
  {#if editor}
    <div class="toolbar">
      <button class:active={is('heading', { level: 1 })} onclick={() => editor.chain().focus().toggleHeading({ level: 1 }).run()} title={$t('editor.editor.heading1')}>H1</button>
      <button class:active={is('heading', { level: 2 })} onclick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()} title={$t('editor.editor.heading2')}>H2</button>
      <button class:active={is('heading', { level: 3 })} onclick={() => editor.chain().focus().toggleHeading({ level: 3 }).run()} title={$t('editor.editor.heading3')}>H3</button>
      <span class="sep"></span>
      <button class:active={is('bold')} onclick={() => editor.chain().focus().toggleBold().run()} title={$t('editor.editor.bold')}><b>B</b></button>
      <button class:active={is('italic')} onclick={() => editor.chain().focus().toggleItalic().run()} title={$t('editor.editor.italic')}><i>I</i></button>
      <button class:active={is('strike')} onclick={() => editor.chain().focus().toggleStrike().run()} title={$t('editor.editor.strikethrough')}><s>S</s></button>
      <button class:active={is('code')} onclick={() => editor.chain().focus().toggleCode().run()} title={$t('editor.editor.inlineCode')}>{'</>'}</button>
      <span class="sep"></span>
      <button class:active={is('bulletList')} onclick={() => editor.chain().focus().toggleBulletList().run()} title={$t('editor.editor.bulletList')}>• {$t('editor.editor.list')}</button>
      <button class:active={is('orderedList')} onclick={() => editor.chain().focus().toggleOrderedList().run()} title={$t('editor.editor.numberedList')}>1. {$t('editor.editor.list')}</button>
      <button class:active={is('taskList')} onclick={() => editor.chain().focus().toggleTaskList().run()} title={$t('editor.editor.todoList')}><Icon name="check-square" size={13} /> {$t('editor.editor.todo')}</button>
      <span class="sep"></span>
      <button class:active={is('blockquote')} onclick={() => editor.chain().focus().toggleBlockquote().run()} title={$t('editor.editor.quote')}><Icon name="text-quote" size={14} /></button>
      <button class:active={is('codeBlock')} onclick={() => editor.chain().focus().toggleCodeBlock().run()} title={$t('editor.editor.codeBlock')}>{'{ }'}</button>
      <span class="sep"></span>
      <button class:active={is('link')} onclick={addLink} title={$t('editor.editor.link')}><Icon name="link" size={14} /></button>
      <button onclick={addImage} title={$t('editor.editor.uploadImage')}><Icon name="image" size={14} /></button>
      <button onclick={addImageUrl} title={$t('editor.editor.imageFromUrl')}>🌐</button>
      <span class="sep"></span>

      <!-- Text color -->
      <div class="menu-wrap">
        <button class:active={openMenu === 'color' || is('textStyle')} onclick={() => toggleMenu('color')} title={$t('editor.editor.textColor')}>
          <span class="swatch-ico" style="color:{editor?.getAttributes('textStyle')?.color ?? 'var(--text)'}">A</span>
        </button>
        {#if openMenu === 'color'}
          <div class="popover" role="menu">
            <div class="swatches">
              {#each TEXT_COLORS as c}
                <button class="swatch" style="background:{c}" title={c} onclick={() => setColor(c)} aria-label="color {c}"></button>
              {/each}
            </div>
            <button class="clear" onclick={clearColor}>{$t('editor.editor.defaultColor')}</button>
          </div>
        {/if}
      </div>

      <!-- Highlight -->
      <div class="menu-wrap">
        <button class:active={openMenu === 'highlight' || is('highlight')} onclick={() => toggleMenu('highlight')} title={$t('editor.editor.highlight')}>
          <span class="hl-ico"><Icon name="highlighter" size={13} /></span>
        </button>
        {#if openMenu === 'highlight'}
          <div class="popover" role="menu">
            <div class="swatches">
              {#each HIGHLIGHTS as c}
                <button class="swatch" style="background:{c}" title={c} onclick={() => setHighlight(c)} aria-label="highlight {c}"></button>
              {/each}
            </div>
            <button class="clear" onclick={clearHighlight}>{$t('editor.editor.noHighlight')}</button>
          </div>
        {/if}
      </div>

      <!-- Font size -->
      <select class="font-size" value={currentFontSize} onchange={setFontSize} title={$t('editor.editor.fontSize')}>
        <option value="">{$t('editor.editor.size')}</option>
        {#each FONT_SIZES as s}
          <option value={s}>{s.replace('px', '')}</option>
        {/each}
      </select>

      <!-- Page width toggle (right-aligned) -->
      <button
        class="width-toggle"
        class:active={layout === 'wide'}
        onclick={() => onLayoutChange(layout === 'wide' ? 'normal' : 'wide')}
        title={layout === 'wide' ? $t('editor.editor.switchToNormalWidth') : $t('editor.editor.switchToFullWidth')}
      >
        <Icon name="move-horizontal" size={12} /> {layout === 'wide' ? $t('editor.editor.wide') : $t('editor.editor.normal')}
      </button>
    </div>
  {/if}

  <div class="scroll">
    <div class="column" class:wide={layout === 'wide'}>
      {#if titleSlot}{@render titleSlot()}{/if}
      <div class="prose" bind:this={element}></div>
    </div>
  </div>

  <SlashMenu bind:state={slash} onpick={runCommand} onempty={closeSlash} />

  <PromptModal
    bind:open={prompt.open}
    title={prompt.title}
    fields={prompt.fields}
    confirmLabel={prompt.confirmLabel}
    onconfirm={prompt.onconfirm}
    oncancel={() => editor?.commands.focus()}
  />

  <Modal open={!!errorMsg} title={$t('editor.editor.somethingWentWrong')} onclose={() => (errorMsg = '')}>
    <p class="error-text">{errorMsg}</p>
    {#snippet footer()}
      <button class="error-ok" onclick={() => (errorMsg = '')}>{$t('editor.editor.ok')}</button>
    {/snippet}
  </Modal>
</div>

<style>
  .editor-wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .error-text {
    color: var(--text);
    font-size: var(--text-base);
    margin: 0;
  }
  .error-ok {
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    font-weight: var(--fw-medium);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 7px 14px;
  }
  .error-ok:hover {
    background: var(--surface-2);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-wrap: wrap;
    /* extra right padding reserves room for the absolute "Saved" status overlay */
    padding: var(--space-2) 72px var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .toolbar button {
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    padding: 4px 8px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .toolbar button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .toolbar button.active {
    color: var(--text);
    border-color: var(--border);
    background: var(--surface-2);
  }
  .toolbar .sep {
    width: 1px;
    height: 18px;
    background: var(--border);
    margin: 0 6px;
  }

  /* Color / highlight popover triggers */
  .menu-wrap {
    position: relative;
    display: inline-flex;
  }
  .swatch-ico {
    font-weight: var(--fw-medium);
    border-bottom: 2px solid currentColor;
    line-height: 1;
  }
  .hl-ico {
    font-size: var(--text-base);
  }
  .popover {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    z-index: var(--z-sticky);
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-2);
  }
  .swatches {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 6px;
  }
  .swatch {
    width: 20px;
    height: 20px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    cursor: pointer;
    padding: 0;
  }
  .swatch:hover {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }
  .clear {
    display: block;
    width: 100%;
    margin-top: var(--space-2);
    background: transparent;
    border: none;
    border-top: 1px solid var(--border);
    padding-top: 6px;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
    text-align: left;
  }
  .clear:hover {
    color: var(--text);
  }
  .font-size {
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-sm);
    padding: 4px 4px;
    cursor: pointer;
  }
  .font-size:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .font-size option {
    background: var(--surface);
    color: var(--text);
  }
  .width-toggle {
    margin-left: auto;
    white-space: nowrap;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-8) var(--space-6) 25vh;
  }
  /* Shared centered column so the title and body line up exactly. */
  .column {
    width: 100%;
    max-width: 860px;
    margin: 0 auto;
  }
  .column.wide {
    max-width: none;
  }
  .prose {
    outline: none;
  }

  /* TipTap content styling */
  /* ── Prose: a document, not an interface ──────────────────────────────────
   * Everything under `.prose .tiptap` is content the user reads and writes, so it
   * runs on an EDITORIAL scale — a 1.05rem body and 2rem headings — not the app's
   * dense 14px UI scale. Deliberately outside the --text-* tokens: shrinking this
   * to match a table would make the editor worse, not more consistent.
   * ────────────────────────────────────────────────────────────────────────── */
  :global(.prose .tiptap) {
    outline: none;
    color: var(--text);
    line-height: 1.7;
    font-size: 1.05rem;
  }
  :global(.prose .tiptap:focus) {
    outline: none;
  }
  :global(.prose .tiptap p) {
    margin: 0.5em 0;
  }
  :global(.prose .tiptap h1) {
    font-size: 2rem;
    font-weight: var(--fw-medium);
    margin: 0.8em 0 0.3em;
  }
  :global(.prose .tiptap h2) {
    font-size: 1.5rem;
    font-weight: var(--fw-medium);
    margin: 0.7em 0 0.3em;
  }
  :global(.prose .tiptap h3) {
    font-size: 1.2rem;
    font-weight: var(--fw-medium);
    margin: 0.6em 0 0.3em;
  }
  :global(.prose .tiptap ul),
  :global(.prose .tiptap ol) {
    padding-left: 1.4em;
    margin: 0.4em 0;
  }
  :global(.prose .tiptap blockquote) {
    border-left: 3px solid var(--border-control);
    padding-left: 1em;
    color: var(--muted);
    margin: 0.6em 0;
  }
  :global(.prose .tiptap code) {
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0.1em 0.35em;
    font-family: var(--mono);
    font-size: 0.9em;
  }
  :global(.prose .tiptap pre) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-4);
    overflow-x: auto;
    margin: 0.6em 0;
    font-family: var(--mono);
  }
  :global(.prose .tiptap pre code) {
    background: transparent;
    padding: 0;
  }
  :global(.prose .tiptap ul[data-type='taskList']) {
    list-style: none;
    padding-left: 0.2em;
  }
  :global(.prose .tiptap ul[data-type='taskList'] li) {
    display: flex;
    gap: 0.5em;
    align-items: flex-start;
  }
  :global(.prose .tiptap img) {
    max-width: 100%;
    border-radius: var(--radius);
  }
  /* Resizable image node-view wrapper */
  :global(.prose .tiptap .otw-img) {
    position: relative;
    display: inline-block;
    line-height: 0;
    max-width: 100%;
  }
  :global(.prose .tiptap .otw-img img) {
    display: block;
  }
  /* Selected state: themed border + visible resize handle */
  :global(.prose .tiptap .otw-img.selected img) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  :global(.prose .tiptap .otw-img-handle) {
    position: absolute;
    right: -5px;
    bottom: -5px;
    width: 12px;
    height: 12px;
    border-radius: var(--radius);
    background: var(--accent);
    border: 2px solid var(--surface);
    cursor: nwse-resize;
    display: none;
  }
  :global(.prose .tiptap .otw-img.selected .otw-img-handle) {
    display: block;
  }
  /* TipTap adds this class to the selected atom node; reinforce the border. */
  :global(.prose .tiptap .ProseMirror-selectednode .otw-img img),
  :global(.prose .tiptap img.ProseMirror-selectednode) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  :global(.prose .tiptap a) {
    color: var(--text);
    text-decoration: underline;
  }
  /* Default highlighter mark (no explicit colour) — a muted institutional wash
     rather than a saturated marker yellow. */
  :global(.prose .tiptap mark) {
    background: color-mix(in srgb, var(--accent) 30%, var(--surface-2));
    color: var(--text);
    border-radius: var(--radius);
    padding: 0 1px;
    box-decoration-break: clone;
  }
  /* Placeholder */
  :global(.prose .tiptap p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    color: var(--muted);
    float: left;
    height: 0;
    pointer-events: none;
  }
</style>
