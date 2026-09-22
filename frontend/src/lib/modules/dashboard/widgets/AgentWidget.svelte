<script>
  // Agent widget: a mini composer — prompt, provider/model picker, tools envelope, Send.
  // Sending creates a conversation, stores a handoff in sessionStorage and redirects to the
  // module page, which opens the conversation and streams the run (see AgentPage.runHandoff).
  import { goto } from '$app/navigation';
  import { agentApi } from '$lib/modules/agent/api.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let { item, editing } = $props();

  let agentCfg = $state(null);
  let providers = $state([]);
  let tokens = $state([]);
  let models = $state([]); // best-effort datalist; free text always works
  let loaded = $state(false);
  let err = $state('');
  let busy = $state(false);

  let text = $state('');
  let providerId = $state('');
  let model = $state('');
  let tokenId = $state('');

  const enabledProviders = $derived(providers.filter((p) => p.enabled));
  const activeProvider = $derived(providers.find((p) => p.id === providerId) ?? null);
  const dlId = `agent-widget-models-${item.id}`;

  async function load() {
    err = '';
    try {
      [providers, agentCfg] = await Promise.all([agentApi.listProviders(), agentApi.getAgent()]);
      providerId = agentCfg?.provider_id ?? '';
      model = agentCfg?.model ?? '';
      tokenId = agentCfg?.mcp_token_id ?? '';
      loaded = true;
    } catch (e) {
      err = e.message;
    }
    try {
      tokens = await agentApi.listMcpTokens();
    } catch {
      tokens = [];
    }
    loadModels();
  }
  $effect(() => {
    if (!editing && !loaded) load();
  });

  /** Best-effort live model list for the datalist (needs a keyed provider). */
  async function loadModels() {
    models = [];
    if (!providerId || !activeProvider?.has_key) return;
    try {
      models = await agentApi.listProviderModels(providerId);
    } catch {
      /* datalist stays empty — free text still works */
    }
  }

  async function send() {
    const msg = text.trim();
    if (!msg || busy) return;
    busy = true;
    err = '';
    try {
      // Persist a provider/model switch first so the run uses it (same config the module
      // page reads).
      const patch = {};
      if ((providerId || null) !== (agentCfg?.provider_id ?? null)) patch.provider_id = providerId || null;
      if (model.trim() !== (agentCfg?.model ?? '').trim()) patch.model = model.trim();
      if (Object.keys(patch).length) agentCfg = await agentApi.updateAgent(patch);

      const conv = await agentApi.createConversation();
      const want = tokenId || null;
      if (want !== (conv.mcp_token_id ?? null)) {
        await agentApi.updateConversation(conv.id, { mcp_token_id: want });
      }
      sessionStorage.setItem(
        'otw-agent-handoff',
        JSON.stringify({ conversationId: conv.id, message: msg })
      );
      goto('/agent');
    } catch (e) {
      err = e.message;
      busy = false;
    }
  }

  function onKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }
</script>

{#if editing}
  <p class="w-state">{$t('dashboard.widgets.agent.preview')}</p>
{:else}
  <div class="w-body">
    <ErrorText error={err} compact />
    <textarea
      rows="2"
      placeholder={$t('dashboard.widgets.agent.placeholder')}
      bind:value={text}
      onkeydown={onKeydown}
    ></textarea>
    <div class="controls">
      <div class="ctl-dd">
        <Dropdown
          bind:value={providerId}
          onpick={loadModels}
          title={$t('agent.set.provider')}
          ariaLabel={$t('agent.set.provider')}
          placeholder="{$t('agent.set.provider')}…"
          options={[
            { value: '', label: `${$t('agent.set.provider')}…` },
            ...enabledProviders.map((p) => ({ value: p.id, label: p.label }))
          ]}
        />
      </div>
      <input
        class="ctl model"
        list={dlId}
        title={$t('agent.set.model')}
        aria-label={$t('agent.set.model')}
        placeholder={activeProvider?.default_model || $t('agent.set.model')}
        bind:value={model}
      />
      <datalist id={dlId}>
        {#each models as m (m)}<option value={m}></option>{/each}
      </datalist>
      <div class="ctl-dd">
        <Dropdown
          bind:value={tokenId}
          title={$t('agent.pick.noTools')}
          ariaLabel={$t('agent.pick.noTools')}
          options={[
            { value: '', label: $t('agent.pick.noTools') },
            ...tokens.map((tk) => ({ value: tk.id, label: tk.name }))
          ]}
        />
      </div>
      <button class="primary send" onclick={send} disabled={busy || !text.trim()}>
        <Icon name="send" size={13} /> {$t('agent.chat.send')}
      </button>
    </div>
  </div>
{/if}

<style>
  /* The composer grows into whatever height the cell offers; the control row stays put. */
  textarea {
    flex: 1;
    min-height: 54px;
    resize: none;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  /* The pickers are Dropdowns and the model box is a text input: all three already own
     the shared control look, so the wrappers carry only the flex sizing of the row. */
  .ctl-dd,
  .ctl {
    flex: 1 1 120px;
    min-width: 0;
    max-width: 42%;
  }
  .send {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: auto;
    flex-shrink: 0;
  }
</style>
