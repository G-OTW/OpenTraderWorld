<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { displayValue } from './cells.js';
  import { uploadFile, pickFile } from '../files-api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let errorMsg = $state('');

  // props: columns, rows, coverColId, onCoverColChange, onAddRow, onOpenRow, onSetCover
  let {
    columns,
    rows,
    coverColId,
    onCoverColChange, // (colId | '')
    onAddRow, // ()
    onOpenRow, // (rowId)
    onSetCover // (rowId, colId, url) — store an uploaded cover URL on a row
  } = $props();

  async function uploadCover(rowId) {
    if (!coverCol) return;
    const file = await pickFile('image/*');
    if (!file) return;
    try {
      const { url } = await uploadFile(file);
      onSetCover?.(rowId, coverCol.id, url);
    } catch (e) {
      errorMsg = $t('editor.galleryView.uploadFailed', { message: e.message });
    }
  }

  // Columns usable as a cover image: url columns (image addresses).
  const urlCols = $derived(columns.filter((c) => c.type === 'url'));
  const coverCol = $derived(columns.find((c) => c.id === coverColId) ?? null);

  const titleCol = $derived(columns.find((c) => c.type === 'text') ?? columns[0] ?? null);
  const detailCols = $derived(columns.filter((c) => c.id !== titleCol?.id && c.id !== coverCol?.id));

  function cover(row) {
    return coverCol ? row.cells?.[coverCol.id] : null;
  }
</script>

<div class="gallery-bar">
  <label>
    {$t('editor.galleryView.coverImageLabel')}
    <select value={coverCol?.id ?? ''} onchange={(e) => onCoverColChange(e.target.value)}>
      <option value="">{$t('editor.galleryView.none')}</option>
      {#each urlCols as c}
        <option value={c.id}>{c.name || $t('editor.docTree.untitled')}</option>
      {/each}
    </select>
  </label>
  {#if urlCols.length === 0}
    <span class="hint">{$t('editor.galleryView.addUrlColumnHint')}</span>
  {/if}
</div>

<div class="grid">
  {#each rows as row (row.id)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="card" onclick={() => onOpenRow?.(row.id)} role="button" tabindex="0">
      <div class="cover">
        {#if cover(row)}
          <img src={cover(row)} alt="" loading="lazy" />
          <button
            class="cover-replace"
            title={$t('editor.galleryView.replaceImage')}
            onclick={(e) => { e.stopPropagation(); uploadCover(row.id); }}
          >{$t('editor.galleryView.replace')}</button>
        {:else if coverCol}
          <button
            class="cover-upload"
            onclick={(e) => { e.stopPropagation(); uploadCover(row.id); }}
          ><Icon name="upload" size={13} /> {$t('editor.galleryView.uploadImage')}</button>
        {:else}
          <div class="cover-empty">{$t('editor.galleryView.noImage')}</div>
        {/if}
      </div>
      <div class="body">
        <div class="card-title">
          {titleCol ? displayValue(titleCol, row.cells?.[titleCol.id]) || $t('editor.docTree.untitled') : $t('editor.docTree.untitled')}
        </div>
        {#each detailCols as col}
          {@const txt = displayValue(col, row.cells?.[col.id])}
          {#if txt}<div class="card-field"><span class="k">{col.name}</span> {txt}</div>{/if}
        {/each}
      </div>
    </div>
  {/each}

  <button class="add-card" onclick={onAddRow}><Icon name="plus" size={13} /> {$t('editor.galleryView.newItem')}</button>
</div>

<Modal open={!!errorMsg} title={$t('editor.galleryView.uploadFailedTitle')} onclose={() => (errorMsg = '')}>
  <p class="err" title="click to copy" use:copyLog={errorMsg}>{errorMsg}</p>
  {#snippet footer()}
    <button class="err-ok" onclick={() => (errorMsg = '')}>{$t('editor.editor.ok')}</button>
  {/snippet}
</Modal>

<style>
  .err {
    color: var(--text);
    font-size: 0.9rem;
    margin: 0;
  }
  .err-ok {
    background: var(--accent);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    cursor: pointer;
    font-size: 0.85rem;
    padding: 7px 14px;
  }
  .gallery-bar {
    margin-bottom: 12px;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .gallery-bar select {
    margin-left: 6px;
  }
  .gallery-bar .hint {
    margin-left: 10px;
    color: var(--amber);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    cursor: pointer;
  }
  .card:hover {
    border-color: var(--accent);
  }
  .cover {
    position: relative;
    aspect-ratio: 16 / 10;
    background: var(--surface-2);
  }
  .cover-replace {
    position: absolute;
    top: 6px;
    right: 6px;
    background: rgba(0, 0, 0, 0.6);
    border: none;
    border-radius: 4px;
    color: #fff;
    font-size: 0.72rem;
    padding: 3px 7px;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.12s;
  }
  .card:hover .cover-replace {
    opacity: 1;
  }
  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .cover-empty,
  .cover-upload {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .cover-upload {
    background: transparent;
    border: none;
    cursor: pointer;
  }
  .cover-upload:hover {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .body {
    padding: 10px 12px;
  }
  .card-title {
    color: var(--text);
    font-size: 0.9rem;
    font-weight: 600;
    margin-bottom: 4px;
  }
  .card-field {
    color: var(--muted);
    font-size: 0.76rem;
    margin-top: 2px;
  }
  .card-field .k {
    color: var(--text);
    opacity: 0.6;
  }
  .add-card {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 120px;
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.85rem;
  }
  .add-card:hover {
    color: var(--text);
    border-color: var(--accent);
  }
</style>
