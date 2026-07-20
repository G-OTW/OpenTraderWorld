<script>
  // Agent settings — provider CRUD (keys write-only, sealed at rest) + the single default
  // agent's config (system prompt, active provider/model, params). Rendered inside a Modal
  // from the chat page. No vendor is pre-filled: the empty state asks the user to add a
  // provider of their choice.
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import MemoryManager from '$lib/modules/agent/MemoryManager.svelte';
  import SkillsManager from '$lib/modules/agent/SkillsManager.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import { agentApi } from '$lib/modules/agent/api.js';

  // props: onsaved (called after any change so the parent can refresh readiness)
  let { onsaved = () => {} } = $props();

  let tab = $state('general'); // 'general' | 'memory' | 'skills'

  let providers = $state([]);
  let agent = $state(null);
  let mcpTokens = $state([]);
  let mcpEnabled = $state(true); // global NETWORK toggle — informational: the agent runs in-process
  let loading = $state(true);
  let error = $state('');

  // Provider editor state (null = closed; {} = new; row = editing).
  let editing = $state(null);
  let form = $state({ kind: 'openai_compat', label: '', base_url: '', api_key: '', api_key_vault_item: null, default_model: '', enabled: true });
  let formErr = $state('');
  let saving = $state(false);

  // Modal confirm (replaces native confirm()).
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  // Extra request params (everything except max_tokens/temperature), edited as JSON.
  let advanced = $state('');

  const KIND_OPTS = $derived([
    { value: 'openai_compat', label: $t('agent.set.kindOpenai') },
    { value: 'anthropic', label: $t('agent.set.kindAnthropic') }
  ]);

  onMount(reload);

  /** The two bound param fields must never be undefined: binding undefined into a
   *  $bindable prop with a fallback is a hard Svelte error (props_invalid_value) that
   *  crashes the pane mid-render. A fresh agent row has params = {}. */
  function normalizeAgent(a) {
    a.params = a.params ?? {};
    if (a.params.max_tokens == null) a.params.max_tokens = 2048;
    if (a.params.temperature == null) a.params.temperature = '';
    return a;
  }

  async function reload() {
    loading = true;
    error = '';
    try {
      [providers, agent] = await Promise.all([agentApi.listProviders(), agentApi.getAgent()]);
      agent = normalizeAgent(agent);
      syncAdvanced();
      // Tool config is optional; don't fail the whole pane if MCP endpoints error.
      try {
        mcpTokens = await agentApi.listMcpTokens();
        const s = await agentApi.mcpSettings();
        mcpEnabled = !!s.enabled;
      } catch {
        mcpTokens = [];
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  /** Mirror agent.params (minus the two dedicated fields) into the advanced JSON editor. */
  function syncAdvanced() {
    const { max_tokens, temperature, ...rest } = agent?.params ?? {};
    advanced = Object.keys(rest).length ? JSON.stringify(rest, null, 2) : '';
  }

  function openNew() {
    editing = {};
    form = { kind: 'openai_compat', label: '', base_url: '', api_key: '', api_key_vault_item: null, default_model: '', enabled: true };
    formErr = '';
  }
  function openEdit(p) {
    editing = p;
    // api_key stays blank on edit — empty means "leave unchanged". An existing vault
    // reference is pre-selected; clearing it back to a pasted key just needs a value.
    form = {
      kind: p.kind,
      label: p.label,
      base_url: p.base_url,
      api_key: '',
      api_key_vault_item: p.api_key_vault_item ?? null,
      default_model: p.default_model,
      enabled: p.enabled
    };
    formErr = '';
  }
  function cancelForm() {
    editing = null;
    formErr = '';
  }

  /** Anthropic uses its default base; a stale compat URL would silently hit the wrong host. */
  function onKindChange(e) {
    if (e.target.value === 'anthropic') form.base_url = '';
  }

  async function saveProvider() {
    formErr = '';
    if (!form.label.trim()) {
      formErr = $t('agent.set.errLabel');
      return;
    }
    if (form.kind === 'openai_compat' && !form.base_url.trim()) {
      formErr = $t('agent.set.errBaseUrl');
      return;
    }
    saving = true;
    try {
      if (editing.id) await agentApi.updateProvider(editing.id, form);
      else await agentApi.addProvider(form);
      editing = null;
      await reload();
      onsaved();
    } catch (e) {
      formErr = e.message;
    } finally {
      saving = false;
    }
  }

  function removeProvider(p) {
    askConfirm($t('agent.set.deleteProvider', { label: p.label }), async () => {
      try {
        await agentApi.deleteProvider(p.id);
        await reload();
        onsaved();
      } catch (e) {
        error = e.message;
      }
    });
  }

  // ── Agent config ──
  let savingAgent = $state(false);
  let agentSaved = $state(false);

  const providerOpts = $derived([
    { value: '', label: $t('agent.set.providerNone') },
    ...providers.map((p) => ({ value: p.id, label: p.label }))
  ]);

  const tokenOpts = $derived([
    { value: '', label: $t('agent.set.toolsNone') },
    ...mcpTokens.map((tk) => ({
      value: tk.id,
      label: $t('agent.set.tokenModules', { name: tk.name, count: Object.keys(tk.permissions || {}).length })
    }))
  ]);

  async function saveAgent() {
    savingAgent = true;
    agentSaved = false;
    error = '';
    try {
      // Advanced JSON is the source of truth for every param except the two dedicated
      // fields — so removing a line there actually removes the key.
      let params = {};
      if (advanced.trim()) {
        let parsed;
        try {
          parsed = JSON.parse(advanced);
        } catch {
          parsed = null;
        }
        if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
          error = $t('agent.set.advancedErr');
          savingAgent = false;
          return;
        }
        params = parsed;
      }
      params.max_tokens = Number(agent.params?.max_tokens) || 2048;
      delete params.temperature;
      if (agent.params?.temperature !== '' && agent.params?.temperature != null) {
        const tv = Number(agent.params.temperature);
        if (!Number.isNaN(tv)) params.temperature = tv;
      }
      const updated = await agentApi.updateAgent({
        system_prompt: agent.system_prompt,
        provider_id: agent.provider_id || null,
        model: agent.model,
        params,
        mcp_token_id: agent.mcp_token_id || null
      });
      agent = normalizeAgent(updated);
      syncAdvanced();
      agentSaved = true;
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      savingAgent = false;
    }
  }
</script>

<div class="settings">
  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={tab === 'general'} class:active={tab === 'general'} onclick={() => (tab = 'general')}>{$t('agent.set.tabGeneral')}</button>
    <button role="tab" aria-selected={tab === 'memory'} class:active={tab === 'memory'} onclick={() => (tab = 'memory')}>{$t('agent.set.tabMemory')}</button>
    <button role="tab" aria-selected={tab === 'skills'} class:active={tab === 'skills'} onclick={() => (tab = 'skills')}>{$t('agent.set.tabSkills')}</button>
  </div>

  {#if tab === 'memory'}
    <MemoryManager />
  {:else if tab === 'skills'}
    <SkillsManager />
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <ErrorText {error} />

    <!-- Providers -->
    <section>
      <div class="sec-head">
        <h3>{$t('agent.set.providers')}</h3>
        {#if !editing}
          <Button size="sm" icon="plus" onclick={openNew}>{$t('agent.set.addProvider')}</Button>
        {/if}
      </div>

      {#if editing}
        <div class="prov-form">
          <Select label={$t('agent.set.type')} options={KIND_OPTS} bind:value={form.kind} onchange={onKindChange} />
          <Input label={$t('agent.set.label')} placeholder={$t('agent.set.labelPh')} bind:value={form.label} />
          {#if form.kind === 'openai_compat'}
            <Input
              label={$t('agent.set.baseUrl')}
              placeholder="https://openrouter.ai/api/v1"
              bind:value={form.base_url}
            />
          {/if}
          <div class="vfield">
            <span class="vlabel">{$t('agent.set.apiKey')}</span>
            <VaultPicker
              bind:vaultItemId={form.api_key_vault_item}
              hasSecret={!!editing.id && editing.has_key}
            />
          </div>
          <Input
            label={$t('agent.set.defaultModel')}
            placeholder={$t('agent.set.defaultModelPh')}
            bind:value={form.default_model}
          />
          <label class="check">
            <input type="checkbox" bind:checked={form.enabled} /> {$t('agent.set.enabled')}
          </label>
          <ErrorText error={formErr} />
          <div class="row">
            <Button variant="primary" loading={saving} onclick={saveProvider}>{$t('common.save')}</Button>
            <Button variant="ghost" onclick={cancelForm}>{$t('common.cancel')}</Button>
          </div>
        </div>
      {:else if !providers.length}
        <EmptyState
          icon="brain"
          title={$t('agent.set.noProviders')}
          description={$t('agent.set.noProvidersDesc')}
        />
      {:else}
        <ul class="prov-list">
          {#each providers as p (p.id)}
            <li>
              <div class="prov-main">
                <span class="prov-label">{p.label}</span>
                <Badge tone="neutral">{p.kind === 'anthropic' ? 'Anthropic' : 'OpenAI-compatible'}</Badge>
                {#if p.has_key}<Badge tone="success">{$t('agent.set.keySet')}</Badge>{:else}<Badge tone="warn">{$t('agent.set.noKey')}</Badge>{/if}
                {#if !p.enabled}<Badge tone="danger">{$t('agent.set.disabled')}</Badge>{/if}
              </div>
              <div class="prov-sub">
                {#if p.default_model}<code>{p.default_model}</code>{/if}
                {#if p.base_url}<span class="muted">{p.base_url}</span>{/if}
              </div>
              <div class="prov-actions">
                <button class="icon" title={$t('agent.edit')} onclick={() => openEdit(p)}><Icon name="pencil" size={14} /></button>
                <button class="icon danger" title={$t('agent.delete')} onclick={() => removeProvider(p)}><Icon name="trash" size={14} /></button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <!-- Agent config -->
    {#if agent}
      <section>
        <div class="sec-head"><h3>{$t('agent.set.assistant')}</h3></div>
        <Input label={$t('agent.set.systemPrompt')} multiline rows={4} bind:value={agent.system_prompt} />
        <div class="grid2">
          <Select label={$t('agent.set.provider')} options={providerOpts} bind:value={agent.provider_id} />
          <Input
            label={$t('agent.set.model')}
            placeholder={$t('agent.set.modelPh')}
            bind:value={agent.model}
          />
          <Input label={$t('agent.set.maxTokens')} type="number" bind:value={agent.params.max_tokens} />
          <Input
            label={$t('agent.set.temperature')}
            type="number"
            placeholder={$t('agent.set.temperaturePh')}
            bind:value={agent.params.temperature}
          />
        </div>
        <Input
          label={$t('agent.set.advanced')}
          multiline
          rows={3}
          placeholder={'{ "top_p": 0.9 }'}
          hint={$t('agent.set.advancedHint')}
          bind:value={advanced}
        />

        <!-- Tools: the agent acts on your OTW data through an MCP token. The token's own
             per-module levels (r/rw/rwd, Settings → AI agents) ARE the permissions — no
             agent-side re-validation. -->
        <div class="tools-block">
          <Select
            label={$t('agent.set.tools')}
            options={tokenOpts}
            bind:value={agent.mcp_token_id}
            hint={mcpTokens.length ? $t('agent.set.toolsHint') : $t('agent.set.toolsHintNone')}
          />
          {#if !mcpEnabled && agent.mcp_token_id}
            <p class="note">{$t('agent.set.mcpOffNote')}</p>
          {/if}
        </div>

        <div class="row">
          <Button variant="primary" loading={savingAgent} onclick={saveAgent}>{$t('agent.set.saveAssistant')}</Button>
          {#if agentSaved}<span class="ok"><Icon name="check" size={14} /> {$t('agent.set.saved')}</span>{/if}
        </div>
      </section>
    {/if}
  {/if}
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('agent.confirm.title')}
  message={confirmMessage}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={onConfirmYes}
/>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .tabs {
    display: flex;
    gap: 2px;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 7px 12px;
    font: inherit;
    font-size: var(--text-sm);
    margin-bottom: -1px;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h3 {
    margin: 0;
    font-size: 1rem;
    color: var(--text);
  }
  .prov-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    background: var(--surface-2);
  }
  .vfield {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .vlabel {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .prov-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .prov-list li {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: 'main actions' 'sub actions';
    gap: 2px var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
  }
  .prov-main {
    grid-area: main;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .prov-label {
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .prov-sub {
    grid-area: sub;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    flex-wrap: wrap;
  }
  .prov-sub code {
    font-family: var(--mono);
    color: var(--text);
  }
  .prov-actions {
    grid-area: actions;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
  }
  .icon:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2) var(--space-3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .tools-block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    border-left: 3px solid var(--border);
    padding-left: var(--space-2);
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--green);
    font-size: var(--text-sm);
  }
  .muted {
    color: var(--muted);
  }
  @media (max-width: 560px) {
    .grid2 {
      grid-template-columns: 1fr;
    }
  }
</style>
