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
    <button type="button" class="mini" onclick={fresh}><Icon name="plus" size={12} /> {$t('histviz.script.new')}</button>
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
    <button
      type="button"
      class="mini danger"
      title={$t('histviz.script.delete')}
      aria-label={$t('histviz.script.delete')}
      disabled={!draft.id || saving}
      onclick={() => (confirmDelete = true)}
    >
      <Icon name="trash-2" size={14} />
    </button>
  </div>

  <label class="field">
    <span>{$t('histviz.script.name')}</span>
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

  <div class="lib-row">
    <button type="button" class="mini" onclick={save} disabled={saving}>
      <Icon name="save" size={12} /> {$t('histviz.script.saveToLib')}
    </button>
    {#if note}<span class="note" class:err={noteErr}>{note}</span>{/if}
  </div>

  {#if compiled?.ok}
    <ScriptInputs inputs={compiled.meta.inputs} bind:values={draft.inputs} />
  {/if}
</div>

<div class="sec" class:gone={section !== 'style'}>
  <div class="field">
    <span>{$t('histviz.custom.pane')}</span>
    <div class="seg">
      <button type="button" class:on={draft.pane === 'overlay'} onclick={() => ((draft.pane = 'overlay'), (draft.paneSet = true))}>
        {$t('histviz.custom.paneOverlay')}
      </button>
      <button type="button" class:on={draft.pane !== 'overlay'} onclick={() => ((draft.pane = 'pane'), (draft.paneSet = true))}>
        {$t('histviz.custom.paneSeparate')}
      </button>
    </div>
  </div>
  {#if plots.length}
    <div class="field">
      <span>{$t('histviz.script.plots')}</span>
      {#each plots as p (p.index)}
        <label class="plot">
          <input type="checkbox" checked={!(draft.hidden ?? []).includes(p.index)} onchange={() => togglePlot(p.index)} />
          {p.title}
        </label>
      {/each}
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
  .gone {
    display: none;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .field > span {
    font-size: var(--text-sm);
    color: var(--muted);
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
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
    max-height: 240px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .row {
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
  }
  .row:last-child {
    border-bottom: none;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .empty {
    padding: var(--space-3);
  }
  .plot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-2);
  }
  /* Same height as the script picker next to it. */
  .lib-row .mini {
    height: var(--control-h);
    padding: 0 var(--space-3);
  }
  .mini:hover:not(:disabled) {
    border-color: var(--border-control);
  }
  .mini:disabled,
  .act:disabled {
    opacity: 0.4;
    cursor: not-allowed;
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
    font-size: var(--text-sm);
  }
  .act:hover:not(:disabled) {
    color: var(--text);
  }
  .seg {
    display: flex;
  }
  .seg button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
  }
  .seg button + button {
    border-left: none;
  }
  .seg button.on {
    color: var(--text);
    border-color: var(--border-control);
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
    font-size: var(--text-xs);
    color: var(--muted);
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
  .mini.danger:hover:not(:disabled) {
    color: var(--red);
    border-color: var(--red);
  }
</style>
