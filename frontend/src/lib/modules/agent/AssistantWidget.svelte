<script>
  // Floating assistant — the Agent module, one click away on every page. Collapsed it is a
  // single button in the bottom-right corner; expanded it is a compact chat over the same
  // conversations/personas/providers as /agent (nothing here is a parallel system: every
  // thread it creates shows up on the Agent page and vice versa).
  //
  // Page awareness: each run carries the current page's module id; the server preloads that
  // module's endpoint catalog into the system prompt (when the tools token grants it), so
  // "download these bars" asked from /histdata needs no discovery round. Other modules stay
  // reachable through otw_catalog.
  import { onMount, tick } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import MessageView from '$lib/modules/agent/MessageView.svelte';
  import ModelPicker from '$lib/modules/agent/ModelPicker.svelte';
  import PromptPicker from '$lib/modules/agent/PromptPicker.svelte';
  import { agentApi, runStream, foldThread } from '$lib/modules/agent/api.js';
  import { moduleForPath } from '$lib/modules/registry';

  const OPEN_KEY = 'otw.assistant.open';
  const CONV_KEY = 'otw.assistant.conv';

  // Frontend page id → MCP catalog module id, where the two differ. Pages absent from the
  // MCP catalog (dashboard, settings…) simply send no page context.
  const PAGE_TO_MCP = { histviz: 'histdata' };
  const MCP_MODULES = new Set([
    'journal', 'portfolios', 'watchlists', 'backtest', 'quant', 'histdata', 'connectors',
    'findb', 'mportfolios', 'wealth', 'subscriptions', 'taxcalc', 'editor', 'todos', 'goals',
    'remindme', 'calendar', 'time', 'routines', 'mindset', 'news', 'resources',
    'prompt-store', 'community-docs', 'mailbox'
  ]);
  const pageModule = $derived.by(() => {
    const mod = moduleForPath($page.url.pathname);
    const id = PAGE_TO_MCP[mod.id] ?? mod.id;
    return MCP_MODULES.has(id) ? id : null;
  });
  const pageName = $derived(moduleForPath($page.url.pathname).name);

  let open = $state(false);
  let booted = $state(false);
  let error = $state('');

  // Config (same rows the Agent page reads).
  let personas = $state([]); // [{ agent, effective_skills }]
  let providers = $state([]);
  let agentCfg = $state(null); // the default agent

  // Conversation.
  let convId = $state(null);
  let conv = $state(null);
  let messages = $state([]);
  let loadingThread = $state(false);

  // Streaming.
  let input = $state('');
  let streaming = $state(false);
  let liveTurns = $state([]);
  let pendingWrite = $state(null);
  let deciding = $state(false);
  let abortController = null;

  // Selections drafted before the conversation exists (applied at creation).
  let personaDraft = $state(null); // agent id
  let modelDraft = $state(null); // { provider_id, model } | null = inherit

  // The pickers take over the panel body instead of floating over it: at 400px a popover
  // could hold neither a model list nor a prompt, and a dialog covers the chat anyway.
  let view = $state('chat'); // 'chat' | 'model' | 'prompt'

  let threadEl = $state(null);
  let composerEl = $state(null);

  const personaSel = $derived(
    conv?.agent_id ?? personaDraft ?? personas.find((p) => p.agent.is_default)?.agent.id ?? ''
  );
  const activePersona = $derived(personas.find((p) => p.agent.id === personaSel)?.agent ?? null);

  // Same resolution chain as the Agent page: conversation override → persona → default agent.
  const activeProvider = $derived(
    providers.find(
      (p) =>
        p.id ===
        (conv?.effective_provider_id ??
          modelDraft?.provider_id ??
          activePersona?.provider_id ??
          agentCfg?.provider_id)
    ) ?? null
  );
  const activeModel = $derived(
    (
      conv?.effective_model ||
      modelDraft?.model ||
      activePersona?.model ||
      agentCfg?.model ||
      activeProvider?.default_model ||
      ''
    ).trim()
  );
  const overridden = $derived(!!(conv?.provider_id || conv?.model || modelDraft));
  const ready = $derived(
    !!(activeProvider && activeProvider.enabled && activeProvider.has_key && activeModel)
  );

  const liveTail = $derived(liveTurns.length ? liveTurns[liveTurns.length - 1] : null);
  const waiting = $derived(streaming && !liveTail?.text);

  // Restore the corner state once, client-side.
  onMount(() => {
    try {
      if (localStorage.getItem(OPEN_KEY) === '1') expand();
    } catch {
      /* private mode */
    }
  });

  function persistOpen(v) {
    try {
      localStorage.setItem(OPEN_KEY, v ? '1' : '0');
    } catch {
      /* private mode */
    }
  }

  async function expand() {
    open = true;
    persistOpen(true);
    if (!booted) {
      booted = true;
      await boot();
    }
    await scrollBottom(true);
  }

  function collapse() {
    open = false;
    view = 'chat';
    persistOpen(false);
  }

  async function boot() {
    try {
      const [ps, prov, cfg] = await Promise.all([
        agentApi.listAgents(),
        agentApi.listProviders(),
        agentApi.getAgent()
      ]);
      personas = ps;
      providers = prov;
      agentCfg = cfg;
    } catch (e) {
      error = e.message;
    }
    let stored = null;
    try {
      stored = localStorage.getItem(CONV_KEY);
    } catch {
      /* private mode */
    }
    if (stored) await openConversation(stored, { silent: true });
  }

  async function openConversation(id, { silent = false } = {}) {
    loadingThread = true;
    try {
      const data = await agentApi.getConversation(id);
      conv = data.conversation;
      convId = conv.id;
      messages = foldThread(data.messages);
      try {
        localStorage.setItem(CONV_KEY, convId);
      } catch {
        /* private mode */
      }
      await scrollBottom(true);
    } catch (e) {
      // Deleted from the Agent page, or gone after a wipe: start fresh, quietly.
      convId = null;
      conv = null;
      messages = [];
      if (!silent) error = e.message;
      try {
        localStorage.removeItem(CONV_KEY);
      } catch {
        /* private mode */
      }
    } finally {
      loadingThread = false;
    }
  }

  function newChat() {
    if (streaming) return;
    view = 'chat';
    convId = null;
    conv = null;
    messages = [];
    liveTurns = [];
    error = '';
    modelDraft = null;
    try {
      localStorage.removeItem(CONV_KEY);
    } catch {
      /* private mode */
    }
  }

  /** Create the conversation on first use, applying drafted persona/model overrides. */
  async function ensureConversation() {
    if (convId) return convId;
    const created = await agentApi.createConversation(personaDraft ?? undefined);
    conv = created;
    convId = created.id;
    if (modelDraft) {
      conv = await agentApi.updateConversation(convId, modelDraft);
      modelDraft = null;
    }
    try {
      localStorage.setItem(CONV_KEY, convId);
    } catch {
      /* private mode */
    }
    return convId;
  }

  async function pickPersona(id) {
    if (!convId) {
      personaDraft = id;
      return;
    }
    try {
      await agentApi.updateConversation(convId, { agent_id: id });
      await openConversation(convId); // pick up the handover marker + new defaults
    } catch (err) {
      error = err.message;
    }
  }

  /** Apply a provider/model override (null/'' = back to inheriting the persona's). Before the
   *  conversation exists the choice is drafted and applied at creation. */
  async function setModel(patch) {
    if (!convId) {
      modelDraft = patch.provider_id || patch.model ? patch : null;
      return;
    }
    try {
      conv = await agentApi.updateConversation(convId, patch);
    } catch (e) {
      error = e.message;
    }
  }

  function insertPrompt(body) {
    input = input.trim() ? `${input.trimEnd()}\n\n${body}` : body;
    tick().then(() => {
      autogrow({ target: composerEl });
      composerEl?.focus();
    });
  }

  async function decideWrite(approve) {
    if (!pendingWrite || deciding) return;
    deciding = true;
    const w = pendingWrite;
    try {
      await agentApi.confirmWrite(w.conversationId, w.id, approve);
      pendingWrite = null;
    } catch (e) {
      error = e.message;
    } finally {
      deciding = false;
    }
  }

  function atBottom() {
    return threadEl && threadEl.scrollHeight - threadEl.scrollTop - threadEl.clientHeight < 100;
  }

  /** Land at the bottom after the DOM has settled — a tick alone lands before the markdown
   *  of the new message is laid out, which is what left the panel one message behind. */
  async function scrollBottom(force = false) {
    const stick = force || atBottom();
    if (!stick) return;
    await tick();
    requestAnimationFrame(() => {
      if (threadEl) threadEl.scrollTop = threadEl.scrollHeight;
    });
  }

  // The thread element is destroyed while a picker view is up, so coming back would show it
  // scrolled to the top.
  $effect(() => {
    if (view === 'chat') scrollBottom(true);
  });

  function currentTurn() {
    if (!liveTurns.length) liveTurns.push({ text: '', thinking: '', tools: [] });
    return liveTurns[liveTurns.length - 1];
  }

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
    if (!ready) return;

    error = '';
    let id;
    try {
      id = await ensureConversation();
    } catch (e) {
      error = e.message;
      return;
    }
    input = '';
    if (composerEl) composerEl.style.height = 'auto';
    messages = [
      ...messages,
      { id: `local-${Date.now()}`, role: 'user', text, thinking: '', tools: [] }
    ];
    await scrollBottom(true);

    streaming = true;
    liveTurns = [{ text: '', thinking: '', tools: [] }];
    abortController = new AbortController();

    try {
      await runStream(
        id,
        text,
        {
          onDelta: (d) => {
            const turn = turnForDelta();
            turn.text += d;
            // A turn's first token is a new message on screen: go down whatever the user was
            // reading. Mid-turn tokens only follow if they were already at the bottom.
            scrollBottom(turn.text === d);
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
            if (pendingWrite?.id === tl.id) pendingWrite = null;
            scrollBottom(true);
          },
          onConfirm: (w) => {
            pendingWrite = { ...w, conversationId: id };
            scrollBottom(true);
          },
          onDone: () => {},
          onError: (msg) => {
            error = msg;
          }
        },
        abortController.signal,
        pageModule ? { page: pageModule } : {}
      );
    } catch (e) {
      if (e.name !== 'AbortError') error = e.message;
    } finally {
      streaming = false;
      liveTurns = [];
      pendingWrite = null;
      abortController = null;
      if (convId === id) await openConversation(id, { silent: true });
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

  function autogrow(e) {
    const el = e.target;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 120)}px`;
  }

</script>

<div class="aw" class:open>
  {#if !open}
    <button class="aw-fab" title={$t('assistant.open')} aria-label={$t('assistant.open')} onclick={expand}>
      <Icon name="brain" size={20} />
      {#if streaming}<span class="aw-dot" aria-hidden="true"></span>{/if}
    </button>
  {:else}
    <section class="aw-panel" aria-label={$t('assistant.title')}>
      <!-- Line 1: what this is, and the window controls. Nothing selectable. -->
      <header class="aw-head">
        <span class="aw-brand"><Icon name="brain" size={15} /> {$t('assistant.title')}</span>
        <span class="aw-spacer"></span>
        <button class="aw-icon" title={$t('agent.chat.newChat')} onclick={newChat} disabled={streaming}>
          <Icon name="plus" size={15} />
        </button>
        <button class="aw-icon" title={$t('assistant.fullPage')} onclick={() => goto('/agent')}>
          <Icon name="external-link" size={14} />
        </button>
        <button class="aw-icon" title={$t('assistant.close')} onclick={collapse}>
          <Icon name="chevron-down" size={16} />
        </button>
      </header>

      {#if view !== 'chat'}
        <div class="aw-viewhead">
          <button class="aw-icon" title={$t('common.back')} onclick={() => (view = 'chat')}>
            <Icon name="chevron-left" size={16} />
          </button>
          <span>{view === 'model' ? $t('agent.pick.modelTitle') : $t('agent.promptPick.title')}</span>
        </div>
        <div class="aw-view">
          {#if view === 'model'}
            <ModelPicker
              compact
              {providers}
              providerId={conv?.provider_id ?? modelDraft?.provider_id ?? null}
              model={conv?.model ?? modelDraft?.model ?? ''}
              effective={overridden
                ? ''
                : [activeProvider?.label, activeModel].filter(Boolean).join(' · ')}
              scope={$t('agent.pick.thisChatOnly')}
              onapply={(patch) => {
                view = 'chat';
                setModel(patch);
              }}
              oncancel={() => (view = 'chat')}
            />
          {:else}
            <PromptPicker
              compact
              oninsert={(body) => {
                view = 'chat';
                insertPrompt(body);
              }}
              oncancel={() => (view = 'chat')}
            />
          {/if}
        </div>
      {:else}
        <!-- Line 2: the selectors — persona, model, prompts — plus the page in context. -->
        <div class="aw-toolbar">
          <div class="aw-select">
            <Dropdown
              title={$t('assistant.persona')}
              ariaLabel={$t('assistant.persona')}
              value={personaSel}
              onpick={pickPersona}
              disabled={streaming}
              options={personas.map((p) => ({ value: p.agent.id, label: p.agent.name }))}
            />
          </div>

          <button
            class="aw-chip model"
            title={$t('agent.pick.modelTitle')}
            disabled={streaming}
            onclick={() => (view = 'model')}
          >
            <Icon name="zap" size={12} />
            <span class="aw-chip-label">{activeModel || $t('agent.set.model')}</span>
            {#if overridden}<span class="aw-dot-sm" title={$t('agent.pick.overridden')}></span>{/if}
          </button>

          <button
            class="aw-chip"
            title={$t('agent.promptPick.title')}
            aria-label={$t('agent.promptPick.title')}
            onclick={() => (view = 'prompt')}
          >
            <Icon name="message-square" size={12} />
          </button>

          {#if pageModule}
            <span
              class="aw-ctx"
              title={$t('assistant.context', { name: pageName })}
              aria-label={$t('assistant.context', { name: pageName })}
            >
              <Icon name="eye" size={13} />
            </span>
          {/if}
        </div>

        <div class="aw-thread" bind:this={threadEl}>
          {#if loadingThread}
            <p class="aw-muted center">{$t('common.loading')}</p>
          {:else if !messages.length && !streaming}
            <div class="aw-empty">
              <Icon name="brain" size={22} />
              <p>{$t('assistant.emptyHint')}</p>
              {#if booted && !ready}
                <a href="/agent">{$t('assistant.notReady')}</a>
              {/if}
            </div>
          {:else}
            <div class="aw-msgs">
              {#each messages as m (m.id)}
                <MessageView role={m.role} text={m.text} thinking={m.thinking} tools={m.tools ?? []} />
              {/each}
              {#if streaming}
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
                  <div class="aw-confirm" class:destructive={pendingWrite.destructive} role="alertdialog">
                    <div class="aw-confirm-head">
                      <Icon name={pendingWrite.destructive ? 'alert-triangle' : 'lock'} size={13} />
                      <strong>
                        {pendingWrite.destructive
                          ? $t('agent.confirmWrite.titleDestructive')
                          : $t('agent.confirmWrite.title')}
                      </strong>
                    </div>
                    <code class="aw-confirm-call">{pendingWrite.method} {pendingWrite.path}</code>
                    {#if pendingWrite.body != null}
                      <pre class="aw-confirm-body">{JSON.stringify(pendingWrite.body, null, 2)}</pre>
                    {/if}
                    <div class="aw-confirm-actions">
                      <Button size="sm" variant="primary" loading={deciding} onclick={() => decideWrite(true)}>
                        {$t('agent.confirmWrite.approve')}
                      </Button>
                      <Button size="sm" variant="ghost" onclick={() => decideWrite(false)}>
                        {$t('agent.confirmWrite.decline')}
                      </Button>
                    </div>
                  </div>
                {:else if waiting}
                  <div class="aw-working" role="status" aria-live="polite">
                    <span class="aw-dots"><i></i><i></i><i></i></span>
                    <span>{$t('agent.chat.working')}…</span>
                  </div>
                {/if}
              {/if}
            </div>
          {/if}
        </div>

        {#if error}
          <div class="aw-err" role="alert">
            <Icon name="alert-triangle" size={13} />
            <span>{error}</span>
            <button class="aw-icon" title={$t('common.dismiss')} onclick={() => (error = '')}>
              <Icon name="x" size={12} />
            </button>
          </div>
        {/if}

        <div class="aw-composer">
          <textarea
            bind:this={composerEl}
            bind:value={input}
            rows="1"
            placeholder={ready ? $t('agent.chat.composerPh') : $t('agent.chat.composerNotReady')}
            onkeydown={onKeydown}
            oninput={autogrow}
            disabled={booted && !ready}
          ></textarea>
          {#if streaming}
            <button class="aw-send stop" title={$t('agent.chat.stop')} onclick={stop}>
              <Icon name="square" size={14} />
            </button>
          {:else}
            <button
              class="aw-send"
              title={$t('agent.chat.send')}
              onclick={send}
              disabled={!input.trim() || !ready}
            >
              <Icon name="send" size={14} />
            </button>
          {/if}
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .aw {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    /* Above sticky chrome, below modals: the assistant floats over pages, never over a
       dialog the user is answering. */
    z-index: calc(var(--z-modal) - 10);
    font-family: var(--font);
  }

  /* ── Collapsed: one quiet button ─────────────────────────────────────────── */
  .aw-fab {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
  }
  .aw-fab {
    overflow: hidden;
    isolation: isolate;
  }
  .aw-fab:hover {
    color: var(--accent);
  }
  /* A light running once round the edge every 10 s: a rotating conic sweep, blurred, with
     the face laid back over it so only the rim shows. */
  .aw-fab::before {
    content: '';
    position: absolute;
    inset: -60%;
    background: conic-gradient(
      from 0deg,
      transparent 0deg 300deg,
      var(--accent) 345deg,
      transparent 360deg
    );
    filter: blur(4px);
    animation: aw-orbit 10s ease-in-out infinite;
  }
  .aw-fab::after {
    content: '';
    position: absolute;
    inset: 2px;
    border-radius: calc(var(--radius) - 1px);
    background: var(--surface);
  }
  .aw-fab:hover::after {
    background: var(--surface-2);
  }
  .aw-fab :global(svg),
  .aw-fab .aw-dot {
    position: relative;
    z-index: 1;
  }
  @keyframes aw-orbit {
    0% {
      transform: rotate(0deg);
    }
    22% {
      transform: rotate(360deg);
    }
    100% {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .aw-fab::before {
      animation: none;
      opacity: 0;
    }
  }
  .aw-dot {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    animation: aw-pulse 1.2s ease-in-out infinite;
  }
  @keyframes aw-pulse {
    50% {
      opacity: 0.35;
    }
  }

  /* ── Expanded panel ──────────────────────────────────────────────────────── */
  .aw-panel {
    display: flex;
    flex-direction: column;
    width: min(400px, calc(100vw - 2 * var(--space-4)));
    height: min(600px, calc(100vh - 2 * var(--space-8)));
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.24);
    overflow: hidden;
  }

  .aw-head {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  .aw-brand {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .aw-spacer {
    flex: 1;
  }
  .aw-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    border-radius: var(--radius);
  }
  .aw-icon:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .aw-icon:disabled {
    opacity: 0.4;
    cursor: default;
  }
  /* Picker view: one back row, then the panel body. */
  .aw-viewhead {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--text-sm);
  }
  .aw-view {
    flex: 1;
    min-height: 0;
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
  }
  .aw-view > :global(*) {
    flex: 1;
    min-height: 0;
  }

  /* ── Toolbar: persona / model / prompts / page context ───────────────────── */
  .aw-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    background: var(--surface-2);
  }
  .aw-select {
    width: 112px;
    flex: none;
  }
  .aw-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    /* Same height as the persona Dropdown next to it. */
    height: var(--control-h);
    flex: none;
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  /* The model name is the one label worth the leftover width. */
  .aw-chip.model {
    flex: 1;
    min-width: 0;
    justify-content: flex-start;
  }
  .aw-chip:hover:not(:disabled) {
    color: var(--text);
  }
  .aw-chip:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .aw-chip-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .aw-dot-sm {
    width: 5px;
    height: 5px;
    flex: none;
    border-radius: 50%;
    background: var(--accent);
  }
  /* Icon only: the page it watches is a whisper, not a label — the name is in the tooltip. */
  .aw-ctx {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: var(--control-h);
    flex: none;
    color: var(--muted);
    cursor: default;
  }
  .aw-ctx:hover {
    color: var(--text);
  }

  /* ── Thread ──────────────────────────────────────────────────────────────── */
  .aw-thread {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-3);
  }
  .aw-msgs {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .aw-empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
    text-align: center;
    padding: var(--space-4);
  }
  .aw-empty a {
    color: var(--accent);
  }
  .aw-muted {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .aw-muted.center {
    text-align: center;
  }

  .aw-working {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
    padding-left: var(--space-3);
  }
  .aw-dots i {
    display: inline-block;
    width: 4px;
    height: 4px;
    margin-right: 3px;
    border-radius: 50%;
    background: var(--muted);
    animation: aw-pulse 1.2s ease-in-out infinite;
  }
  .aw-dots i:nth-child(2) {
    animation-delay: 0.2s;
  }
  .aw-dots i:nth-child(3) {
    animation-delay: 0.4s;
  }

  /* Confirm card — same rules as the Agent page: the exact call, or no consent. */
  .aw-confirm {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-2);
    margin-left: var(--space-3);
    font-size: var(--text-xs);
  }
  .aw-confirm.destructive {
    border-color: var(--red);
  }
  .aw-confirm-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--amber);
  }
  .aw-confirm.destructive .aw-confirm-head {
    color: var(--red);
  }
  .aw-confirm-call {
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .aw-confirm-body {
    max-height: 120px;
    overflow: auto;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    color: var(--muted);
  }
  .aw-confirm-actions {
    display: flex;
    gap: var(--space-2);
  }

  /* ── Error + composer ────────────────────────────────────────────────────── */
  .aw-err {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    border-top: var(--hairline) solid var(--border);
    color: var(--red);
    font-size: var(--text-xs);
  }
  .aw-err span {
    flex: 1;
    overflow-wrap: anywhere;
  }

  .aw-composer {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .aw-composer textarea {
    flex: 1;
    resize: none;
    border: none;
    background: transparent;
    color: var(--text);
    font-family: var(--font);
    font-size: var(--text-sm);
    line-height: 1.45;
    max-height: 120px;
    padding: var(--space-1) 0;
  }
  .aw-composer textarea:focus {
    outline: none;
  }
  .aw-send {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: none;
    border-radius: var(--radius);
    background: var(--accent);
    color: #fff;
    cursor: pointer;
  }
  .aw-send:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .aw-send:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .aw-send.stop {
    background: var(--red);
  }

  @media (max-width: 640px) {
    .aw-panel {
      width: calc(100vw - 2 * var(--space-4));
      height: min(560px, calc(100vh - 2 * var(--space-6)));
    }
  }
</style>
