<script>
  // The workflow editor: the step grid in the middle, block reference on the right.
  //
  // Every edit autosaves after a short pause and is undoable. The two go together: the
  // safety net for "I did not mean that" is Undo, not the absence of a save, so nothing
  // here waits for a Save button. Autosave never repoints a pinned schedule, which is the
  // one thing a save must not decide on its own: a schedule pinned to a revision keeps
  // running that revision until the user says otherwise, from the Save button.
  import { onMount, onDestroy } from 'svelte';
  import { page } from '$app/stores';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Grid from '$lib/modules/automator/Grid.svelte';
  import NodePalette from '$lib/modules/automator/NodePalette.svelte';
  import NodeModal from '$lib/modules/automator/NodeModal.svelte';
  import RunTrace from '$lib/modules/automator/RunTrace.svelte';
  import SchedulePicker from '$lib/modules/automator/SchedulePicker.svelte';
  import HelpModal from '$lib/modules/automator/HelpModal.svelte';
  import {
    automatorApi,
    ancestorsOf,
    defaultGraph,
    defaultSchedule,
    describeSchedule,
    fmtLocal,
    STATUS_TONE
  } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  const id = $derived($page.params.id);

  let workflow = $state(null);
  let graph = $state(defaultGraph());
  let schedules = $state([]);
  let tokens = $state([]);
  let catalog = $state(null);
  let loading = $state(true);
  let error = $state('');
  let dirty = $state(false);
  let saving = $state(false);
  let savedAt = $state(null);
  // Why the last autosave landed as a draft rather than as the live graph, if it did.
  let draftReason = $state('');
  // The draft is a valid graph an agent wrote: it runs, it is simply not in service yet.
  let proposal = $state(false);

  // Undo history. `baseline` is the graph as it stands; an edit pushes the previous
  // baseline onto `undoStack`, so what is undone is always the state before that edit.
  const HISTORY_MAX = 60;
  let baseline = defaultGraph();
  let undoStack = $state([]);
  let redoStack = $state([]);

  let grid = $state(null);
  let selected = $state('');
  let nodeOpen = $state(false);
  let editingNode = $state(null);

  // Test run results, keyed by node id, so the grid can show what happened where.
  let statuses = $state({});
  let trace = $state([]);
  let traceOpen = $state(false);
  let running = $state(false);

  let settingsOpen = $state(false);
  let scheduleOpen = $state(false);
  let rule = $state(null);
  let editingRule = $state(null);
  let saveAsk = $state(false);
  let helpOpen = $state(false);
  // The name is edited in place, in the title. The settings modal still carries it, and
  // both write through the same patch, so the two cannot drift.
  let nameDraft = $state('');
  let nameEl = $state(null);

  // Client-side navigation never fires `pagehide`, so leaving the editor flushes too.
  onDestroy(() => flush());

  onMount(async () => {
    await load();
    try {
      catalog = await automatorApi.catalog();
    } catch (e) {
      error = e.message;
    }
  });

  async function load() {
    loading = true;
    try {
      const r = await automatorApi.get(id);
      workflow = r.workflow;
      nameDraft = r.workflow.name;
      // A draft is either work autosave could not save for real (a block still being
      // filled in) or a graph an agent proposed. Both are what the user last needs to
      // look at, so both are what the editor opens on; `draft_valid` says which it is,
      // because only a proposal parses (autosave saves a graph that parses for real).
      graph = normalize(r.workflow.draft ?? r.workflow.graph);
      proposal = !!r.workflow.draft && !!r.draft_valid;
      draftReason = r.workflow.draft && !r.draft_valid ? $t('automator.autosave.draftHint') : '';
      schedules = r.schedules ?? [];
      tokens = r.tokens ?? [];
      dirty = false;
      resetHistory();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // A graph saved by an older revision may miss the fields the editor binds to.
  function normalize(raw) {
    const g = raw && typeof raw === 'object' ? raw : defaultGraph();
    return {
      nodes: (g.nodes ?? []).map((n) => ({
        pos: { x: 0, y: 0 },
        on_error: 'stop',
        retry: { count: 0, backoff_secs: 5 },
        timeout_secs: null,
        name: '',
        config: {},
        ...n
      })),
      edges: (g.edges ?? []).map((e) => ({ port: 'out', ...e }))
    };
  }

  const weekdayNames = $derived([
    $t('common.weekday.mon'),
    $t('common.weekday.tue'),
    $t('common.weekday.wed'),
    $t('common.weekday.thu'),
    $t('common.weekday.fri'),
    $t('common.weekday.sat'),
    $t('common.weekday.sun')
  ]);

  const sources = $derived(editingNode ? ancestorsOf(graph, editingNode.id) : []);

  function snapshot() {
    return structuredClone($state.snapshot(graph));
  }

  function resetHistory() {
    baseline = snapshot();
    undoStack = [];
    redoStack = [];
  }

  /** Every graph edit lands here: it records the step, marks the page dirty and starts the
   *  autosave countdown. Undo and redo move the graph themselves and never come through
   *  here, or undoing would push itself back onto the stack. */
  function touched() {
    undoStack = [...undoStack, baseline].slice(-HISTORY_MAX);
    redoStack = [];
    baseline = snapshot();
    dirty = true;
    scheduleAutosave();
  }

  /** Pop one graph off `from`, push the current one onto `to`. Undo and redo are the same
   *  move with the stacks swapped. */
  function restore(from, to) {
    const previous = baseline;
    baseline = from[from.length - 1];
    graph = structuredClone(baseline);
    nodeOpen = false;
    selected = '';
    dirty = true;
    scheduleAutosave();
    return { from: from.slice(0, -1), to: [...to, previous] };
  }

  function undo() {
    if (!undoStack.length) return;
    const next = restore(undoStack, redoStack);
    undoStack = next.from;
    redoStack = next.to;
  }

  function redo() {
    if (!redoStack.length) return;
    const next = restore(redoStack, undoStack);
    redoStack = next.from;
    undoStack = next.to;
  }

  function shortcut(ev) {
    if (!(ev.metaKey || ev.ctrlKey) || ev.key.toLowerCase() !== 'z') return;
    if (['INPUT', 'TEXTAREA', 'SELECT'].includes(ev.target?.tagName)) return;
    ev.preventDefault();
    if (ev.shiftKey) redo();
    else undo();
  }

  function openNode(node) {
    editingNode = node;
    nodeOpen = true;
  }

  // The block editor owns the block's settings, never its place in the grid: a task opened
  // from the palette carries the position it had before it was dropped, and writing that
  // back would move it into the first step.
  function applyNode(next) {
    graph.nodes = graph.nodes.map((n) => (n.id === next.id ? { ...next, pos: { ...n.pos } } : n));
    touched();
  }

  function addBlock(kind, endpoint = null) {
    const patch = endpoint ? { method: endpoint.method, path: endpoint.path } : null;
    const node = grid?.addNode(kind, null, patch);
    if (node) openNode(node);
  }

  async function save(repoint = false, auto = false) {
    saveAsk = false;
    // A save already in flight: let it land, then come back for what changed since.
    if (saving) {
      scheduleAutosave();
      return;
    }
    clearTimeout(autoTimer);
    saving = true;
    if (!auto) error = '';
    const sent = $state.snapshot(graph);
    try {
      const r = await automatorApi.saveGraph(id, sent, {
        repointSchedules: repoint,
        note: auto ? 'auto' : '',
        draft: auto
      });
      workflow = r.workflow;
      savedAt = Date.now();
      // A block still being filled in is not a runnable graph: autosave parks it as a
      // draft and says why, and the schedules keep running the last valid version.
      draftReason = r.draft ? r.reason : '';
      // A real save is the adoption: the graph is now the one that runs.
      if (!r.draft) proposal = false;
      // An edit made while the request was in flight keeps the page dirty, and the
      // countdown it started still stands.
      if (JSON.stringify(sent) === JSON.stringify($state.snapshot(graph))) dirty = false;
      else scheduleAutosave();
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Autosave. It writes the graph and nothing else: repointing a pinned schedule stays a
  // decision the user makes from the Save button.
  const AUTOSAVE_MS = 1200;
  let autoTimer = null;

  function scheduleAutosave() {
    clearTimeout(autoTimer);
    autoTimer = setTimeout(() => save(false, true), AUTOSAVE_MS);
  }

  /** Leaving inside the countdown would lose the last edit; `keepalive` lets the request
   *  outlive the document. */
  function flush() {
    clearTimeout(autoTimer);
    if (!dirty) return;
    try {
      fetch(`/api/automator/workflows/${id}`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          graph: $state.snapshot(graph),
          note: 'auto',
          repoint_schedules: false,
          draft: true
        }),
        keepalive: true
      });
    } catch {
      /* nothing more we can do on the way out */
    }
  }

  // Pinned schedules are the only thing left for the Save button to decide: the graph is
  // already saved, the question is whether those schedules follow it.
  const pinnedStale = $derived(
    schedules.some((s) => s.version_id && s.version_id !== workflow?.version_id)
  );

  function askSave() {
    if (schedules.some((s) => s.version_id)) saveAsk = true;
    else save(false);
  }

  /** A run and a reload both read the stored graph, so pending edits go first. */
  async function settle() {
    if (dirty && !saving) await save(false, true);
  }

  async function testRun() {
    await settle();
    running = true;
    error = '';
    statuses = {};
    try {
      const r = await automatorApi.test(id, { send_notifications: false, call_external: true });
      trace = r.steps ?? [];
      traceOpen = true;
      statuses = Object.fromEntries(trace.map((s) => [s.node_id, s.status]));
    } catch (e) {
      error = e.message;
    } finally {
      running = false;
    }
  }

  async function runLive() {
    await settle();
    running = true;
    error = '';
    try {
      await automatorApi.run(id);
      await load();
    } catch (e) {
      error = e.message;
    } finally {
      running = false;
    }
  }

  /** Commit the inline rename. An empty name is not a name: the old one comes back. */
  async function renameFromTitle() {
    const next = nameDraft.trim();
    if (!next || next === workflow?.name) {
      nameDraft = workflow?.name ?? '';
      return;
    }
    try {
      workflow = await automatorApi.patch(id, { name: next });
    } catch (e) {
      error = e.message;
    }
    nameDraft = workflow.name;
  }

  async function saveSettings() {
    try {
      workflow = await automatorApi.patch(id, {
        name: workflow.name,
        description: workflow.description,
        enabled: workflow.enabled,
        max_runtime_secs: Number(workflow.max_runtime_secs),
        mcp_token_id: workflow.mcp_token_id || null
      });
      nameDraft = workflow.name;
      settingsOpen = false;
    } catch (e) {
      error = e.message;
    }
  }

  function openSchedule(existing = null) {
    editingRule = existing;
    rule = existing
      ? { ...existing }
      : defaultSchedule(id);
    scheduleOpen = true;
  }

  async function saveSchedule() {
    await settle();
    try {
      if (editingRule) await automatorApi.updateSchedule(editingRule.id, rule);
      else await automatorApi.createSchedule(rule);
      scheduleOpen = false;
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  async function removeSchedule(s) {
    await settle();
    try {
      await automatorApi.deleteSchedule(s.id);
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  const fmtTime = (ms) =>
    new Date(ms).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });

  const tokenOptions = $derived([
    { value: '', label: $t('automator.settings.noToken') },
    ...tokens.map((tk) => ({ value: tk.id, label: tk.name }))
  ]);
</script>

<svelte:window onkeydown={shortcut} onpagehide={flush} />

<div class="editor">
  <header class="head">
    <div class="left">
      <a class="back" href="/automator">
        <Icon name="arrow-left" size={12} />
        {$t('automator.backToList')}
      </a>
      <!-- The name is edited where it is read. The input grows with its own text: a
           mirror span sets the width and the field sits on top of it, so the pencil
           always lands right after the last letter. -->
      <h1 class="nameline">
        <span class="grow">
          <span class="mirror">{nameDraft || ' '}</span>
          <input
            bind:this={nameEl}
            class="name"
            bind:value={nameDraft}
            onblur={renameFromTitle}
            onkeydown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur();
              if (e.key === 'Escape') {
                nameDraft = workflow.name;
                e.currentTarget.blur();
              }
            }}
            maxlength="80"
            size="1"
            aria-label={$t('automator.name')}
          />
        </span>
        <button
          class="pencil"
          onclick={() => nameEl?.select()}
          title={$t('automator.renameTitle')}
          aria-label={$t('automator.renameTitle')}
        >
          <Icon name="pencil" size={13} />
        </button>
      </h1>
      {#if workflow?.description}<p class="sub">{workflow.description}</p>{/if}
    </div>

    <div class="right">
      <div class="history">
        <button
          class="icon"
          onclick={undo}
          disabled={!undoStack.length}
          title={$t('automator.undo')}
          aria-label={$t('automator.undo')}
        >
          <Icon name="undo" size={14} />
        </button>
        <button
          class="icon"
          onclick={redo}
          disabled={!redoStack.length}
          title={$t('automator.redo')}
          aria-label={$t('automator.redo')}
        >
          <Icon name="redo" size={14} />
        </button>
      </div>
      <span class="savestate" class:pending={dirty}>
        {#if saving}
          {$t('automator.autosave.saving')}
        {:else if dirty}
          {$t('automator.autosave.pending')}
        {:else if proposal}
          {$t('automator.autosave.proposal')}
        {:else if draftReason}
          {$t('automator.autosave.draft')}
        {:else if savedAt}
          {$t('automator.autosave.savedAt', { at: fmtTime(savedAt) })}
        {:else}
          {$t('automator.saved')}
        {/if}
      </span>
      <Button icon="clock" onclick={() => openSchedule()}>{$t('automator.addSchedule')}</Button>
      <Button icon="settings" onclick={() => (settingsOpen = true)}>{$t('automator.settingsBtn')}</Button>
      <Button icon="flask" loading={running} onclick={testRun}>{$t('automator.testRun')}</Button>
      <Button icon="play" loading={running} onclick={runLive}>{$t('automator.runNow')}</Button>
      {#if proposal}
        <Button variant="primary" icon="check" loading={saving} onclick={askSave}>
          {$t('automator.autosave.adopt')}
        </Button>
      {:else if pinnedStale}
        <Button variant="primary" icon="save" loading={saving} onclick={askSave}>
          {$t('automator.repointBtn')}
        </Button>
      {/if}
      <button class="howto" onclick={() => (helpOpen = true)}>
        <Icon name="info" size={12} />
        {$t('automator.help.open')}
      </button>
    </div>
  </header>

  <ErrorText error={error} />

  {#if proposal}
    <p class="draft">
      <Icon name="alert-triangle" size={12} />
      {$t('automator.autosave.proposalWhy')}
    </p>
  {:else if draftReason}
    <p class="draft">
      <Icon name="alert-triangle" size={12} />
      {$t('automator.autosave.draftWhy', { reason: draftReason })}
    </p>
  {/if}

  {#if schedules.length}
    <ul class="rulebar">
      {#each schedules as s (s.id)}
        <li class:paused={!s.active}>
          <Icon name="clock" size={12} />
          <button class="rulebtn" onclick={() => openSchedule(s)}>
            {describeSchedule(s, $t, weekdayNames)}
          </button>
          {#if s.next_run_at && s.active}
            <span class="next">{fmtLocal(s.next_run_at)}</span>
          {/if}
          {#if s.version_id}
            <Badge tone="warn">{$t('automator.pinned')}</Badge>
          {/if}
          <button class="icon" onclick={() => removeSchedule(s)} title={$t('common.delete')}>
            <Icon name="x" size={12} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if loading}
    <Skeleton height="60vh" />
  {:else}
    <div class="work">
      <Grid
        bind:this={grid}
        bind:graph
        bind:selected
        {statuses}
        onchange={touched}
        onopen={openNode}
      />
      <NodePalette {catalog} onadd={addBlock} />
    </div>
  {/if}

  {#if traceOpen}
    <section class="trace">
      <header>
        <h3>{$t('automator.traceTitle')}</h3>
        <button class="icon" onclick={() => (traceOpen = false)}>
          <Icon name="x" size={14} />
        </button>
      </header>
      <RunTrace steps={trace} dense />
    </section>
  {/if}
</div>

<HelpModal bind:open={helpOpen} />

<NodeModal
  bind:open={nodeOpen}
  node={editingNode}
  {catalog}
  {sources}
  workflowId={id}
  onapply={applyNode}
  onremove={(nodeId) => {
    grid?.removeNode(nodeId);
    touched();
  }}
/>

<Modal bind:open={settingsOpen} title={$t('automator.settings.title')} size="md">
  {#if workflow}
    <div class="form">
      <Input label={$t('automator.name')} bind:value={workflow.name} maxlength="80" />
      <Input label={$t('automator.description')} bind:value={workflow.description} maxlength="240" />
      <Select
        label={$t('automator.settings.token')}
        options={tokenOptions}
        bind:value={workflow.mcp_token_id}
      />
      <p class="hint">{$t('automator.settings.tokenHint')}</p>
      <Input
        label={$t('automator.settings.maxRuntime')}
        type="number"
        min="10"
        max="3600"
        bind:value={workflow.max_runtime_secs}
      />
      <label class="check">
        <input type="checkbox" bind:checked={workflow.enabled} />
        <span>
          {$t('automator.settings.enabled')}
          <em>{$t('automator.settings.enabledHint')}</em>
        </span>
      </label>
    </div>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (settingsOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={saveSettings}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<Modal
  bind:open={scheduleOpen}
  title={editingRule ? $t('automator.sched.edit') : $t('automator.addSchedule')}
  size="md"
>
  {#if rule}
    <SchedulePicker bind:rule />
    <label class="check top">
      <input type="checkbox" bind:checked={rule.active} />
      {$t('automator.sched.active')}
    </label>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (scheduleOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={saveSchedule}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<Modal bind:open={saveAsk} title={$t('automator.repointTitle')} size="sm">
  <p>{$t('automator.repointBody')}</p>
  {#snippet footer()}
    <Button onclick={() => save(false)}>{$t('automator.repointKeep')}</Button>
    <Button variant="primary" onclick={() => save(true)}>{$t('automator.repointApply')}</Button>
  {/snippet}
</Modal>

<style>
  .editor {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-4) var(--space-4) 0;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  .left {
    min-width: 0;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--dim);
    text-decoration: none;
  }
  .back:hover {
    color: var(--text);
  }
  .nameline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-1) 0 0;
    color: var(--text);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
    line-height: var(--lh-tight);
  }
  /* Mirror and field share one cell, so the field is exactly as wide as its text. */
  .grow {
    display: inline-grid;
    max-width: 46ch;
  }
  .grow > * {
    grid-area: 1 / 1;
    font: inherit;
    letter-spacing: inherit;
  }
  .mirror {
    visibility: hidden;
    white-space: pre;
    padding: 0 1px;
  }
  .name {
    /* The column is sized by the mirror; without this the input's own intrinsic width
       (its default `size`) would win and leave the pencil stranded far to the right. */
    width: 100%;
    min-width: 0;
    height: auto;
    padding: 0 1px;
    color: inherit;
    background: none;
    border: 0;
    border-bottom: 1px solid transparent;
  }
  .name:hover {
    border-bottom-color: var(--border);
  }
  .name:focus {
    outline: none;
    border-bottom-color: var(--accent);
  }
  .pencil {
    display: inline-flex;
    background: none;
    border: 0;
    padding: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .pencil:hover {
    color: var(--accent);
  }
  .sub {
    margin: var(--space-1) 0 0;
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
  }
  .savestate {
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
    margin-right: var(--space-1);
  }
  .savestate.pending {
    color: var(--amber);
  }
  .history {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-right: var(--space-2);
    border-right: 0.5px solid var(--border);
  }
  .howto {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: var(--space-3);
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent);
    font-size: var(--text-xs);
    cursor: pointer;
    white-space: nowrap;
  }
  .howto:hover {
    text-decoration: underline;
  }
  /* A note, not a banner: it says the graph is parked, which is not an alarm. */
  .draft {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    width: fit-content;
    max-width: 100%;
    margin: 0 0 var(--space-3);
    padding: 2px var(--space-2);
    border: 0.5px solid var(--border);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .draft :global(svg) {
    color: var(--amber);
    flex: none;
  }
  .work {
    flex: 1;
    display: flex;
    min-height: 0;
    border: 0.5px solid var(--border);
  }
  .rulebar {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0 0 var(--space-2);
    padding: 0;
  }
  .rulebar li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border: 0.5px solid var(--border);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .rulebar li.paused {
    opacity: 0.5;
  }
  .rulebtn {
    background: none;
    border: 0;
    color: inherit;
    font-size: inherit;
    cursor: pointer;
    padding: 0;
  }
  .next {
    font-family: var(--mono);
    color: var(--muted);
  }
  .icon {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    display: inline-flex;
  }
  .icon:hover {
    color: var(--text);
  }
  .icon:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .icon:disabled:hover {
    color: var(--muted);
  }
  .trace {
    max-height: 40vh;
    overflow-y: auto;
    border-top: 0.5px solid var(--border);
    padding: var(--space-3) 0;
  }
  .trace header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
  }
  .trace h3 {
    margin: 0;
    font-size: var(--text-sm);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    font-size: var(--text-sm);
  }
  .check.top {
    margin-top: var(--space-3);
  }
  .check em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
