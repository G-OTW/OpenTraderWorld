<script>
  import { onMount } from 'svelte';
  import { docsApi } from '$lib/modules/editor/api';
  import DocTree from '$lib/modules/editor/DocTree.svelte';
  import Editor from '$lib/modules/editor/Editor.svelte';
  import Database from '$lib/modules/editor/Database.svelte';
  import SubmitModal from '$lib/modules/editor/SubmitModal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let docs = $state([]);
  let selectedId = $state(null);
  let current = $state(null); // full doc with content
  let title = $state('');
  let saveState = $state('saved'); // saved | saving | error
  let editorRef = $state(null);
  let infoOpen = $state(false); // metadata popover

  // ── Submit for publication ──
  let submitOpen = $state(false);
  let submitDoc = $state(null); // snapshot passed to the modal

  function openSubmit() {
    if (!editorRef || editorRef.isEmpty()) return;
    submitDoc = {
      title: title.trim() || $t('editor.docTree.untitled'),
      icon: current?.icon ?? null,
      layout: current?.layout ?? 'normal',
      html: editorRef.getHTML(),
      source_json: editorRef.getJSON()
    };
    submitOpen = true;
  }

  const fmtDate = (s) =>
    s
      ? new Date(s).toLocaleString(undefined, {
          dateStyle: 'medium',
          timeStyle: 'short'
        })
      : '—';

  onMount(loadTree);

  $effect(() => {
    if (!infoOpen) return;
    const onDoc = (e) => {
      if (!e.target.closest('.info-wrap')) infoOpen = false;
    };
    window.addEventListener('mousedown', onDoc);
    return () => window.removeEventListener('mousedown', onDoc);
  });

  async function loadTree() {
    docs = await docsApi.list();
    // Auto-open first page if nothing selected.
    if (!selectedId) {
      const firstPage = docs.find((d) => d.kind === 'page');
      if (firstPage) selectDoc(firstPage.id);
    }
  }

  async function selectDoc(id) {
    selectedId = id;
    infoOpen = false;
    current = await docsApi.get(id);
    title = current.title;
    if (current.kind !== 'database') editorRef?.setContent(current.content);
  }

  // Persist a database's view config (stored in the document content).
  function onDbConfig(newContent) {
    if (selectedId) scheduleSave({ content: newContent });
    current = { ...current, content: newContent };
  }

  // ── Autosave (debounced) ──
  let saveTimer;
  function scheduleSave(patch) {
    saveState = 'saving';
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try {
        await docsApi.update(selectedId, patch);
        saveState = 'saved';
        if (current) current = { ...current, updated_at: new Date().toISOString() };
        // Reflect title changes in the tree.
        if (patch.title !== undefined) {
          docs = docs.map((d) => (d.id === selectedId ? { ...d, title: patch.title } : d));
        }
      } catch {
        saveState = 'error';
      }
    }, 600);
  }

  function onTitleInput(e) {
    title = e.target.value;
    if (selectedId) scheduleSave({ title });
  }

  function onContentChange(json) {
    if (selectedId) scheduleSave({ content: json });
  }

  function onLayoutChange(layout) {
    if (!selectedId || !current) return;
    current = { ...current, layout };
    scheduleSave({ layout });
  }

  // ── Tree operations ──
  async function create(parentId, kind) {
    const titleFor = {
      folder: $t('editor.page.newFolder'),
      database: $t('editor.page.newDatabase'),
      page: $t('editor.docTree.untitled')
    };
    const doc = await docsApi.create(parentId, kind, titleFor[kind] ?? $t('editor.docTree.untitled'));
    await loadTree();
    if (kind === 'page' || kind === 'database') selectDoc(doc.id);
  }
  async function rename(id, newTitle) {
    await docsApi.update(id, { title: newTitle });
    docs = docs.map((d) => (d.id === id ? { ...d, title: newTitle } : d));
    if (id === selectedId) title = newTitle;
  }
  // Deleting a folder takes its children with it, and nothing here is undoable —
  // so it goes through ConfirmModal, which exists to replace the browser's confirm().
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function remove(id) {
    pendingDelete = id;
    confirmOpen = true;
  }

  async function confirmRemove() {
    const id = pendingDelete;
    pendingDelete = null;
    if (!id) return;
    await docsApi.remove(id);
    if (id === selectedId) {
      selectedId = null;
      current = null;
    }
    await loadTree();
  }
  async function move(id, parentId, position) {
    await docsApi.move(id, parentId, position);
    await loadTree();
  }
</script>

<div class="editor-module">
  <aside class="sidebar">
    <DocTree
      {docs}
      {selectedId}
      onselect={selectDoc}
      oncreate={create}
      onrename={rename}
      ondelete={remove}
      onmove={move}
    />
  </aside>

  <main class="workarea">
    {#if current}
      <span class="save-state" data-state={saveState}>
        {saveState === 'saving' ? $t('editor.page.saving') : saveState === 'error' ? $t('editor.page.saveFailed') : $t('editor.page.saved')}
      </span>
      {#if current.kind === 'database'}
        <div class="db-title-bar">
          <input class="doc-title" value={title} oninput={onTitleInput} placeholder={$t('editor.page.untitledDatabase')} />
        </div>
        <Database docId={current.id} content={current.content} onConfigChange={onDbConfig} />
      {:else}
        <Editor
          bind:this={editorRef}
          content={current.content}
          onChange={onContentChange}
          layout={current.layout ?? 'normal'}
          {onLayoutChange}
          {titleSlot}
        />
      {/if}
    {:else}
      <div class="placeholder">
        <EmptyState
          icon="file-text"
          title={$t('editor.page.noDocumentOpen')}
          description={$t('editor.page.selectOrCreate')}
        />
      </div>
    {/if}
  </main>
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('editor.docTree.delete')}
  message={$t('editor.page.confirmDelete')}
  confirmLabel={$t('editor.docTree.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmRemove}
/>

<SubmitModal bind:open={submitOpen} doc={submitDoc} />

{#snippet titleSlot()}
  <div class="title-row">
    <input class="doc-title" value={title} oninput={onTitleInput} placeholder={$t('editor.docTree.untitled')} />
    <button class="submit-btn" onclick={openSubmit} title={$t('editor.page.submitTitle')} aria-label={$t('editor.page.submitForPublication')}>
      <Icon name="upload" size={14} />
    </button>
    <div class="info-wrap">
      <button class="info-btn" title={$t('editor.page.pageInfo')} aria-label={$t('editor.page.pageInfo')} onclick={() => (infoOpen = !infoOpen)}>ⓘ</button>
      {#if infoOpen}
        <div class="info-pop" role="dialog">
          <div class="info-row"><span>{$t('editor.page.created')}</span><strong>{fmtDate(current?.created_at)}</strong></div>
          <div class="info-row"><span>{$t('editor.page.lastEdited')}</span><strong>{fmtDate(current?.updated_at)}</strong></div>
        </div>
      {/if}
    </div>
  </div>
{/snippet}

<style>
  .editor-module {
    display: grid;
    grid-template-columns: 260px 1fr;
    height: 100%;
    min-height: 0;
  }

  .sidebar {
    border-right: 1px solid var(--border);
    background: var(--surface);
    min-height: 0;
  }

  .workarea {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .title-row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .submit-btn {
    flex-shrink: 0;
    align-self: center;
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    padding: 4px;
    cursor: pointer;
    line-height: 1;
  }
  .submit-btn:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .info-wrap {
    position: relative;
    flex-shrink: 0;
    margin-top: 0.6em;
  }
  .info-btn {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-md);
    padding: 2px 4px;
    border-radius: var(--radius);
    line-height: 1;
  }
  .info-btn:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .info-pop {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    z-index: var(--z-sticky);
    min-width: 220px;
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-3);
  }
  .info-row {
    display: flex;
    justify-content: space-between;
    gap: var(--space-4);
    font-size: var(--text-sm);
    padding: 3px 0;
  }
  .info-row span {
    color: var(--muted);
  }
  .info-row strong {
    color: var(--text);
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }

  /* Title rendered inside the editor's content column (see Editor.svelte),
     so it aligns exactly with the body text. */
  :global(.doc-title) {
    display: block;
    width: 100%;
    background: transparent;
    border: none;
    color: var(--text);
    /* Editorial scale, like the prose it heads — not the app's --text-* UI scale.
       This is the document's own title, sitting above its body text. */
    font-size: 2.2rem;
    font-weight: var(--fw-medium);
    line-height: 1.2;
    outline: none;
    padding: 0;
    margin: 0 0 0.2em;
  }
  :global(.doc-title::placeholder) {
    color: var(--muted);
    opacity: 0.5;
  }

  .db-title-bar {
    max-width: 1100px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-6) 0;
    width: 100%;
  }

  .save-state {
    position: absolute;
    top: var(--space-3);
    right: var(--space-4);
    z-index: var(--z-dropdown);
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .save-state[data-state='error'] {
    color: var(--red);
  }

  /* Centers EmptyState in the empty work area; the copy is the component's. */
  .placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
  }
</style>
