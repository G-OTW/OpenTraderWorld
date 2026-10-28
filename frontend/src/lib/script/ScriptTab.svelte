<script>
  // The indicator sheet's script tab: it opens on the saved scripts (search + New); picking one
  // or New opens the editor, with live diagnostics, save it to the shared library, set its inputs (Inputs) and its pane and plot
  // visibility (Style). `draft` is what the chart instance stores; `compiled` is the latest
  // compile result in the browser, so the sheet refuses to add a script that does not compile.
  // The editor (CodeMirror + its language) loads only when this tab opens.
  import Icon from '$lib/ui/Icon.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { t } from '$lib/i18n';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { diagText } from './diag.js';
  import ScriptInputs from './ScriptInputs.svelte';

  let {
    // { id, name, source, inputs, hidden, pane }; `paneSet` = the pane was chosen (or comes
    // from an instance being edited), so a compile no longer moves it to the script's default.
    draft = $bindable(),
    compiled = $bindable(null),
    // false = the library list, true = the editor (an instance being edited opens on it).
    editing = $bindable(false),
    section = 'inputs'
  } = $props();

  const tr = (k, p) => $t(k, p);
  const describe = (d) => diagText(tr, d);
  const editor = import('./ScriptEditor.svelte');

  let library = $state([]);
  let libError = $state('');
  let note = $state('');
  let noteErr = $state(false);
  let serverDiags = $state([]);
  let saving = $state(false);
  let confirmDelete = $state(false);
  let query = $state('');

  async function loadLibrary() {
    try {
      library = await indicatorLib.list('script');
      libError = '';
    } catch (e) {
      libError = e.message;
    }
  }
  loadLibrary();

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? library.filter((r) => r.name.toLowerCase().includes(q)) : library;
  });

  const setNote = (text, err = false) => {
    note = text;
    noteErr = err;
  };

  function pick(id) {
    const row = library.find((r) => r.id === id);
    if (!row) return;
    // Another script: its inputs and plots are not this one's.
    draft = { ...draft, id: row.id, name: row.name, source: row.source ?? '', inputs: {}, hidden: [], paneSet: false };
    compiled = null;
    serverDiags = [];
    setNote('');
    editing = true;
  }

  function fresh() {
    draft = { id: null, name: '', source: '', inputs: {}, hidden: [], pane: 'pane', paneSet: false };
    compiled = null;
    serverDiags = [];
    setNote('');
    editing = true;
  }

  function oncompile(r) {
    compiled = r;
    if (r.ok && !draft.paneSet) draft.pane = r.meta.overlay ? 'overlay' : 'pane';
  }

  async function save() {
    const name = (draft.name ?? '').trim();
    if (!name) return setNote($t('histviz.script.nameRequired'), true);
    if (!draft.source?.trim()) return setNote($t('histviz.script.empty'), true);
    saving = true;
    serverDiags = [];
    try {
      const body = { kind: 'script', source: draft.source };
      if (draft.id) await indicatorLib.update(draft.id, name, '', body);
      else draft.id = (await indicatorLib.create(name, '', body)).id;
      await loadLibrary();
      setNote($t('histviz.script.saved'));
    } catch (e) {
      serverDiags = e.details?.diagnostics ?? [];
      setNote(serverDiags.length ? $t('histviz.script.compileErrors') : e.message, true);
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!draft.id) return;
    saving = true;
    try {
      await indicatorLib.remove(draft.id);
      draft.id = null;
      await loadLibrary();
      setNote('');
      editing = false;
    } catch (e) {
      setNote(e.message, true);
    } finally {
      saving = false;
    }
  }

  const errors = $derived(compiled && !compiled.ok ? (compiled.diagnostics ?? []) : []);
  const plots = $derived(compiled?.ok ? compiled.meta.plots : []);
  function togglePlot(index) {
    const h = new Set(draft.hidden ?? []);
    if (h.has(index)) h.delete(index);
    else h.add(index);
    draft.hidden = [...h].sort((a, b) => a - b);
  }
</script>

{#if !editing}
<div class="sec">
  <div class="lib-row">
    <input class="search" type="search" bind:value={query} placeholder={$t('histviz.script.search')} aria-label={$t('histviz.script.search')} />
    <button type="button" class="btn" onclick={fresh}><Icon name="plus" size={13} /> {$t('histviz.script.new')}</button>
  </div>
  {#if libError}<p class="err">{libError}</p>{/if}
  <div class="list">
    {#each shown as r (r.id)}
      <button type="button" class="row" onclick={() => pick(r.id)}>{r.name}</button>
    {:else}
      <p class="hint empty">{query.trim() ? $t('histviz.picker.noMatches') : $t('histviz.script.noScripts')}</p>
    {/each}
  </div>
</div>
{:else}
<!-- Both sections stay mounted: the editor keeps its state and its last compile. -->
<div class="sec" class:gone={section !== 'inputs'}>
  <div class="lib-row">
    <button type="button" class="act" onclick={() => (editing = false)}>
      <Icon name="arrow-left" size={14} /> {$t('histviz.script.back')}
    </button>
    <span class="spacer"></span>
    {#if note}<span class="note" class:err={noteErr}>{note}</span>{/if}
    <button type="button" class="btn" onclick={save} disabled={saving}>
      <Icon name="save" size={13} /> {$t('histviz.script.saveToLib')}
    </button>
    <button
      type="button"
      class="ib danger"
      title={$t('histviz.script.delete')}
      aria-label={$t('histviz.script.delete')}
      disabled={!draft.id || saving}
      onclick={() => (confirmDelete = true)}
    >
      <Icon name="trash-2" size={14} />
    </button>
  </div>

  <label class="field">
    <span class="lbl">{$t('histviz.script.name')}</span>
    <input bind:value={draft.name} placeholder={$t('histviz.script.namePlaceholder')} />
  </label>

  <small class="hint">{$t('histviz.script.hint')}</small>
  {#await editor}
    <p class="hint">{$t('histviz.script.loading')}</p>
  {:then m}
    {@const Editor = m.default}
    <Editor bind:value={draft.source} {describe} {oncompile} />
  {:catch e}
    <p class="err">{e.message}</p>
  {/await}

  {#if errors.length || serverDiags.length}
    <ul class="diags">
      {#each errors.length ? errors : serverDiags as d, i (i)}<li>{describe(d)}</li>{/each}
    </ul>
  {/if}

  {#if compiled?.ok}
    <ScriptInputs inputs={compiled.meta.inputs} bind:values={draft.inputs} />
  {/if}
</div>

<div class="sec style" class:gone={section !== 'style'}>
  <div class="line-row">
    <div class="rl">
      <span class="lbl">{$t('histviz.modal.placement')}</span>
      <span class="hint">{$t('histviz.modal.placementHint')}</span>
    </div>
    <div class="seg" role="radiogroup" aria-label={$t('histviz.modal.placement')}>
      <button type="button" role="radio" aria-checked={draft.pane === 'overlay'} class:on={draft.pane === 'overlay'} onclick={() => ((draft.pane = 'overlay'), (draft.paneSet = true))}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"></rect><path d="M6 15l4-4 3 2 5-5"></path></svg>
        {$t('histviz.custom.paneOverlay')}
      </button>
      <button type="button" role="radio" aria-checked={draft.pane !== 'overlay'} class:on={draft.pane !== 'overlay'} onclick={() => ((draft.pane = 'pane'), (draft.paneSet = true))}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"></rect><path d="M3 13h18"></path></svg>
        {$t('histviz.custom.paneSeparate')}
      </button>
    </div>
  </div>
  {#if plots.length}
    <div class="block">
      <span class="lbl">{$t('histviz.script.plots')}</span>
      <div class="tbl">
        {#each plots as p (p.index)}
          {@const vis = !(draft.hidden ?? []).includes(p.index)}
          <div class="tr" class:off={!vis}>
            <button
              type="button"
              class="eye"
              aria-pressed={vis}
              aria-label={vis ? $t('histviz.script.hidePlot', { name: p.title }) : $t('histviz.script.showPlot', { name: p.title })}
              title={vis ? $t('histviz.script.hidePlot', { name: p.title }) : $t('histviz.script.showPlot', { name: p.title })}
              onclick={() => togglePlot(p.index)}
            >
              <Icon name={vis ? 'eye' : 'eye-off'} size={15} />
            </button>
            <span class="el">{p.title}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>
{/if}

<ConfirmModal
  bind:open={confirmDelete}
  title={$t('histviz.script.deleteTitle')}
  message={$t('histviz.script.deleteMessage', { name: draft.name || $t('histviz.script.untitled') })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={remove}
/>

<style>
  .sec {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .sec.style {
    gap: var(--space-6);
  }
  .gone {
    display: none;
  }
  .field,
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .lbl {
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .lib-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .search {
    flex: 1;
    min-width: 160px;
  }
  .spacer {
    flex: 1;
  }
  .list {
    display: flex;
    flex-direction: column;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface-2);
    max-height: 280px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .row {
    background: transparent;
    border: none;
    border-bottom: var(--hairline) solid var(--border);
    color: var(--text);
    font-size: var(--fs-body);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease);
  }
  .row:last-child {
    border-bottom: none;
  }
  .row:hover {
    background: var(--surface);
  }
  .empty {
    padding: var(--space-3);
  }
  .btn:disabled,
  .ib:disabled,
  .act:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .ib {
    width: var(--control-h);
    height: var(--control-h);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .ib.danger:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--red);
  }
  .act {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--fs-body);
  }
  .act:hover:not(:disabled) {
    color: var(--text);
  }
  .line-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .rl {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .seg {
    display: flex;
    gap: 2px;
    padding: 3px;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
  }
  .seg button {
    height: 28px;
    padding: 0 var(--space-3);
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
    white-space: nowrap;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: inset 0 0 0 var(--hairline) var(--border-control);
  }
  .tbl {
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .tr {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3) var(--space-1) var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
  }
  .tr:last-child {
    border-bottom: none;
  }
  .tr.off .el {
    color: var(--faint);
  }
  .el {
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .eye {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text);
    cursor: pointer;
  }
  .tr.off .eye {
    color: var(--faint);
  }
  .eye:hover {
    background: var(--surface);
  }
  .diags {
    margin: 0;
    padding-left: var(--space-4);
    color: var(--red);
    font-size: var(--text-sm);
  }
  .hint {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--dim);
    line-height: 1.45;
  }
  .note {
    font-size: var(--text-xs);
    color: var(--green);
  }
  .err {
    margin: 0;
    color: var(--red);
    font-size: var(--text-xs);
  }
  @media (max-width: 767px) {
    .line-row {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
