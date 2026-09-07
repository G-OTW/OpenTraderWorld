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
  import ModelPickerModal from '$lib/modules/agent/ModelPickerModal.svelte';
  import PromptPickerModal from '$lib/modules/agent/PromptPickerModal.svelte';
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

  // Agent config + providers. The settings modal writes the DEFAULT agent row (what new
  // conversations inherit); the header picker writes THIS conversation, so two threads can
  // run different models side by side.
  //
  // Which one is actually in force is resolved server-side and arrives as
  // `effective_provider_id` / `effective_model` — the fallback order (conversation → persona
  // → default agent) lives in one place, not in a copy here that would drift.
  let agentCfg = $state(null);
  let providers = $state([]);
  const activeProvider = $derived(
    providers.find((p) => p.id === (activeConv?.effective_provider_id ?? agentCfg?.provider_id)) ??
      null
  );
  const activeModel = $derived(
    (activeConv?.effective_model || agentCfg?.model || activeProvider?.default_model || '').trim()
  );
  // True when this conversation overrides the inherited choice — worth showing, because the
  // same persona then answers differently in two tabs.
  const modelOverridden = $derived(!!(activeConv?.provider_id || activeConv?.model));
  const agentReady = $derived(
    !!(activeProvider && activeProvider.enabled && activeProvider.has_key && activeModel)
  );

  // Wide mode: the thread is sized for prose (820px), but the agent emits tables — a trial
  // ledger or a stats breakdown is unreadable squeezed into a reading column. Remembered
  // across sessions: it is a workspace preference, not a property of one conversation.
  let wide = $state(false);
  function toggleWide() {
    wide = !wide;
    try {
      localStorage.setItem('otw-agent-wide', wide ? '1' : '0');
    } catch {
      /* private mode: the toggle still works for this session */
    }
  }

  // Personas — a conversation is opened as one and can be switched mid-thread. A persona is a
  // prompt + a skill shelf; it grants no access of its own (that is the tools envelope below).
  let personas = $state([]); // [{ agent, effective_skills }]
  let personaOpen = $state(false);
  const activePersona = $derived(
    personas.find((p) => p.agent.id === activeConv?.agent_id) ?? null
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

  // Provider/model picker + Prompt Store browser — both fixed dialogs, so the choice is
  // made on a full-size list and confirmed, not committed by a stray click in a popover.
  let modelPickOpen = $state(false);
  let promptPickOpen = $state(false);
  let pendingSave = null; // in-flight config save; send() awaits it (switch-then-send race)

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
  // The in-flight turn, and whether it currently has anything on screen. A turn is only
  // created when a text delta arrives, so while tools run the last turn holds chips and no
  // text — and MessageView's cursor is suppressed once a turn has tools. That left the whole
  // tool phase, which is most of a skill-driven run, with no sign of life at all.
  const liveTail = $derived(liveTurns.length ? liveTurns[liveTurns.length - 1] : null);
  const waiting = $derived(streamingHere && !liveTail?.text);
  // Before the first tool call the model is composing; after one it is working through a
  // round. Neither can be observed precisely (the server reports a tool once it has already
  // run), so the label stays honest about the phase rather than naming a step.
  const waitingLabel = $derived(
    liveTail?.tools?.length ? $t('agent.chat.working') : $t('agent.chat.thinkingNow')
  );
  let abortController = null;
  // A write the agent wants to make, parked server-side until answered (R5). While this is
  // set the run is alive but idle: nothing has been mutated, and nothing will be unless the
  // user approves. Cleared when the decision lands as a tool result, or when the run ends.
  let pendingWrite = $state(null);
  let deciding = $state(false);

  async function decideWrite(approve) {
    if (!pendingWrite || deciding) return;
    deciding = true;
    const w = pendingWrite;
    try {
      await agentApi.confirmWrite(w.conversationId, w.id, approve);
    } catch (e) {
      // The wait timed out or the run stopped — either way nothing was written. Say so
      // instead of leaving a chip that looks live.
      error = e.message;
      pendingWrite = null;
    } finally {
      deciding = false;
    }
  }

  let threadEl = $state(null);
  let composerEl = $state(null);
  // Set when the agent produces output while the user is reading further up; drives the
  // "new message" pill that jumps back down.
  let unreadBelow = $state(false);

  onMount(() => {
    try {
      wide = localStorage.getItem('otw-agent-wide') === '1';
    } catch {
      /* ignore */
    }
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
      if (conversations.length && !activeId) {
        await openConversation(conversations[0].id);
      } else if (!conversations.length) {
        // Always keep one conversation open. The persona and tools pickers act on the OPEN
        // conversation, so with none there is nothing to attach them to — and the tools
        // envelope is what decides whether the agent can reach any data at all. It has to be
        // visible and settable before the first message, not after.
        await newConversation();
      }
    } catch (e) {
      error = e.message;
    }
  }

  /** Re-read the agent config + providers (readiness and the header picker derive from it). */
  async function refreshConfig() {
    try {
      [providers, agentCfg] = await Promise.all([agentApi.listProviders(), agentApi.getAgent()]);
    } catch {
      agentCfg = null;
      providers = [];
    }
    try {
      personas = await agentApi.listAgents();
    } catch {
      personas = [];
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

  /** Switch this conversation's persona. Applies from the next message: the server drops a
   *  marker into the transcript so the turns above stay attributed to who produced them. */
  async function pickPersona(agentId) {
    personaOpen = false;
    if (!activeId || agentId === activeConv?.agent_id) return;
    const next = personas.find((p) => p.agent.id === agentId);
    const apply = async () => {
      try {
        const updated = await agentApi.updateConversation(activeId, { agent_id: agentId });
        conversations = conversations.map((c) => (c.id === updated.id ? updated : c));
        // Re-read so the marker row appears without a manual refresh.
        const data = await agentApi.getConversation(activeId);
        messages = foldThread(data.messages);
        await scrollBottom(true);
      } catch (e) {
        error = e.message;
      }
    };
    // Nothing said yet: no handover to explain, just switch.
    if (!messages.length) return apply();
    askConfirm(
      $t('agent.persona.switchConfirm', {
        to: next?.agent.name ?? '',
        from: activePersona?.agent.name ?? ''
      }),
      apply
    );
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

  /** Persist a default-agent config change (what NEW conversations inherit). */
  async function saveCfg(patch) {
    const p = agentApi
      .updateAgent(patch)
      .then((a) => {
        agentCfg = a;
      })
      .catch((e) => {
        error = e.message;
      });
    pendingSave = p;
    await p;
    pendingSave = null;
  }

  /**
   * Persist a provider/model switch for THIS conversation only. Sending `null`/`''` clears
   * the override and the conversation goes back to inheriting.
   */
  async function saveConvModel(patch) {
    if (!activeId) return;
    const p = agentApi
      .updateConversation(activeId, patch)
      .then((c) => {
        conversations = conversations.map((x) => (x.id === c.id ? c : x));
      })
      .catch((e) => {
        error = e.message;
      });
    pendingSave = p;
    await p;
    pendingSave = null;
  }

  function toggleTools() {
    toolsOpen = !toolsOpen;
    if (toolsOpen) toolsSearch = '';
  }
  function closeTools() {
    toolsOpen = false;
    toolsSearch = '';
  }

  /** Validated in the picker: a provider/model pair for THIS conversation (null/'' = inherit). */
  async function applyModel(patch) {
    await saveConvModel(patch);
  }

  /** A prompt picked from the Store lands in the composer, appended to whatever is typed. */
  function insertPrompt(body) {
    input = input.trim() ? `${input.trimEnd()}\n\n${body}` : body;
    tick().then(() => {
      autogrow();
      composerEl?.focus();
    });
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

  async function newConversation(agentId = null) {
    sidebarOpen = false;
    try {
      const conv = await agentApi.createConversation(agentId);
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
            // The write this answers has been decided; clear the prompt either way.
            if (pendingWrite?.id === tl.id) pendingWrite = null;
            scrollBottom();
          },
          // R5: the run is parked until the user answers. Nothing has been written yet.
          onConfirm: (w) => {
            pendingWrite = { ...w, conversationId: convId };
            scrollBottom(true);
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
      pendingWrite = null;
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

<div class="agent" class:wide>
  <!-- Sidebar (drawer on narrow screens) -->
  {#if sidebarOpen}
    <button class="backdrop" aria-label={$t('common.close')} onclick={() => (sidebarOpen = false)}></button>
  {/if}
  <aside class="sidebar" class:open={sidebarOpen}>
    <div class="side-head">
      <!-- Wrapped: passing the handler directly would hand the click event to `agentId`. -->
      <Button size="sm" variant="primary" icon="plus" onclick={() => newConversation()}>
        {$t('agent.chat.newChat')}
      </Button>
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
        <button
          class="pick"
          disabled={streaming}
          title={$t('agent.pick.title')}
          onclick={() => (modelPickOpen = true)}
        >
          <span class="pick-provider">{activeProvider?.label ?? $t('agent.set.providerNone')}</span>
          <span class="pick-model">· {activeModel || '—'}</span>
          {#if modelOverridden}<span class="pick-dot" title={$t('agent.pick.overridden')}></span>{/if}
          <Icon name="chevron-down" size={12} />
        </button>
      {/if}

      {#if activeId && personas.length}
        <div class="pick-wrap">
          <button
            class="pick"
            disabled={streaming}
            title={$t('agent.persona.pickTitle')}
            onclick={() => (personaOpen = !personaOpen)}
          >
            <Icon name="user" size={12} />
            <span class="pick-model">{activePersona?.agent.name ?? $t('agent.persona.default')}</span>
            <Icon name="chevron-down" size={12} />
          </button>
          {#if personaOpen}
            <button
              class="pick-backdrop"
              aria-label={$t('common.close')}
              onclick={() => (personaOpen = false)}
            ></button>
            <div class="pick-pop">
              <p class="pick-sect">{$t('agent.persona.pickTitle')}</p>
              <ul class="pick-opts">
                {#each personas as p (p.agent.id)}
                  {@const sel = p.agent.id === activeConv?.agent_id}
                  <li>
                    <button class="pick-opt" class:sel onclick={() => pickPersona(p.agent.id)}>
                      <span class="opt-ico"><Icon name="user" size={14} /></span>
                      <span class="opt-name">{p.agent.name}</span>
                      <span class="opt-meta">
                        {$t('agent.persona.skills', { count: p.effective_skills.length })}
                      </span>
                      {#if sel}<span class="opt-check"><Icon name="check" size={14} /></span>{/if}
                    </button>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Always shown once a conversation is open, even with no tokens and no servers: the
           tools envelope is what decides whether the agent can reach any data, so it must be
           inspectable before the first message. With nothing configured the picker still
           offers the links that create the first token/server. -->
      {#if activeId}
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

      <button
        class="icon wide-toggle"
        title={wide ? $t('agent.chat.narrowTitle') : $t('agent.chat.wideTitle')}
        aria-pressed={wide}
        onclick={toggleWide}
      >
        <!-- One icon both ways: the pressed state carries the mode, and the title says which
             action the click performs. (There is no `minimize` in the icon set.) -->
        <Icon name={wide ? 'move-horizontal' : 'maximize'} size={15} />
      </button>

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
              {#if pendingWrite}
                <!-- R5: nothing is written until this is answered. The exact call is shown,
                     because approving a summary of a call is not consent to the call. -->
                <div class="confirm" class:destructive={pendingWrite.destructive} role="alertdialog">
                  <div class="confirm-head">
                    <Icon name={pendingWrite.destructive ? 'alert-triangle' : 'lock'} size={14} />
                    <strong>
                      {pendingWrite.destructive
                        ? $t('agent.confirmWrite.titleDestructive')
                        : $t('agent.confirmWrite.title')}
                    </strong>
                  </div>
                  <code class="confirm-call">{pendingWrite.method} {pendingWrite.path}</code>
                  {#if pendingWrite.body != null}
                    <pre class="confirm-body">{JSON.stringify(pendingWrite.body, null, 2)}</pre>
                  {/if}
                  <div class="confirm-actions">
                    <Button size="sm" variant="primary" loading={deciding} onclick={() => decideWrite(true)}>
                      {$t('agent.confirmWrite.approve')}
                    </Button>
                    <Button size="sm" variant="ghost" onclick={() => decideWrite(false)}>
                      {$t('agent.confirmWrite.decline')}
                    </Button>
                  </div>
                </div>
              {:else if waiting}
                <div class="working" role="status" aria-live="polite">
                  <span class="dots"><i></i><i></i><i></i></span>
                  <span>{waitingLabel}</span>
                </div>
              {/if}
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
      <button
        class="composer-btn"
        title={$t('agent.promptPick.title')}
        aria-label={$t('agent.promptPick.title')}
        onclick={() => (promptPickOpen = true)}
      >
        <Icon name="message-square" size={16} />
      </button>
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

<ModelPickerModal
  bind:open={modelPickOpen}
  {providers}
  providerId={activeConv?.provider_id ?? null}
  model={activeConv?.model ?? ''}
  effective={modelOverridden ? '' : [activeProvider?.label, activeModel].filter(Boolean).join(' · ')}
  scope={$t('agent.pick.thisChatOnly')}
  onapply={applyModel}
/>

<PromptPickerModal bind:open={promptPickOpen} oninsert={insertPrompt} />

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
  /* Sign of life while the run produces nothing visible: waiting on the first token, or
     working through a tool round. */
  .working {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
    padding-left: var(--space-3);
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-3);
    margin-left: var(--space-3);
  }
  .confirm.destructive {
    border-color: var(--red);
  }
  .confirm-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--amber);
    font-size: var(--text-sm);
  }
  .confirm.destructive .confirm-head {
    color: var(--red);
  }
  .confirm-call {
    font-size: var(--text-xs);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .confirm-body {
    margin: 0;
    max-height: 200px;
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .confirm-actions {
    display: flex;
    gap: var(--space-2);
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .dots i {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: currentColor;
    animation: dot 1.2s ease-in-out infinite;
  }
  .dots i:nth-child(2) {
    animation-delay: 0.15s;
  }
  .dots i:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes dot {
    0%, 60%, 100% {
      opacity: 0.25;
    }
    30% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dots i {
      animation: none;
      opacity: 0.6;
    }
  }

  /* Thread width lives in one variable so the message column, the error banner and the
     composer can never drift apart. Wide mode is for tabular output, not for prose. */
  .agent {
    --thread-max: 820px;
  }
  .agent.wide {
    --thread-max: min(1500px, 100%);
  }
  .wide-toggle[aria-pressed='true'] {
    color: var(--accent);
  }

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
  .pick-pop input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .pick-warn {
    margin: 0;
    color: var(--amber);
    font-size: var(--text-xs);
  }
  /* Marks a conversation that runs on something other than the inherited model. */
  .pick-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
  }
  .pick-muted {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
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
    max-width: var(--thread-max);
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
    max-width: var(--thread-max);
    margin: 0 auto;
  }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--border);
    /* Tracks the thread, plus the composer's own horizontal padding. */
    max-width: calc(var(--thread-max) + 32px);
    width: 100%;
    margin: 0 auto;
  }
  /* Prompt Store shortcut, sized to the textarea's resting height. */
  .composer-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    flex: none;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .composer-btn:hover {
    color: var(--text);
    border-color: var(--border-control);
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
  /* Send / stop sits on the composer's line, not the shorter default control. */
  .composer :global(.btn) {
    height: 40px;
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
