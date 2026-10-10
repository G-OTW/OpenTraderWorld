<script>
  // Dump a conversation into a new Editor page: pick a folder (or create one), name the
  // page, optionally include tool calls and token counts. "Save" stays on the agent,
  // "Save and open" jumps to the page.
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { fmtDateTime } from '$lib/format.js';
  import { docsApi } from '$lib/modules/editor/api.js';
  import { agentApi } from './api.js';
  import { threadToDoc } from './dump.js';

  // props: open (bindable), conversation ({ id, title }), personas ([{ agent }])
  let { open = $bindable(false), conversation = null, personas = [] } = $props();

  let docs = $state([]);
  let folderId = $state(''); // '' = root
  let name = $state('');
  let details = $state(false);
  let newFolder = $state(null); // null = closed, string = the name being typed
  let busy = $state(false);
  let error = $state('');

  // Folders in tree order, indented by depth.
  const folders = $derived.by(() => {
    const kids = new Map();
    for (const d of docs) {
      if (d.kind !== 'folder') continue;
      const k = d.parent_id ?? '';
      if (!kids.has(k)) kids.set(k, []);
      kids.get(k).push(d);
    }
    const out = [];
    const walk = (parent, depth) => {
      for (const f of (kids.get(parent) ?? []).sort((a, b) => a.position - b.position)) {
        out.push({ id: f.id, label: `${'  '.repeat(depth)}${f.title || $t('editor.docTree.untitled')}` });
        walk(f.id, depth + 1);
      }
    };
    walk('', 0);
    return out;
  });

  $effect(() => {
    if (!open) return;
    untrack(() => {
      name = conversation?.title || $t('agent.chat.newChat');
      details = false;
      newFolder = null;
      error = '';
    });
    docsApi
      .list()
      .then((d) => (docs = d))
      .catch((e) => (error = e.message));
  });

  async function createFolder() {
    const title = newFolder?.trim();
    if (!title) return;
    error = '';
    try {
      const f = await docsApi.create(folderId || null, 'folder', title);
      docs = [...docs, f];
      folderId = f.id;
      newFolder = null;
    } catch (e) {
      error = e.message;
    }
  }

  async function save(andOpen) {
    const title = name.trim();
    if (!title || !conversation || busy) return;
    busy = true;
    error = '';
    try {
      const data = await agentApi.getConversation(conversation.id);
      const content = threadToDoc({
        messages: data.messages ?? [],
        personas,
        details,
        fmtDate: fmtDateTime,
        labels: {
          you: $t('agent.dump.you'),
          assistant: $t('agent.dump.assistant'),
          thinking: $t('agent.msg.thinking'),
          tool: $t('agent.dump.tool'),
          result: $t('agent.dump.result'),
          error: $t('agent.dump.error'),
          tokens: $t('agent.dump.tokens')
        }
      });
      const doc = await docsApi.create(folderId || null, 'page', title);
      await docsApi.update(doc.id, { content });
      open = false;
      if (andOpen) goto(`/editor?doc=${doc.id}`);
      else toast.ok($t('agent.dump.saved', { name: title }));
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={$t('agent.dump.title')} size="sm">
  <div class="form">
    <label class="field">
      <span>{$t('agent.dump.folder')}</span>
      <div class="row">
        <select bind:value={folderId}>
          <option value="">{$t('agent.dump.root')}</option>
          {#each folders as f (f.id)}
            <option value={f.id}>{f.label}</option>
          {/each}
        </select>
        <button
          type="button"
          class="icon"
          title={$t('agent.dump.newFolder')}
          aria-label={$t('agent.dump.newFolder')}
          onclick={() => (newFolder = newFolder === null ? '' : null)}
        >
          <Icon name="folder-plus" size={16} />
        </button>
      </div>
    </label>

    {#if newFolder !== null}
      <div class="row">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={newFolder}
          placeholder={$t('agent.dump.newFolderName')}
          autofocus
          onkeydown={(e) => e.key === 'Enter' && createFolder()}
        />
        <button type="button" class="ghost" disabled={!newFolder.trim()} onclick={createFolder}>
          {$t('agent.dump.create')}
        </button>
      </div>
    {/if}

    <label class="field">
      <span>{$t('agent.dump.fileName')}</span>
      <input type="text" bind:value={name} onkeydown={(e) => e.key === 'Enter' && save(false)} />
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={details} />
      {$t('agent.dump.details')}
    </label>

    <ErrorText {error} />
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="ghost" disabled={busy || !name.trim()} onclick={() => save(false)}>
      {$t('agent.dump.save')}
    </button>
    <button class="primary" disabled={busy || !name.trim()} onclick={() => save(true)}>
      {$t('agent.dump.saveOpen')}
    </button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .row > select,
  .row > input {
    flex: 1;
    min-width: 0;
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-sm);
    color: var(--text);
  }
</style>
