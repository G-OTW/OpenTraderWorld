<script>
  // Personas: the role-shaped agents a conversation can be opened as. A persona is a system
  // prompt plus a skill shelf — it grants no data access of its own (that stays with the
  // conversation's MCP token), so nothing here talks about permissions.
  //
  // Skills are NOT created here. There is one catalog, managed in the Skills tab; this picks
  // from it. A persona holding a skill and a persona editing it are different acts, and
  // conflating them would let two personas drift into two versions of "the same" skill.
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { agentApi } from '$lib/modules/agent/api.js';

  const ALL = '*';

  let personas = $state([]);
  let catalog = $state([]); // every skill, to pick from
  let loading = $state(true);
  let error = $state('');
  let expanded = $state(null); // id whose prompt is open
  let importing = $state(false);
  let importMsg = $state('');
  let fileInput;

  // Editor state. `editing` is the row being edited ({} for a new one).
  let editing = $state(null);
  let form = $state({ name: '', system_prompt: '', skills: [], all: false, auto: false });
  let formErr = $state('');
  let saving = $state(false);

  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let confirmDanger = $state(false);
  let onConfirmYes = $state(() => {});
  function ask(message, onyes, danger = false) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmDanger = danger;
    confirmOpen = true;
  }

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      const [agents, skills] = await Promise.all([
        agentApi.listAgents(),
        agentApi.listSkills()
      ]);
      personas = agents;
      catalog = skills.skills.map((r) => r.skill);
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function openNew() {
    editing = {};
    form = { name: '', system_prompt: '', skills: [], all: false, auto: false };
    formErr = '';
  }

  function openEdit(p) {
    const declared = Array.isArray(p.agent.skills) ? p.agent.skills : [];
    editing = p.agent;
    form = {
      name: p.agent.name,
      system_prompt: p.agent.system_prompt,
      // Ids that no longer resolve are dropped here rather than saved back: the shelf shown
      // must be the shelf that will load.
      skills: declared.filter((id) => catalog.some((s) => s.id === id)),
      all: declared.includes(ALL),
      auto: !!p.agent.auto_approve_writes
    };
    formErr = '';
  }

  function toggleSkill(id) {
    form.skills = form.skills.includes(id)
      ? form.skills.filter((s) => s !== id)
      : [...form.skills, id];
  }

  async function save() {
    formErr = '';
    if (!form.name.trim()) {
      formErr = $t('agent.persona.errName');
      return;
    }
    saving = true;
    try {
      const body = {
        name: form.name,
        system_prompt: form.system_prompt,
        skills: form.all ? [ALL] : form.skills,
        auto_approve_writes: form.auto
      };
      if (editing.id) await agentApi.updateOneAgent(editing.id, body);
      else await agentApi.createAgent(body);
      editing = null;
      await reload();
    } catch (e) {
      formErr = e.message;
    } finally {
      saving = false;
    }
  }

  async function clone(p) {
    try {
      await agentApi.cloneAgent(p.agent.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  function askReset(p) {
    ask($t('agent.persona.resetConfirm', { name: p.agent.name }), async () => {
      try {
        await agentApi.resetAgent(p.agent.id);
        await reload();
      } catch (e) {
        error = e.message;
      }
    });
  }

  function askDelete(p) {
    // Say what happens to the conversations, because the answer is "nothing" and that is not
    // what a delete button usually means.
    ask(
      $t('agent.persona.deleteConfirm', { name: p.agent.name }),
      async () => {
        try {
          await agentApi.deleteAgent(p.agent.id);
          await reload();
        } catch (e) {
          error = e.message;
        }
      },
      true
    );
  }

  async function onFile(ev) {
    const file = ev.target.files?.[0];
    ev.target.value = '';
    if (!file) return;
    importing = true;
    importMsg = '';
    error = '';
    try {
      const bundle = JSON.parse(await file.text());
      const r = await agentApi.importShelf(bundle);
      importMsg = $t('agent.persona.imported', {
        added: r.personas_added,
        skipped: r.personas_skipped,
        skills: r.skills_added
      });
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      importing = false;
    }
  }
</script>

<div class="mgr">
  <p class="muted intro">{$t('agent.persona.intro')}</p>

  {#if !editing}
    <div class="head">
      <div class="row">
        <Button size="sm" variant="ghost" icon="upload" loading={importing} onclick={() => fileInput?.click()}>
          {$t('agent.shelf.import')}
        </Button>
        <a class="btn-link" href={agentApi.shelfExportUrl()} download>
          <Icon name="download" size={13} />
          {$t('agent.persona.exportAll')}
        </a>
      </div>
      <Button size="sm" icon="plus" onclick={openNew}>{$t('agent.persona.add')}</Button>
    </div>
  {/if}
  <input bind:this={fileInput} type="file" accept="application/json,.json" hidden onchange={onFile} />
  <ErrorText {error} />
  {#if importMsg}<p class="ok">{importMsg}</p>{/if}

  {#if editing}
    <div class="form">
      <Input label={$t('agent.persona.name')} placeholder="e.g. Macro Analyst" bind:value={form.name} />
      <Input
        label={$t('agent.persona.prompt')}
        multiline
        rows={10}
        placeholder={$t('agent.persona.promptPh')}
        bind:value={form.system_prompt}
      />

      <div class="shelf">
        <span class="lbl">{$t('agent.persona.shelf')}</span>
        <p class="hint">{$t('agent.persona.shelfHint')}</p>
        <label class="check">
          <input type="checkbox" bind:checked={form.all} />
          {$t('agent.persona.allSkills')}
        </label>
        {#if !form.all}
          {#if !catalog.length}
            <p class="hint">{$t('agent.persona.noCatalog')}</p>
          {:else}
            <div class="picker">
              {#each catalog as s (s.id)}
                <label class="pick" class:off={!s.enabled}>
                  <input
                    type="checkbox"
                    checked={form.skills.includes(s.id)}
                    onchange={() => toggleSkill(s.id)}
                  />
                  <span class="pick-name">{s.name}</span>
                  {#if !s.enabled}<Badge tone="neutral">{$t('agent.skl.disabled')}</Badge>{/if}
                  {#if s.description}<span class="pick-desc">{s.description}</span>{/if}
                </label>
              {/each}
            </div>
          {/if}
        {/if}
      </div>

      <label class="check">
        <input type="checkbox" bind:checked={form.auto} />
        {$t('agent.persona.autoApprove')}
      </label>
      <p class="hint">{$t('agent.persona.autoApproveHint')}</p>

      <ErrorText error={formErr} />
      <div class="row">
        <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
        <Button variant="ghost" onclick={() => (editing = null)}>{$t('common.cancel')}</Button>
      </div>
    </div>
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !personas.length}
    <EmptyState icon="user" title={$t('agent.persona.empty')} />
  {:else}
    <ul class="list">
      {#each personas as p (p.agent.id)}
        <li>
          <div class="main">
            <div class="name-row">
              <strong>{p.agent.name}</strong>
              {#if p.agent.builtin}<Badge tone="accent">{$t('agent.persona.builtin')}</Badge>{/if}
              {#if p.agent.is_default}<Badge tone="neutral">{$t('agent.pick.default')}</Badge>{/if}
              {#if p.agent.auto_approve_writes}
                <Badge tone="warn">{$t('agent.persona.autoBadge')}</Badge>
              {/if}
            </div>
            <!-- The shelf it will actually get: its allowlist intersected with the enabled
                 catalog. A declared skill that is not seeded or is disabled is simply absent. -->
            <span class="desc">
              {#if p.effective_skills.length}
                {p.effective_skills.join(', ')}
              {:else}
                {$t('agent.persona.noSkills')}
              {/if}
            </span>
            {#if expanded === p.agent.id}
              <pre class="prompt">{p.agent.system_prompt}</pre>
            {/if}
          </div>
          <div class="actions">
            <button
              class="icon"
              title={$t('agent.persona.showPrompt')}
              onclick={() => (expanded = expanded === p.agent.id ? null : p.agent.id)}
            >
              <Icon name={expanded === p.agent.id ? 'chevron-down' : 'chevron-right'} size={14} />
            </button>
            <a
              class="icon"
              title={$t('agent.shelf.export')}
              href={agentApi.shelfExportUrl({ agent: p.agent.id })}
              download
            >
              <Icon name="download" size={14} />
            </a>
            <button class="icon" title={$t('agent.persona.clone')} onclick={() => clone(p)}>
              <Icon name="copy" size={14} />
            </button>
            <button class="icon" title={$t('agent.edit')} onclick={() => openEdit(p)}>
              <Icon name="pencil" size={14} />
            </button>
            {#if p.agent.builtin}
              <Button size="sm" variant="ghost" onclick={() => askReset(p)}>
                {$t('agent.persona.reset')}
              </Button>
            {:else if !p.agent.is_default}
              <button class="icon danger" title={$t('agent.delete')} onclick={() => askDelete(p)}>
                <Icon name="trash" size={14} />
              </button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<ConfirmModal
  bind:open={confirmOpen}
  message={confirmMessage}
  danger={confirmDanger}
  onconfirm={() => {
    confirmOpen = false;
    onConfirmYes();
  }}
/>

<style>
  .mgr {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .intro {
    margin: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .ok {
    color: var(--green);
    font-size: var(--text-sm);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-xs);
    margin: 0;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    background: var(--surface-2);
  }
  .shelf {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lbl {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .picker {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 260px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-2);
  }
  .pick {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    padding: 2px 0;
    min-width: 0;
  }
  .pick.off {
    opacity: 0.6;
  }
  .pick-name {
    color: var(--text);
    flex-shrink: 0;
  }
  .pick-desc {
    color: var(--muted);
    font-size: var(--text-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  li {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-2) var(--space-3);
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
  }
  .name-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .desc {
    color: var(--muted);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }
  .prompt {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: var(--space-2);
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    white-space: pre-wrap;
    overflow-x: auto;
    max-width: 100%;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }
  .icon {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius);
    display: inline-flex;
    align-items: center;
    text-decoration: none;
  }
  .icon:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .btn-link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: var(--text-sm);
    text-decoration: none;
  }
  .btn-link:hover {
    color: var(--text);
  }
</style>
