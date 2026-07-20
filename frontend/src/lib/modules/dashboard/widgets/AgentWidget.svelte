<script>
  // Agent widget: a mini composer — prompt, provider/model picker, tools envelope, Send.
  // Sending creates a conversation, stores a handoff in sessionStorage and redirects to the
  // module page, which opens the conversation and streams the run (see AgentPage.runHandoff).
  import { goto } from '$app/navigation';
  import { agentApi } from '$lib/modules/agent/api.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';

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
  <p class="hint">{$t('dashboard.widgets.agent.preview')}</p>
{:else}
  <div class="wrap">
    <ErrorText error={err} compact />
    <textarea
      rows="2"
      placeholder={$t('dashboard.widgets.agent.placeholder')}
      bind:value={text}
      onkeydown={onKeydown}
    ></textarea>
    <div class="controls">
      <select
        class="ctl"
        title={$t('agent.set.provider')}
        aria-label={$t('agent.set.provider')}
        bind:value={providerId}
        onchange={loadModels}
      >
        <option value="">{$t('agent.set.provider')}…</option>
        {#each enabledProviders as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
      </select>
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
      <select
        class="ctl"
        title={$t('agent.pick.noTools')}
        aria-label={$t('agent.pick.noTools')}
        bind:value={tokenId}
      >
        <option value="">{$t('agent.pick.noTools')}</option>
        {#each tokens as tk (tk.id)}<option value={tk.id}>{tk.name}</option>{/each}
      </select>
      <button class="send" onclick={send} disabled={busy || !text.trim()}>
        <Icon name="send" size={13} /> {$t('agent.chat.send')}
      </button>
    </div>
  </div>
{/if}

<style>
  .hint {
    color: var(--dim);
  }
  .wrap {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  textarea {
    flex: 1;
    min-height: 54px;
    resize: none;
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2);
    font-size: var(--text-sm);
    font-family: inherit;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    flex-shrink: 0;
  }
  .ctl {
    flex: 1;
    min-width: 0;
    max-width: 40%;
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--dim);
    font-size: var(--text-xs);
    padding: 3px var(--space-1);
  }
  .send {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: auto;
    padding: 3px var(--space-3);
    font-size: var(--text-xs);
    background: var(--accent);
    color: var(--bg);
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .send:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
