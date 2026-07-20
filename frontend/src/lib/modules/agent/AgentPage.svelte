<script>
  // Agent chat — a ChatGPT-like pane. Left: a conversation sidebar (new / select / rename /
  // delete / export; a slide-in drawer on narrow screens). Right: the message thread with a
  // streaming composer. The active provider + model come entirely from the agent's settings;
  // if none is configured, the composer nudges the user into settings.
  import { onMount, tick } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import MessageView from '$lib/modules/agent/MessageView.svelte';
  import AgentSettings from '$lib/modules/agent/AgentSettings.svelte';
  import { agentApi, runStream, foldThread } from '$lib/modules/agent/api.js';

  let conversations = $state([]);
  let activeId = $state(null);
  let messages = $state([]); // [{ id, role, text, thinking, tools }]
  let usage = $state({ input_tokens: 0, output_tokens: 0 });
  let loading = $state(true);
  let error = $state('');

  let showSettings = $state(false);
  let sidebarOpen = $state(false); // narrow screens: sidebar as a drawer

  // Modal confirm (replaces native confirm()).
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  // Rename prompt modal (replaces native prompt()).
  let renameOpen = $state(false);
  let renameValue = $state('');
  let renameConv = $state(null);

  // Agent config + providers — single source of truth is the backend agent row; the header
  // picker and the settings modal both write there, and refreshConfig() re-reads it.
  let agentCfg = $state(null);
  let providers = $state([]);
  const activeProvider = $derived(providers.find((p) => p.id === agentCfg?.provider_id) ?? null);
  const activeModel = $derived((agentCfg?.model || activeProvider?.default_model || '').trim());
  const agentReady = $derived(
    !!(activeProvider && activeProvider.enabled && activeProvider.has_key && activeModel)
  );

  // Tools — PER CONVERSATION: each conversation stores its own MCP token (prefilled at
  // creation from the agent default) plus a selection of external MCP servers; the header
  // picker patches both.
  let mcpTokens = $state([]);
  let mcpServers = $state([]); // external servers from the MCP store
  let toolsOpen = $state(false);
  let toolsSearch = $state(''); // filters the tokens + external servers in the picker
  const activeConv = $derived(conversations.find((c) => c.id === activeId) ?? null);
  const activeToken = $derived(mcpTokens.find((tk) => tk.id === activeConv?.mcp_token_id) ?? null);
  // Informational only — the token's own r/rw/rwd levels are the single permission gate.
  const tokenGrantsWrite = $derived(
    Object.values(activeToken?.permissions ?? {}).some((v) => v === 'rw' || v === 'rwd')
  );
  const activeServerIds = $derived(new Set(activeConv?.mcp_servers ?? []));
  const activeServerCount = $derived(
    mcpServers.filter((s) => s.enabled && activeServerIds.has(s.id)).length
  );
  const toolsChipLabel = $derived.by(() => {
    const parts = [];
    if (activeToken) parts.push(activeToken.name);
    if (activeServerCount) parts.push(`+${activeServerCount}`);
    return parts.length ? parts.join(' ') : $t('agent.pick.noTools');
  });
  // Search filters the picker lists by name (case-insensitive). The always-present
  // "no tools" entry is kept regardless so the user can still clear the selection.
  const enabledServers = $derived(mcpServers.filter((s) => s.enabled));
  const filteredTokens = $derived.by(() => {
    const q = toolsSearch.trim().toLowerCase();
    return q ? mcpTokens.filter((tk) => tk.name.toLowerCase().includes(q)) : mcpTokens;
  });
  const filteredServers = $derived.by(() => {
    const q = toolsSearch.trim().toLowerCase();
    return q ? enabledServers.filter((s) => s.name.toLowerCase().includes(q)) : enabledServers;
  });
  const toolsNoMatch = $derived(
    !!toolsSearch.trim() && !filteredTokens.length && !filteredServers.length
  );

  // Provider/model picker (chat header popover).
  let pickOpen = $state(false);
  let modelDraft = $state('');
  let pickModels = $state([]);
  let pickModelsErr = $state('');
  let loadingModels = $state(false);
  let pendingSave = null; // in-flight config save; send() awaits it (switch-then-send race)
  const enabledProviderOpts = $derived(
    providers.filter((p) => p.enabled).map((p) => ({ value: p.id, label: p.label }))
  );
  const filteredModels = $derived.by(() => {
    const q = modelDraft.trim().toLowerCase();
    const list = q ? pickModels.filter((m) => m.toLowerCase().includes(q)) : pickModels;
    return list.slice(0, 60);
  });

  let input = $state('');
  let streaming = $state(false);
  // Which conversation the in-flight run belongs to. `streaming`/`liveTurns` are component
  // state, so without this the live turns render into whatever conversation is open —
  // switching mid-run showed the other thread's stream.
  let streamConvId = $state(null);
  // Assistant turns of the in-flight run (kept out of `messages` until done so Svelte
  // re-renders the growing text cheaply). A run with tool calls spans several turns:
  // text → tool chips → next turn's text. Keeping them all preserves earlier turns on
  // screen instead of blanking them when a tool fires.
  let liveTurns = $state([]);
  // True only when the open conversation is the one being streamed into.
  const streamingHere = $derived(streaming && streamConvId === activeId);
  let abortController = null;

  let threadEl = $state(null);
  let composerEl = $state(null);
  // Set when the agent produces output while the user is reading further up; drives the
  // "new message" pill that jumps back down.
  let unreadBelow = $state(false);

  onMount(() => {
    (async () => {
      await Promise.all([loadConversations(), refreshConfig()]);
      loading = false;
      await runHandoff();
    })();
    // Re-sync the config when the tab regains focus (another tab / the settings modal in
    // another window may have switched provider or model meanwhile).
    const onFocus = () => refreshConfig();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  });

  /** Dashboard-widget handoff: the widget creates a conversation, stashes its id + message
   *  in sessionStorage and redirects here; we open that conversation and send the message. */
  async function runHandoff() {
    let pending = null;
    try {
      const raw = sessionStorage.getItem('otw-agent-handoff');
      if (raw) pending = JSON.parse(raw);
    } catch {
      /* malformed — drop it */
    }
    sessionStorage.removeItem('otw-agent-handoff');
    if (!pending?.conversationId || !pending.message) return;
    if (!conversations.some((c) => c.id === pending.conversationId)) return;
    await openConversation(pending.conversationId);
    input = pending.message;
    await send();
  }

  async function loadConversations() {
    try {
      conversations = await agentApi.listConversations();
      if (conversations.length && !activeId) await openConversation(conversations[0].id);
    } catch (e) {
      error = e.message;
    }
  }

  /** Re-read the agent config + providers (readiness and the header picker derive from it). */
  async function refreshConfig() {
    try {
      [providers, agentCfg] = await Promise.all([agentApi.listProviders(), agentApi.getAgent()]);
      modelDraft = agentCfg.model ?? '';
    } catch {
      agentCfg = null;
      providers = [];
    }
    // Token/server lists are optional (their endpoints may error independently of chat).
    try {
      mcpTokens = await agentApi.listMcpTokens();
    } catch {
      mcpTokens = [];
    }
    try {
      mcpServers = await agentApi.listMcpServers();
    } catch {
      mcpServers = [];
    }
  }

  /** Switch this conversation's tools envelope (null = chat only). */
  async function pickTools(tokenId) {
    if (!activeId) return;
    try {
      const updated = await agentApi.updateConversation(activeId, { mcp_token_id: tokenId });
      conversations = conversations.map((c) => (c.id === updated.id ? updated : c));
    } catch (e) {
      error = e.message;
    }
  }

  /** Set (or clear, when re-checked) the token new conversations start with. */
  async function setDefaultToken(tokenId) {
    await saveCfg({ mcp_token_id: agentCfg?.mcp_token_id === tokenId ? null : tokenId });
  }

  /** Toggle one external MCP server for this conversation. */
  async function toggleServer(serverId) {
    if (!activeId) return;
    const next = new Set(activeConv?.mcp_servers ?? []);
    if (next.has(serverId)) next.delete(serverId);
    else next.add(serverId);
    try {
      const updated = await agentApi.updateConversation(activeId, { mcp_servers: [...next] });
      conversations = conversations.map((c) => (c.id === updated.id ? updated : c));
    } catch (e) {
      error = e.message;
    }
  }

  /** Persist a config change (provider/model switch). Errors surface in the banner. */
  async function saveCfg(patch) {
    const p = agentApi
      .updateAgent(patch)
      .then((a) => {
        agentCfg = a;
        modelDraft = a.model ?? '';
      })
      .catch((e) => {
        error = e.message;
      });
    pendingSave = p;
    await p;
    pendingSave = null;
  }

  function togglePick() {
    pickOpen = !pickOpen;
    if (pickOpen) loadModels();
  }

  function toggleTools() {
    toolsOpen = !toolsOpen;
    if (toolsOpen) toolsSearch = '';
  }
  function closeTools() {
    toolsOpen = false;
    toolsSearch = '';
  }

  async function pickProvider(e) {
    const id = e.target.value || null;
    await saveCfg({ provider_id: id });
    loadModels();
  }

  /** Save the typed/picked model (empty = provider default). */
  async function saveModel(value = modelDraft) {
    modelDraft = value;
    const m = modelDraft.trim();
    if (m === (agentCfg?.model ?? '').trim()) return;
    await saveCfg({ model: m });
  }

  /** Fetch the provider's live model list (best-effort; free text always works). */
  async function loadModels() {
    pickModels = [];
    pickModelsErr = '';
    if (!agentCfg?.provider_id || !activeProvider?.has_key) return;
    loadingModels = true;
    try {
      pickModels = await agentApi.listProviderModels(agentCfg.provider_id);
    } catch (e) {
      pickModelsErr = e.message;
    } finally {
      loadingModels = false;
    }
  }

  async function openConversation(id) {
    activeId = id;
    sidebarOpen = false;
    error = '';
    try {
      const data = await agentApi.getConversation(id);
      messages = foldThread(data.messages);
      usage = data.usage ?? { input_tokens: 0, output_tokens: 0 };
      await scrollBottom(true);
    } catch (e) {
      error = e.message;
    }
  }

  async function newConversation() {
    sidebarOpen = false;
    try {
      const conv = await agentApi.createConversation();
      conversations = [conv, ...conversations];
      activeId = conv.id;
      messages = [];
      usage = { input_tokens: 0, output_tokens: 0 };
    } catch (e) {
      error = e.message;
    }
  }

  function renameConversation(conv) {
    renameConv = conv;
    renameValue = conv.title || '';
    renameOpen = true;
  }

  async function submitRename({ title }) {
    const conv = renameConv;
    if (!conv) return;
    const next = (title ?? '').trim();
    try {
      await agentApi.updateConversation(conv.id, { title: next });
      conversations = conversations.map((c) => (c.id === conv.id ? { ...c, title: next } : c));
    } catch (e) {
      error = e.message;
    }
  }

  function deleteConversation(conv) {
    askConfirm($t('agent.chat.deleteConfirm'), async () => {
      try {
        await agentApi.deleteConversation(conv.id);
        conversations = conversations.filter((c) => c.id !== conv.id);
        if (activeId === conv.id) {
          activeId = null;
          messages = [];
          if (conversations.length) await openConversation(conversations[0].id);
        }
      } catch (e) {
        error = e.message;
      }
    });
  }

  /** True when the thread is scrolled (near) to the bottom. */
  function atBottom() {
    return threadEl && threadEl.scrollHeight - threadEl.scrollTop - threadEl.clientHeight < 120;
  }

  /** Scroll to the bottom — but never yank the user away while they read older messages
   *  (only sticks when they were already at the bottom, unless forced). */
  async function scrollBottom(force = false) {
    const stick = force || atBottom();
    await tick();
    if (stick) {
      if (threadEl) threadEl.scrollTop = threadEl.scrollHeight;
      // Forced scrolls (open/switch conversation, own message, pill click) always land at the
      // bottom, so nothing is left unread below.
      unreadBelow = false;
    } else {
      // New content landed below the viewport while the user reads older messages.
      unreadBelow = true;
    }
  }

  /** Clears the pill as soon as the user scrolls back down on their own. */
  function onThreadScroll() {
    if (unreadBelow && atBottom()) unreadBelow = false;
  }

  function currentTurn() {
    if (!liveTurns.length) liveTurns.push({ text: '', thinking: '', tools: [] });
    return liveTurns[liveTurns.length - 1];
  }

  /** A delta after tool chips means the model started its next turn. */
  function turnForDelta() {
    const cur = currentTurn();
    if (cur.tools.length) {
      liveTurns.push({ text: '', thinking: '', tools: [] });
      return liveTurns[liveTurns.length - 1];
    }
    return cur;
  }

  async function send() {
    const text = input.trim();
    if (!text || streaming) return;
    // A provider/model switch may still be saving — let it land so this message uses it.
    if (pendingSave) await pendingSave;
    if (!agentReady) {
      showSettings = true;
      return;
    }

    // Ensure there is a conversation.
    if (!activeId) {
      await newConversation();
      if (!activeId) return;
    }
    const convId = activeId;

    error = '';
    input = '';
    resetComposerHeight();
    messages = [...messages, { id: `local-${Date.now()}`, role: 'user', text, thinking: '', tools: [] }];
    await scrollBottom(true);

    streaming = true;
    streamConvId = convId;
    liveTurns = [{ text: '', thinking: '', tools: [] }];
    abortController = new AbortController();

    try {
      await runStream(
        convId,
        text,
        {
          onDelta: (d) => {
            turnForDelta().text += d;
            scrollBottom();
          },
          onThinking: (d) => {
            turnForDelta().thinking += d;
          },
          onTool: (tl) => {
            currentTurn().tools.push({
              id: tl.id,
              name: tl.name,
              input: tl.input,
              result: tl.result ?? '',
              is_error: !!tl.is_error
            });
            scrollBottom();
          },
          onDone: () => {},
          onError: (msg) => {
            error = msg;
          }
        },
        abortController.signal
      );
    } catch (e) {
      if (e.name !== 'AbortError') error = e.message;
    } finally {
      streaming = false;
      streamConvId = null;
      liveTurns = [];
      abortController = null;
      // Reload from the server: it has every persisted turn (assistant text + tool calls +
      // results) folded correctly, which is simpler and more reliable than reconstructing a
      // multi-turn tool run on the client. Only when that conversation is still open —
      // otherwise this would yank the user back to the one they left.
      if (activeId === convId) await openConversation(convId);
      agentApi.listConversations().then((c) => (conversations = c)).catch(() => {});
    }
  }

  function stop() {
    abortController?.abort();
  }

  function onKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  /** Grow the composer with its content (up to the CSS max-height). */
  function autogrow() {
    if (!composerEl) return;
    composerEl.style.height = 'auto';
    composerEl.style.height = `${Math.min(composerEl.scrollHeight, 200)}px`;
  }
  function resetComposerHeight() {
    if (composerEl) composerEl.style.height = 'auto';
  }

  function activeTitle() {
    const c = conversations.find((x) => x.id === activeId);
    return c?.title || $t('agent.chat.newChat');
  }

  // Compact token count: 1234 → "1.2k".
  function fmtTokens(n) {
    if (n >= 1000) return `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k`;
    return `${n}`;
  }
  let totalTokens = $derived((usage.input_tokens || 0) + (usage.output_tokens || 0));
</script>

<div class="agent">
  <!-- Sidebar (drawer on narrow screens) -->
  {#if sidebarOpen}
    <button class="backdrop" aria-label={$t('common.close')} onclick={() => (sidebarOpen = false)}></button>
  {/if}
  <aside class="sidebar" class:open={sidebarOpen}>
    <div class="side-head">
      <Button size="sm" variant="primary" icon="plus" onclick={newConversation}>{$t('agent.chat.newChat')}</Button>
      <a class="icon mcp-btn" href="/agent/mcp" title={$t('agent.mcp.title')} aria-label={$t('agent.mcp.title')}>
        <Icon name="plug" size={16} />
      </a>
      <button class="icon" title={$t('agent.chat.settings')} onclick={() => (showSettings = true)}>
        <Icon name="settings" size={16} />
      </button>
    </div>
    <ul class="conv-list">
      {#each conversations as c (c.id)}
        <li class:active={c.id === activeId}>
          <button class="conv-open" onclick={() => openConversation(c.id)} title={c.title || $t('agent.chat.newChat')}>
            <Icon name="message-square" size={14} />
            <span>{c.title || $t('agent.chat.newChat')}</span>
          </button>
          <div class="conv-actions">
            <a class="icon" href={agentApi.exportUrl(c.id)} title={$t('agent.chat.export')} download>
              <Icon name="download" size={13} />
            </a>
            <button class="icon" title={$t('agent.chat.rename')} onclick={() => renameConversation(c)}><Icon name="pencil" size={13} /></button>
            <button class="icon danger" title={$t('agent.delete')} onclick={() => deleteConversation(c)}><Icon name="trash" size={13} /></button>
          </div>
        </li>
      {/each}
    </ul>
  </aside>

  <!-- Chat pane -->
  <section class="chat">
    <header class="chat-head">
      <button class="icon menu-btn" title={$t('agent.chat.conversations')} onclick={() => (sidebarOpen = !sidebarOpen)}>
        <Icon name="menu" size={16} />
      </button>
      <Icon name="brain" size={16} />
      <h1>{activeTitle()}</h1>

      {#if providers.length}
        <div class="pick-wrap">
          <button class="pick" disabled={streaming} title={$t('agent.pick.title')} onclick={togglePick}>
            <span class="pick-provider">{activeProvider?.label ?? $t('agent.set.providerNone')}</span>
            <span class="pick-model">· {activeModel || '—'}</span>
            <Icon name="chevron-down" size={12} />
          </button>
          {#if pickOpen}
            <button class="pick-backdrop" aria-label={$t('common.close')} onclick={() => (pickOpen = false)}></button>
            <div class="pick-pop">
              <label class="pick-lbl" for="pick-provider">{$t('agent.set.provider')}</label>
              <select id="pick-provider" value={agentCfg?.provider_id ?? ''} onchange={pickProvider}>
                {#each enabledProviderOpts as o (o.value)}
                  <option value={o.value}>{o.label}</option>
                {/each}
              </select>
              {#if activeProvider && !activeProvider.has_key}
                <p class="pick-warn">{$t('agent.set.noKey')}</p>
              {/if}

              <label class="pick-lbl" for="pick-model">{$t('agent.set.model')}</label>
              <input
                id="pick-model"
                type="text"
                placeholder={activeProvider?.default_model || $t('agent.set.modelPh')}
                bind:value={modelDraft}
                onblur={() => saveModel()}
                onkeydown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    saveModel();
                    pickOpen = false;
                  }
                }}
              />
              {#if loadingModels}
                <p class="pick-muted">{$t('common.loading')}</p>
              {:else if pickModelsErr}
                <p class="pick-muted">{$t('agent.pick.noModels')}</p>
              {:else if filteredModels.length}
                <ul class="pick-models">
                  {#each filteredModels as m (m)}
                    <li>
                      <button
                        class:sel={m === activeModel}
                        onclick={() => {
                          saveModel(m);
                          pickOpen = false;
                        }}
                      >
                        {m}
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        </div>
      {/if}

      {#if activeId && (mcpTokens.length || mcpServers.length)}
        <div class="pick-wrap">
          <button class="pick" disabled={streaming} title={$t('agent.pick.toolsTitle')} onclick={toggleTools}>
            <Icon name="zap" size={12} />
            <span class="pick-model">{toolsChipLabel}</span>
            <Icon name="chevron-down" size={12} />
          </button>
          {#if toolsOpen}
            <button class="pick-backdrop" aria-label={$t('common.close')} onclick={closeTools}></button>
            <div class="pick-pop">
              <div class="pick-find">
                <span class="find-ico"><Icon name="search" size={13} /></span>
                <input
                  type="text"
                  aria-label={$t('agent.pick.search')}
                  bind:value={toolsSearch}
                  autocomplete="off"
                />
              </div>

              {#if mcpTokens.length}
                <p class="pick-sect">{$t('agent.pick.toolsTitle')}</p>
                <ul class="pick-opts">
                  {#if !toolsSearch.trim()}
                    <li>
                      <button class="pick-opt" class:sel={!activeToken} onclick={() => pickTools(null)}>
                        <span class="opt-ico"><Icon name="message-square" size={14} /></span>
                        <span class="opt-name">{$t('agent.pick.chatOnly')}</span>
                        {#if !activeToken}<span class="opt-check"><Icon name="check" size={14} /></span>{/if}
                      </button>
                    </li>
                  {/if}
                  {#each filteredTokens as tk (tk.id)}
                    {@const sel = tk.id === activeConv?.mcp_token_id}
                    {@const def = tk.id === agentCfg?.mcp_token_id}
                    <li>
                      <button class="pick-opt" class:sel onclick={() => pickTools(tk.id)}>
                        <span class="opt-ico"><Icon name="zap" size={14} /></span>
                        <span class="opt-name">{tk.name}</span>
                        <span class="opt-meta">
                          {$t('agent.pick.modules', { count: Object.keys(tk.permissions || {}).length })}
                        </span>
                        {#if sel}<span class="opt-check"><Icon name="check" size={14} /></span>{/if}
                      </button>
                      <!-- Separate from the row button: picks the token for FUTURE chats
                           (agents.mcp_token_id), not for the open one. -->
                      <label class="opt-def" title={$t('agent.pick.defaultTitle')}>
                        <input type="checkbox" checked={def} onchange={() => setDefaultToken(tk.id)} />
                        <span>{$t('agent.pick.default')}</span>
                      </label>
                    </li>
                  {/each}
                </ul>
                {#if tokenGrantsWrite}
                  <p class="pick-muted">{$t('agent.pick.tokenWrites')}</p>
                {/if}
              {/if}

              {#if filteredServers.length}
                <p class="pick-sect">{$t('agent.pick.external')}</p>
                <ul class="pick-opts">
                  {#each filteredServers as s (s.id)}
                    <li>
                      <label class="pick-opt">
                        <span class="opt-ico"><Icon name="plug" size={14} /></span>
                        <span class="opt-name">{s.name}</span>
                        <input
                          type="checkbox"
                          checked={activeServerIds.has(s.id)}
                          onchange={() => toggleServer(s.id)}
                        />
                      </label>
                    </li>
                  {/each}
                </ul>
                {#if activeServerCount && tokenGrantsWrite}
                  <p class="pick-warn">{$t('agent.pick.extWarn')}</p>
                {/if}
              {/if}

              {#if toolsNoMatch}
                <p class="pick-muted">{$t('agent.pick.noMatch')}</p>
              {/if}

              <div class="pick-links">
                {#if enabledServers.length}
                  <a class="pick-manage" href="/agent/mcp">{$t('agent.pick.manage')} →</a>
                {:else}
                  <a class="pick-manage" href="/agent/mcp">+ {$t('agent.pick.addServer')}</a>
                {/if}
                <a class="pick-manage" href="/settings#mcp">{$t('agent.pick.manageTokens')} →</a>
              </div>
            </div>
          {/if}
        </div>
      {/if}

      {#if activeId && totalTokens > 0}
        <span class="tok" title={$t('agent.chat.tokTitle', { in: usage.input_tokens.toLocaleString(), out: usage.output_tokens.toLocaleString() })}>
          <Icon name="zap" size={12} />
          {fmtTokens(totalTokens)} tok
        </span>
      {/if}
    </header>

    <div class="thread-wrap">
      <div class="thread" bind:this={threadEl} onscroll={onThreadScroll}>
        {#if loading}
          <p class="muted center">{$t('common.loading')}</p>
        {:else if !messages.length && !streamingHere}
          <EmptyState
            icon="brain"
            title={$t('agent.chat.emptyTitle')}
            description={agentReady ? $t('agent.chat.emptyReady') : $t('agent.chat.emptyNotReady')}
          >
            {#snippet action()}
              {#if !agentReady}
                <Button variant="primary" icon="settings" onclick={() => (showSettings = true)}>{$t('agent.chat.openSettings')}</Button>
              {/if}
            {/snippet}
          </EmptyState>
        {:else}
          <div class="msgs">
            {#each messages as m (m.id)}
              <MessageView role={m.role} text={m.text} thinking={m.thinking} tools={m.tools ?? []} />
            {/each}
            {#if streamingHere}
              {#each liveTurns as turn, i (i)}
                <MessageView
                  role="assistant"
                  text={turn.text}
                  thinking={turn.thinking}
                  tools={turn.tools}
                  streaming={i === liveTurns.length - 1}
                />
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      {#if unreadBelow}
        <button class="jump-new" onclick={() => scrollBottom(true)}>
          <Icon name="arrow-down" size={13} />
          {$t('agent.chat.newMessage')}
        </button>
      {/if}
    </div>

    {#if error}
      <div class="err-banner" role="alert">
        <Icon name="alert-triangle" size={14} />
        <span>{error}</span>
        <button class="err-close" title={$t('common.dismiss')} onclick={() => (error = '')}><Icon name="x" size={13} /></button>
      </div>
    {/if}

    <div class="composer">
      <textarea
        placeholder={agentReady ? $t('agent.chat.composerPh') : $t('agent.chat.composerNotReady')}
        bind:value={input}
        bind:this={composerEl}
        onkeydown={onKeydown}
        oninput={autogrow}
        rows="1"
      ></textarea>
      {#if streamingHere}
        <Button variant="danger" icon="x" onclick={stop}>{$t('agent.chat.stop')}</Button>
      {:else}
        <Button variant="primary" icon="send" onclick={send} disabled={!input.trim() || streaming}>{$t('agent.chat.send')}</Button>
      {/if}
    </div>
  </section>
</div>

<Modal bind:open={showSettings} title={$t('agent.chat.settingsTitle')} size="lg">
  {#if showSettings}
    <AgentSettings onsaved={refreshConfig} />
  {/if}
</Modal>

<PromptModal
  bind:open={renameOpen}
  title={$t('agent.chat.rename')}
  fields={[{ key: 'title', label: $t('agent.chat.renamePrompt'), value: renameValue }]}
  confirmLabel={$t('common.save')}
  onconfirm={submitRename}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('agent.confirm.title')}
  message={confirmMessage}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={onConfirmYes}
/>

<style>
  .agent {
    position: relative;
    display: grid;
    grid-template-columns: 260px 1fr;
    height: 100%;
    min-height: 0;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    min-height: 0;
    background: var(--surface);
  }
  .side-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  /* MCP + settings sit together at the right edge; the new-chat button takes the slack. */
  .mcp-btn {
    margin-left: auto;
  }
  .conv-list {
    list-style: none;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .conv-list li {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
    border-left: 1.5px solid transparent;
  }
  .conv-list li.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
  }
  .conv-open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    padding: 7px var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
  }
  .conv-open span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .conv-actions {
    display: none;
    align-items: center;
    gap: 1px;
    padding-right: 4px;
  }
  .conv-list li:hover .conv-actions,
  .conv-list li.active .conv-actions {
    display: flex;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    width: 26px;
    height: 26px;
    border-radius: var(--radius);
    text-decoration: none;
  }
  .icon:hover {
    color: var(--text);
    background: var(--surface);
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .backdrop {
    display: none;
  }
  .menu-btn {
    display: none;
  }

  .chat {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .chat-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
  }
  .chat-head h1 {
    margin: 0;
    font-size: 1.05rem;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick-wrap {
    position: relative;
    min-width: 0;
  }
  .pick {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 320px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    padding: 3px 9px;
    font: inherit;
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .pick:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-control);
  }
  .pick:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .pick-provider {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .pick-model {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pick-backdrop {
    position: fixed;
    inset: 0;
    z-index: 25;
    background: transparent;
    border: none;
    cursor: default;
  }
  .pick-pop {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 30;
    width: 320px;
    max-width: 84vw;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-3);
  }
  .pick-lbl {
    color: var(--muted);
    font-size: var(--text-xs);
    margin: var(--space-1) 0 0;
  }
  .pick-pop select,
  .pick-pop input {
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 9px;
    font: inherit;
    font-size: var(--text-sm);
  }
  .pick-pop select:focus,
  .pick-pop input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .pick-warn {
    margin: 0;
    color: var(--amber);
    font-size: var(--text-xs);
  }
  .pick-muted {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .pick-models {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .pick-models button {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    padding: 6px 9px;
    font: inherit;
    font-size: var(--text-xs);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick-models button:hover {
    background: var(--surface-2);
  }
  .pick-models button.sel {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  /* Tools popover: icon-only search pill, then flat option rows.
     One row shape for both lists — icon, name, meta, trailing check/checkbox. */
  .pick-find {
    position: relative;
  }
  .find-ico {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    color: var(--muted);
    pointer-events: none;
  }
  .pick-find input {
    padding-left: 30px;
    border-radius: var(--radius);
  }
  .pick-sect {
    margin: var(--space-1) 0 0;
    color: var(--dim);
    font-size: 0.62rem;
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .pick-opts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-height: 240px;
    overflow-y: auto;
  }
  .pick-opt {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .pick-opt:hover {
    background: var(--surface-2);
  }
  .pick-opt.sel {
    background: var(--surface-2);
  }
  .opt-ico {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--muted);
  }
  .pick-opt.sel .opt-ico {
    color: var(--text);
  }
  .opt-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick-opt.sel .opt-name {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .opt-meta {
    flex-shrink: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .opt-check {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--accent);
  }
  /* Sits under its token row, indented to the row's icon column. */
  .opt-def {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px 8px 6px 30px;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .opt-def:hover {
    color: var(--text);
  }
  /* Undo the popover-wide input styling for the row checkboxes. */
  .pick-opt input[type='checkbox'],
  .opt-def input[type='checkbox'] {
    width: 14px;
    height: 14px;
    padding: 0;
    margin: 0;
    flex-shrink: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .pick-links {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-1) var(--space-2);
    margin-top: var(--space-1);
    border-top: 1px solid var(--border);
    padding-top: var(--space-2);
  }
  .pick-manage {
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }
  .pick-manage:hover {
    text-decoration: underline;
  }

  .tok {
    margin-left: auto;
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: var(--text-xs);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px 7px;
    white-space: nowrap;
  }
  .err-banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 auto var(--space-2);
    max-width: 820px;
    width: calc(100% - var(--space-8));
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--red);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--red) 12%, transparent);
    color: var(--text);
    font-size: var(--text-sm);
  }
  .err-banner span {
    flex: 1;
    min-width: 0;
  }
  .err-close {
    flex-shrink: 0;
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
    border-radius: var(--radius);
  }
  .err-close:hover {
    color: var(--text);
  }
  .thread-wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .thread {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4);
  }
  .jump-new {
    position: absolute;
    bottom: var(--space-3);
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 6px var(--space-3);
    font-size: 0.8rem;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.18);
    cursor: pointer;
  }
  .jump-new:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .msgs {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 820px;
    margin: 0 auto;
  }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--border);
    max-width: 852px;
    width: 100%;
    margin: 0 auto;
  }
  .composer textarea {
    flex: 1;
    resize: none;
    max-height: 200px;
    min-height: 40px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 9px 11px;
    font: inherit;
    font-size: var(--text-base);
    line-height: 1.4;
  }
  .composer textarea:focus {
    outline: none;
    border-color: var(--accent);
  }
  .center {
    text-align: center;
    margin-top: var(--space-6);
  }
  .muted {
    color: var(--muted);
  }

  @media (max-width: 720px) {
    .agent {
      grid-template-columns: 1fr;
    }
    /* Sidebar becomes a slide-in drawer with a dismissable backdrop. */
    .sidebar {
      position: absolute;
      inset: 0 auto 0 0;
      width: min(280px, 85vw);
      z-index: 20;
      transform: translateX(-100%);
      transition: transform 0.18s ease;
      border-right: 1px solid var(--border-control);
    }
    .sidebar.open {
      transform: none;
    }
    .backdrop {
      display: block;
      position: absolute;
      inset: 0;
      z-index: 15;
      background: rgba(0, 0, 0, 0.35);
      border: none;
      cursor: pointer;
    }
    .menu-btn {
      display: inline-flex;
    }
    /* Touch screens have no hover: keep the row actions always visible. */
    .conv-actions {
      display: flex;
    }
  }
</style>
