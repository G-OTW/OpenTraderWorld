<script>
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { docsApi } from '$lib/modules/editor/api';
  import DocTree from '$lib/modules/editor/DocTree.svelte';
  import Editor from '$lib/modules/editor/Editor.svelte';
  import Database from '$lib/modules/editor/Database.svelte';
  import SubmitModal from '$lib/modules/editor/SubmitModal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import VersionMenu from '$lib/versioning/VersionMenu.svelte';
  import DatabaseVersionModal from '$lib/modules/editor/DatabaseVersionModal.svelte';
  import { docVersions } from '$lib/versioning/api.js';
  import { versioning, fmtVersionDate } from '$lib/versioning/state.svelte.js';
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

  onMount(() => {
    loadTree();
    versioning.load();
  });

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
    // A link from another module (the agent's "Save and open") names the page to open.
    const linked = $page.url.searchParams.get('doc');
    if (!selectedId && linked && docs.some((d) => d.id === linked)) {
      selectDoc(linked);
      return;
    }
    // Auto-open first page if nothing selected.
    if (!selectedId) {
      const firstPage = docs.find((d) => d.kind === 'page');
      if (firstPage) selectDoc(firstPage.id);
    }
  }

  async function selectDoc(id) {
    await flushSave();
    selectedId = id;
    infoOpen = false;
    preview = null;
    liveJson = null;
    dbPreview = null;
    current = await docsApi.get(id);
    title = current.title;
    if (current.kind !== 'database') editorRef?.setContent(current.content);
  }

  // ── Versions ──
  // A page version opens in the editor itself, read-only, under a bar that offers to
  // restore it or go back; what was on screen is kept aside and put back untouched. A
  // database version opens in a modal (its rows live outside the document).
  let versionMenu = $state(null);
  let preview = $state(null); // full page version shown read-only
  let liveJson = null; // the page as it was before the preview
  let dbPreview = $state(null); // full database version shown in the modal
  let versionError = $state('');

  async function openVersion(meta) {
    versionError = '';
    try {
      const v = await docVersions.get(current.id, meta.id);
      if (current.kind === 'database') {
        dbPreview = v;
        return;
      }
      await flushSave();
      if (!preview) liveJson = editorRef?.getJSON() ?? current.content;
      preview = v;
      editorRef?.setContent(v.content, { emitUpdate: false });
    } catch (e) {
      versionError = e.message;
    }
  }

  function closePreview() {
    if (!preview) return;
    preview = null;
    editorRef?.setContent(liveJson, { emitUpdate: false });
    liveJson = null;
  }

  async function restoreVersion(v) {
    versionError = '';
    try {
      await flushSave();
      await docVersions.restore(
        current.id,
        v.id,
        $t('versioning.restoredNote', { date: fmtVersionDate(v.created_at) })
      );
      liveJson = null;
      dbPreview = null;
      const id = current.id;
      await selectDoc(id);
      if (current.kind === 'database') dbReload++;
      docs = docs.map((d) => (d.id === id ? { ...d, title: current.title, has_versions: true } : d));
      versionMenu?.reload();
    } catch (e) {
      versionError = e.message;
    }
  }

  function onVersionCount(n) {
    const id = current?.id;
    if (docs.some((d) => d.id === id && d.has_versions !== n > 0)) {
      docs = docs.map((d) => (d.id === id ? { ...d, has_versions: n > 0 } : d));
    }
  }

  function onVersionedChange(on) {
    current = { ...current, versioned: on };
    docs = docs.map((d) => (d.id === current.id ? { ...d, versioned: on } : d));
  }

  // Remounts the database view after a restore replaced its rows.
  let dbReload = $state(0);

  // Persist a database's view config (stored in the document content).
  function onDbConfig(newContent) {
    if (preview) return;
    if (selectedId) scheduleSave({ content: newContent });
    current = { ...current, content: newContent };
  }

  // ── Autosave (debounced) ──
  // Patches merge while the timer runs: layout, title and content all land here,
  // and rescheduling must not drop what an earlier call asked to save.
  let saveTimer;
  let pendingPatch = {};
  function scheduleSave(part) {
    saveState = 'saving';
    pendingPatch = { ...pendingPatch, ...part };
    clearTimeout(saveTimer);
    const id = selectedId;
    saveTimer = setTimeout(() => runSave(id), 600);
  }

  async function runSave(id) {
    saveTimer = null;
    const patch = pendingPatch;
    pendingPatch = {};
    if (!id || !Object.keys(patch).length) return;
    try {
      await docsApi.update(id, patch);
      saveState = 'saved';
      if (current?.id === id) current = { ...current, updated_at: new Date().toISOString() };
      // Reflect title changes in the tree.
      if (patch.title !== undefined) {
        docs = docs.map((d) => (d.id === id ? { ...d, title: patch.title } : d));
      }
    } catch {
      saveState = 'error';
    }
  }

  /** Write what the debounce is still holding, now (before a snapshot, a switch, a restore). */
  async function flushSave() {
    if (!saveTimer) return;
    clearTimeout(saveTimer);
    await runSave(selectedId);
  }

  function onTitleInput(e) {
    title = e.target.value;
    if (selectedId) scheduleSave({ title });
  }

  function onContentChange(json) {
    if (selectedId && !preview) scheduleSave({ content: json });
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
  // Versions go with the document; the confirmation says so when there are any.
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);
  let pendingHasVersions = $state(false);

  function subtreeHasVersions(id) {
    const ids = new Set([id]);
    let grew = true;
    while (grew) {
      grew = false;
      for (const d of docs) {
        if (d.parent_id && ids.has(d.parent_id) && !ids.has(d.id)) {
          ids.add(d.id);
          grew = true;
        }
      }
    }
    return docs.some((d) => ids.has(d.id) && d.has_versions);
  }

  function remove(id) {
    pendingDelete = id;
    pendingHasVersions = subtreeHasVersions(id);
    confirmOpen = true;
  }

  async function confirmRemove() {
    const id = pendingDelete;
    pendingDelete = null;
    if (!id) return;
    await docsApi.remove(id);
    if (id === selectedId) {
      clearTimeout(saveTimer);
      saveTimer = null;
      pendingPatch = {};
      selectedId = null;
      current = null;
      preview = null;
      dbPreview = null;
    }
    await loadTree();
  }
  async function move(id, parentId, position) {
    await docsApi.move(id, parentId, position);
    await loadTree();
  }
  async function setFlag(id, flag) {
    docs = docs.map((d) => (d.id === id ? { ...d, flag } : d));
    await docsApi.setFlag(id, flag);
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
      onflag={setFlag}
    />
  </aside>

  <main class="workarea">
    {#if current}
      {#if preview}
        <div class="preview-bar">
          <Icon name="history" size={14} />
          <span class="pv-txt">
            {$t('versioning.preview.viewing', { date: fmtVersionDate(preview.created_at) })}
            {#if preview.note}<span class="pv-note">{preview.note}</span>{/if}
          </span>
          <button class="btn sm" onclick={closePreview}>{$t('versioning.preview.back')}</button>
          <button class="primary sm" onclick={() => restoreVersion(preview)}>
            <Icon name="rotate-ccw" size={12} /> {$t('versioning.restore')}
          </button>
        </div>
      {:else if current.kind === 'database'}
        <div class="db-save-state">{@render statusSlot()}</div>
      {/if}
      {#if versionError}<div class="ver-err"><ErrorText error={versionError} /></div>{/if}
      {#if current.kind === 'database'}
        <div class="db-title-bar">
          <input class="doc-title" value={title} oninput={onTitleInput} placeholder={$t('editor.page.untitledDatabase')} />
          {#if versioning.editor}
            <VersionMenu
              bind:this={versionMenu}
              api={docVersions}
              itemId={current.id}
              itemName={title}
              scope="doc"
              versioned={current.versioned}
              activeId={dbPreview?.id ?? null}
              beforesave={flushSave}
              onversionedchange={onVersionedChange}
              onpreview={openVersion}
              onerror={(m) => (versionError = m)}
              oncount={onVersionCount}
            />
          {/if}
        </div>
        {#key dbReload}
          <Database
            docId={current.id}
            content={current.content}
            onConfigChange={onDbConfig}
            layout={current.layout ?? 'normal'}
            {onLayoutChange}
          />
        {/key}
      {:else}
        <Editor
          bind:this={editorRef}
          content={current.content}
          onChange={onContentChange}
          layout={current.layout ?? 'normal'}
          {onLayoutChange}
          {titleSlot}
          {statusSlot}
          readonly={!!preview}
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
  message={pendingHasVersions ? $t('versioning.delete.message.doc') : $t('editor.page.confirmDelete')}
  confirmLabel={$t('editor.docTree.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmRemove}
/>

<DatabaseVersionModal
  version={dbPreview}
  onclose={() => (dbPreview = null)}
  onrestore={restoreVersion}
/>

<SubmitModal bind:open={submitOpen} doc={submitDoc} />

{#snippet statusSlot()}
  <span class="save-state" data-state={saveState}>
    {saveState === 'saving' ? $t('editor.page.saving') : saveState === 'error' ? $t('editor.page.saveFailed') : $t('editor.page.saved')}
  </span>
{/snippet}

{#snippet titleSlot()}
  <div class="title-row">
    <input class="doc-title" value={preview ? preview.title : title} oninput={onTitleInput} readonly={!!preview} placeholder={$t('editor.docTree.untitled')} />
    {#if versioning.editor && !preview}
      <VersionMenu
        bind:this={versionMenu}
        api={docVersions}
        itemId={current.id}
        itemName={title}
        scope="doc"
        versioned={current.versioned}
        beforesave={flushSave}
        onversionedchange={onVersionedChange}
        onpreview={openVersion}
        onerror={(m) => (versionError = m)}
        oncount={onVersionCount}
      />
    {/if}
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

  /* A version shown read-only in place of the page. */
  .preview-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-shrink: 0;
    padding: var(--space-2) var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    color: var(--text);
    font-size: var(--text-sm);
  }
  /* Same box for both; the primary's drop shadow made it look taller. */
  .preview-bar button {
    height: var(--control-h);
    box-shadow: none;
  }
  .pv-txt {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    overflow: hidden;
    white-space: nowrap;
  }
  .pv-note {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ver-err {
    padding: var(--space-2) var(--space-4) 0;
  }

  .workarea {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    /* Grid items default to min-width:auto, which lets wide content (a kanban
       board, a large table) stretch the track instead of scrolling inside it. */
    min-width: 0;
  }

  .title-row {
    display: flex;
    align-items: center;
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
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
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
    display: flex;
    align-items: center;
    gap: var(--space-2);
    max-width: 1100px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-6) 0;
    width: 100%;
  }

  .db-save-state {
    position: absolute;
    top: var(--space-2);
    right: var(--space-4);
    z-index: var(--z-dropdown);
  }
  .save-state {
    flex-shrink: 0;
    margin-left: var(--space-2);
    padding: 4px 0;
    border: 1px solid transparent;
    font-size: var(--text-sm);
    line-height: 1.2;
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
