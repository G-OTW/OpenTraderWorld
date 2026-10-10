<script>
  // Settings → Voice → Speech & shortcut.
  //
  // The voice broker: which engine turns speech into text (the browser's own recognizer, or
  // a server one: a self-hosted Whisper, OpenAI, Groq…), plus the two push-to-talk shortcuts,
  // the language and what happens to a sentence no command claims. Commands themselves
  // live on the next page.
  import { onMount } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ConfigField from '$lib/notifications/ConfigField.svelte';
  import { t } from '$lib/i18n';
  import { voiceApi } from '$lib/voice/api.js';
  import { voice } from '$lib/voice/voice.svelte.js';
  import { browserRecognizer, speak } from '$lib/voice/audio.js';
  import { shortcutFromEvent, shortcutKeys, shortcutUsable } from '$lib/voice/shortcut.js';

  const LANGUAGES = [
    ['', 'voice.lang.auto'],
    ['en', 'English'],
    ['fr', 'Français'],
    ['de', 'Deutsch'],
    ['es', 'Español'],
    ['it', 'Italiano'],
    ['pt', 'Português'],
    ['zh', '中文']
  ];

  // Starting points for a new engine. Every field stays editable; a preset only fills them.
  const PRESETS = [
    {
      id: 'selfhosted',
      icon: 'database',
      kind: 'openai_compat',
      label: 'Self-hosted Whisper',
      base_url: 'http://whisper:8000/v1',
      model: 'Systran/faster-whisper-small',
      key: false
    },
    { id: 'whispercpp', icon: 'audio-lines', kind: 'whisper_cpp', label: 'whisper.cpp server', base_url: 'http://whisper-cpp:8080', model: '', key: false },
    { id: 'openai', icon: 'globe', kind: 'openai_compat', label: 'OpenAI', base_url: 'https://api.openai.com/v1', model: 'gpt-4o-mini-transcribe', key: true },
    { id: 'groq', icon: 'zap', kind: 'openai_compat', label: 'Groq', base_url: 'https://api.groq.com/openai/v1', model: 'whisper-large-v3-turbo', key: true },
    { id: 'custom', icon: 'plug', kind: 'openai_compat', label: '', base_url: '', model: '', key: true }
  ];

  let settings = $state(null);
  let engines = $state([]);
  let loading = $state(true);
  let error = $state('');
  let saving = $state(false);

  const browserSupported = typeof window !== 'undefined' && !!browserRecognizer();
  const secure = typeof window === 'undefined' || window.isSecureContext;

  // Engine test results by id: { ok, ms } | { error }.
  let tests = $state({});
  let testing = $state('');

  // Editor
  let modalOpen = $state(false);
  let editing = $state(null);
  let presetId = $state('selfhosted');
  let fKind = $state('openai_compat');
  let fLabel = $state('');
  let fUrl = $state('');
  let fModel = $state('');
  let fKey = $state('');
  let fVault = $state(null);
  let fClearKey = $state(false);
  let formError = $state('');
  let savingEngine = $state(false);
  let confirmDelete = $state('');

  // Shortcut capture
  let capturing = $state(''); // '' | 'shortcut' | 'dictation_shortcut'
  let captureHint = $state('');

  onMount(reload);

  $effect(() => {
    voice.suspended = !!capturing;
    return () => (voice.suspended = false);
  });

  async function reload() {
    loading = true;
    error = '';
    try {
      const [s, e] = await Promise.all([voiceApi.settings(), voiceApi.engines()]);
      settings = s;
      engines = e.engines ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function save(patch) {
    const next = { ...settings, ...patch };
    saving = true;
    error = '';
    try {
      settings = await voiceApi.saveSettings(next);
      await voice.load();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  function pickEngine(id) {
    if (settings.engine === id) return;
    save({ engine: id });
  }

  function openCreate() {
    editing = null;
    applyPreset('selfhosted');
    fKey = '';
    fVault = null;
    fClearKey = false;
    formError = '';
    modalOpen = true;
  }

  function openEdit(e) {
    editing = e;
    presetId = 'custom';
    fKind = e.kind;
    fLabel = e.label;
    fUrl = e.base_url;
    fModel = e.model;
    fKey = '';
    fVault = e.api_key_vault_item ?? null;
    fClearKey = false;
    formError = '';
    modalOpen = true;
  }

  function applyPreset(id) {
    const p = PRESETS.find((x) => x.id === id);
    presetId = id;
    fKind = p.kind;
    fLabel = p.label;
    fUrl = p.base_url;
    fModel = p.model;
  }

  const preset = $derived(PRESETS.find((p) => p.id === presetId));

  async function saveEngine() {
    savingEngine = true;
    formError = '';
    try {
      const payload = {
        kind: fKind,
        label: fLabel,
        base_url: fUrl,
        model: fModel,
        clear_key: fClearKey,
        ...(fVault ? { api_key_vault_item: fVault } : fKey ? { api_key: fKey } : {})
      };
      const saved = editing
        ? await voiceApi.updateEngine(editing.id, payload)
        : await voiceApi.addEngine(payload);
      modalOpen = false;
      await reload();
      // The first engine anyone adds is the one they mean to use.
      if (!editing && !settings.engine) await save({ engine: saved.id });
      runTest(saved.id);
    } catch (e) {
      formError = e.message;
    } finally {
      savingEngine = false;
    }
  }

  async function removeEngine(e) {
    if (confirmDelete !== e.id) {
      confirmDelete = e.id;
      return;
    }
    confirmDelete = '';
    try {
      await voiceApi.deleteEngine(e.id);
      await reload();
      await voice.load();
    } catch (err) {
      error = err.message;
    }
  }

  async function runTest(id) {
    testing = id;
    try {
      const r = await voiceApi.testEngine(id);
      tests = { ...tests, [id]: { ok: true, ms: r.ms } };
    } catch (e) {
      tests = { ...tests, [id]: { error: e.message } };
    } finally {
      testing = '';
    }
  }

  function startCapture(field) {
    capturing = field;
    captureHint = '';
  }

  function onCaptureKey(e) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      capturing = '';
      return;
    }
    const sc = shortcutFromEvent(e);
    if (!sc) return; // a modifier on its own: wait for the key
    if (!shortcutUsable(sc)) {
      captureHint = $t('voice.settings.shortcutNeedsMod');
      return;
    }
    const other = capturing === 'shortcut' ? settings.dictation_shortcut : settings.shortcut;
    if (sc === other) {
      captureHint = $t('voice.settings.shortcutTaken');
      return;
    }
    const field = capturing;
    capturing = '';
    captureHint = '';
    save({ [field]: sc });
  }

  const kindLabel = (k) => (k === 'whisper_cpp' ? 'whisper.cpp' : $t('voice.engine.kindOpenai'));
</script>

<svelte:window onkeydowncapture={onCaptureKey} />

<div class="section">
  <header class="head">
    <div class="heading">
      <h2>{$t('voice.settings.title')}</h2>
      <p class="muted small">{$t('voice.settings.subtitle')}</p>
    </div>
    {#if settings}
      <label class="switch" class:on={settings.enabled}>
        <input
          type="checkbox"
          checked={settings.enabled}
          disabled={saving}
          onchange={() => save({ enabled: !settings.enabled })}
        />
        <span>{settings.enabled ? $t('voice.settings.on') : $t('voice.settings.off')}</span>
      </label>
    {/if}
  </header>

  <ErrorText {error} />

  {#if !secure}
    <div class="banner warn">
      <span class="banner-icon" aria-hidden="true"><Icon name="lock" size={16} /></span>
      <p>{$t('voice.settings.insecure')}</p>
    </div>
  {/if}

  {#if loading}
    <div class="card"><Skeleton height="8rem" /></div>
  {:else if settings}
    <!-- 1. Engine -->
    <section class="card">
      <div class="card-head">
        <div>
          <h3><span class="num">1</span>{$t('voice.settings.engineTitle')}</h3>
          <p class="muted small">{$t('voice.settings.engineHint')}</p>
        </div>
        <button class="primary" onclick={openCreate}>
          <Icon name="plus" size={13} /> {$t('voice.settings.addEngine')}
        </button>
      </div>

      <div class="engines" role="radiogroup" aria-label={$t('voice.settings.engineTitle')}>
        <div
          class="engine"
          class:selected={settings.engine === 'browser'}
          class:disabled={!browserSupported}
          role="radio"
          tabindex="0"
          aria-checked={settings.engine === 'browser'}
          aria-disabled={!browserSupported}
          onclick={() => browserSupported && pickEngine('browser')}
          onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && browserSupported && (e.preventDefault(), pickEngine('browser'))}
        >
          <span class="dot" aria-hidden="true"></span>
          <span class="eicon"><Icon name="globe" size={16} /></span>
          <div class="ebody">
            <div class="etitle">
              {$t('voice.engine.browser')}
              {#if !browserSupported}
                <Badge tone="warn">{$t('voice.engine.notHere')}</Badge>
              {:else if settings.engine === 'browser'}
                <Badge tone="accent">{$t('voice.engine.inUse')}</Badge>
              {/if}
            </div>
            <p class="muted small">{$t('voice.engine.browserHint')}</p>
          </div>
        </div>

        {#each engines as e (e.id)}
          {@const test = tests[e.id]}
          <div
            class="engine"
            class:selected={settings.engine === e.id}
            role="radio"
            tabindex="0"
            aria-checked={settings.engine === e.id}
            onclick={() => pickEngine(e.id)}
            onkeydown={(ev) => (ev.key === 'Enter' || ev.key === ' ') && ev.target === ev.currentTarget && (ev.preventDefault(), pickEngine(e.id))}
          >
            <span class="dot" aria-hidden="true"></span>
            <span class="eicon"><Icon name={e.kind === 'whisper_cpp' ? 'audio-lines' : 'database'} size={16} /></span>
            <div class="ebody">
              <div class="etitle">
                {e.label}
                {#if settings.engine === e.id}<Badge tone="accent">{$t('voice.engine.inUse')}</Badge>{/if}
                {#if test?.ok}
                  <Badge tone="success" icon="check">{$t('voice.engine.ok', { ms: test.ms })}</Badge>
                {:else if test?.error}
                  <Badge tone="danger" icon="alert-triangle">{$t('voice.engine.failing')}</Badge>
                {/if}
              </div>
              <p class="muted small mono">
                {kindLabel(e.kind)} · {e.base_url}{e.model ? ` · ${e.model}` : ''}{e.has_key ? ` · ${$t('voice.engine.keySet')}` : ''}
              </p>
              {#if test?.error}<p class="test-err small">{test.error}</p>{/if}
            </div>
            <div class="eactions">
              <button class="ghost" disabled={testing === e.id} onclick={(ev) => (ev.stopPropagation(), runTest(e.id))}>
                <Icon name="play" size={12} /> {testing === e.id ? $t('voice.engine.testing') : $t('voice.engine.test')}
              </button>
              <button class="ghost" onclick={(ev) => (ev.stopPropagation(), openEdit(e))}>
                <Icon name="pencil" size={12} /> {$t('common.edit')}
              </button>
              <button
                class="ghost danger"
                class:armed={confirmDelete === e.id}
                onclick={(ev) => (ev.stopPropagation(), removeEngine(e))}
              >
                <Icon name="trash" size={12} />
                {confirmDelete === e.id ? $t('voice.engine.deleteConfirm') : $t('common.delete')}
              </button>
            </div>
          </div>
        {/each}
      </div>

      {#if !engines.length}
        <p class="muted small selfhost">
          <Icon name="info" size={13} />
          {$t('voice.settings.selfHostHint')}
          <a href="https://g-otw.github.io/OpenTraderWorld/config/voice#whisper-example" target="_blank" rel="noopener">
            g-otw.github.io/OpenTraderWorld/config/voice
          </a>
        </p>
      {/if}
    </section>

    <!-- 2. Talking -->
    <section class="card">
      <div class="card-head">
        <div>
          <h3><span class="num">2</span>{$t('voice.settings.talkTitle')}</h3>
          <p class="muted small">{$t('voice.settings.talkHint')}</p>
        </div>
      </div>
      <div class="rows">
        {#snippet shortcutPicker(field)}
          <div class="picker">
            {#if capturing === field}
              <span class="capture" aria-live="polite">
                <Icon name="keyboard" size={14} /> {$t('voice.settings.pressKeys')}
              </span>
              <button class="ghost" onclick={() => (capturing = '')}>{$t('common.cancel')}</button>
            {:else}
              <span class="keys">
                {#each shortcutKeys(settings[field]) as k (k)}<kbd>{k}</kbd>{/each}
              </span>
              <button class="ghost" onclick={() => startCapture(field)}>{$t('voice.settings.change')}</button>
            {/if}
          </div>
        {/snippet}

        <div class="modes">
          <div class="modecard">
            <span class="mtag cmd"><Icon name="mic" size={12} /> {$t('voice.mode.command')}</span>
            <p class="small">{$t('voice.settings.modeCommand')}</p>
            {@render shortcutPicker('shortcut')}
          </div>
          <div class="modecard">
            <span class="mtag dict"><Icon name="type" size={12} /> {$t('voice.mode.dictation')}</span>
            <p class="small">{$t('voice.settings.modeDictation')}</p>
            {@render shortcutPicker('dictation_shortcut')}
          </div>
        </div>
        {#if captureHint}<p class="test-err small">{captureHint}</p>{/if}
        <p class="muted small">{$t('voice.settings.shortcutHint')}</p>

        <div class="row">
          <div class="rlabel">
            <span>{$t('voice.settings.language')}</span>
            <p class="muted small">{$t('voice.settings.languageHint')}</p>
          </div>
          <div class="rctl narrow">
            <Dropdown
              value={settings.language}
              ariaLabel={$t('voice.settings.language')}
              options={LANGUAGES.map(([v, l]) => ({ value: v, label: v ? l : $t(l) }))}
              onpick={(v) => save({ language: v })}
            />
          </div>
        </div>
      </div>
    </section>

    <!-- 3. Behaviour -->
    <section class="card">
      <div class="card-head">
        <div>
          <h3><span class="num">3</span>{$t('voice.settings.behaviourTitle')}</h3>
        </div>
      </div>
      <div class="rows">
        <label class="row toggle">
          <div class="rlabel">
            <span>{$t('voice.settings.agentFallback')}</span>
            <p class="muted small">{$t('voice.settings.agentFallbackHint')}</p>
          </div>
          <input
            type="checkbox"
            checked={settings.agent_fallback}
            disabled={saving}
            onchange={() => save({ agent_fallback: !settings.agent_fallback })}
          />
        </label>
        <div class="row toggle">
          <label class="rlabel" for="voice-speak">
            <span>{$t('voice.settings.speak')}</span>
            <p class="muted small">{$t('voice.settings.speakHint')}</p>
          </label>
          <div class="rctl">
            <button class="ghost" onclick={() => speak($t('voice.say.done'), settings.language)}>
              <Icon name="volume-2" size={12} /> {$t('voice.settings.listen')}
            </button>
            <input
              id="voice-speak"
              type="checkbox"
              checked={settings.speak}
              disabled={saving}
              onchange={() => save({ speak: !settings.speak })}
            />
          </div>
        </div>
      </div>
    </section>

    <aside class="notes-card">
      <span class="notes-icon" aria-hidden="true"><Icon name="shield" size={17} /></span>
      <ul class="notes muted small">
        <li>{$t('voice.settings.note1')}</li>
        <li>{$t('voice.settings.note2')}</li>
        <li>{$t('voice.settings.note3')}</li>
      </ul>
    </aside>
  {/if}
</div>

<Modal bind:open={modalOpen} title={editing ? $t('voice.engine.editTitle') : $t('voice.engine.newTitle')} size="md">
  <div class="form">
    {#if !editing}
      <div class="presets" role="radiogroup" aria-label={$t('voice.engine.preset')}>
        {#each PRESETS as p (p.id)}
          <button
            type="button"
            class="preset"
            class:on={presetId === p.id}
            role="radio"
            aria-checked={presetId === p.id}
            onclick={() => applyPreset(p.id)}
          >
            <Icon name={p.icon} size={15} />
            <span>{p.label || $t('voice.engine.custom')}</span>
          </button>
        {/each}
      </div>
      <p class="muted small">{$t(`voice.engine.presetHint.${presetId}`)}</p>
    {/if}

    <label class="field">
      <span>{$t('voice.engine.name')}</span>
      <input type="text" bind:value={fLabel} maxlength="80" placeholder={$t('voice.engine.namePlaceholder')} />
    </label>

    <label class="field">
      <span>{$t('voice.engine.kind')}</span>
      <Dropdown
        bind:value={fKind}
        ariaLabel={$t('voice.engine.kind')}
        options={[
          { value: 'openai_compat', label: $t('voice.engine.kindOpenai') },
          { value: 'whisper_cpp', label: 'whisper.cpp' }
        ]}
      />
    </label>

    <label class="field">
      <span>{$t('voice.engine.url')}</span>
      <input type="url" bind:value={fUrl} placeholder={fKind === 'whisper_cpp' ? 'http://whisper-cpp:8080' : 'http://whisper:8000/v1'} />
    </label>

    {#if fKind === 'openai_compat'}
      <label class="field">
        <span>{$t('voice.engine.model')}</span>
        <input type="text" bind:value={fModel} placeholder="whisper-1" />
      </label>
    {/if}

    <div class="field">
      <span>
        {$t('voice.engine.key')}
        {#if !preset?.key && !editing}<em class="muted">{$t('voice.engine.keyOptional')}</em>{/if}
      </span>
      <ConfigField
        bind:value={fKey}
        bind:vaultItem={fVault}
        type="password"
        placeholder={editing?.has_key ? $t('voice.engine.keyKeep') : $t('voice.engine.keyPlaceholder')}
      />
    </div>
    {#if editing?.has_key}
      <label class="check">
        <input type="checkbox" bind:checked={fClearKey} />
        <span>{$t('voice.engine.clearKey')}</span>
      </label>
    {/if}

    <ErrorText error={formError} />
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (modalOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={savingEngine} onclick={saveEngine}>
      {savingEngine ? $t('common.saving') : $t('voice.engine.saveTest')}
    </button>
  {/snippet}
</Modal>

<style>
  .section {
    display: flex;
    width: min(100%, 980px);
    flex-direction: column;
    gap: var(--space-4);
  }
  .section h2 {
    margin: 0;
    color: var(--text);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-6);
  }
  .heading {
    display: flex;
    max-width: 72ch;
    flex-direction: column;
    gap: 6px;
  }
  .heading p,
  .card-head p,
  .rlabel p,
  .modecard p {
    margin: 0;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: var(--fs-desc);
    line-height: 1.5;
  }
  .mono {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }
  .switch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 116px;
    min-height: 42px;
    justify-content: center;
    flex: none;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--muted);
    cursor: pointer;
  }
  .switch.on {
    border-color: color-mix(in srgb, var(--green) 45%, var(--border-control));
    background: color-mix(in srgb, var(--green) 7%, var(--surface));
    color: var(--green-ink, var(--green));
  }
  .banner {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-left: var(--active-rule, 2px) solid var(--amber);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: 11px 14px;
    color: var(--muted);
  }
  .banner p {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.5;
  }
  .banner-icon {
    display: inline-flex;
    margin-top: 2px;
    color: var(--amber-ink, var(--amber));
  }
  .card {
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
  }
  .card-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }
  .card-head h3 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 4px;
    color: var(--text);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .num {
    display: inline-flex;
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
    color: var(--accent);
    font-size: var(--text-xs);
  }
  .engines {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .engine {
    display: grid;
    grid-template-columns: 16px 34px 1fr auto;
    align-items: center;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    cursor: pointer;
    transition: border-color 120ms, background 120ms;
  }
  .engine:hover {
    border-color: var(--border-strong, var(--border-control));
  }
  .engine:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .engine.selected {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border-control));
    background: color-mix(in srgb, var(--accent) 5%, var(--surface));
  }
  .engine.disabled {
    cursor: default;
    opacity: 0.6;
  }
  .dot {
    width: 14px;
    height: 14px;
    border: 1.5px solid var(--border-control);
    border-radius: 50%;
  }
  .engine.selected .dot {
    border: 4px solid var(--accent);
  }
  .eicon {
    display: inline-flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
  }
  .ebody {
    min-width: 0;
  }
  .ebody p {
    margin: 2px 0 0;
  }
  .etitle {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .eactions {
    display: flex;
    gap: 2px;
  }
  .eactions button.armed {
    border-color: color-mix(in srgb, var(--red) 50%, var(--border-control));
    color: var(--red);
  }
  .test-err {
    margin: 4px 0 0;
    color: var(--red);
    overflow-wrap: anywhere;
  }
  .selfhost {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: var(--space-3) 0 0;
  }
  .selfhost a {
    color: var(--accent);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .row.toggle {
    cursor: pointer;
  }
  .row.toggle input[type='checkbox'] {
    width: auto;
  }
  .rlabel {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--text);
  }
  .rctl {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
  }
  .rctl.narrow {
    width: 220px;
  }
  .keys {
    display: inline-flex;
    gap: 4px;
  }
  kbd {
    min-width: 28px;
    border: var(--hairline) solid var(--border-control);
    border-bottom-width: 2px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    padding: 3px 8px;
    color: var(--text);
    font-family: inherit;
    font-size: var(--text-sm);
    text-align: center;
  }
  .capture {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: var(--hairline) dashed var(--accent);
    border-radius: var(--radius-sm);
    padding: 5px 10px;
    color: var(--accent);
    font-size: var(--text-sm);
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.55;
    }
  }
  .modes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  .picker {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin-top: auto;
    padding-top: var(--space-1);
  }
  .modecard {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-3);
    color: var(--muted);
  }
  .mtag {
    display: inline-flex;
    align-self: flex-start;
    align-items: center;
    gap: 5px;
    border-radius: 999px;
    padding: 2px 9px;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .mtag.cmd {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    color: var(--accent);
  }
  .mtag.dict {
    background: color-mix(in srgb, var(--amber) 14%, var(--surface));
    color: var(--amber-ink, var(--amber));
  }
  .notes-card {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 72%, var(--surface));
    padding: var(--space-3) var(--space-4);
  }
  .notes-icon {
    display: inline-flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    flex: none;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin: 1px 0 0;
    padding-left: 18px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .form > p {
    margin: 0;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: var(--text-base);
  }
  .field em {
    margin-left: 6px;
    font-style: normal;
    font-size: var(--text-xs);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .check input {
    width: auto;
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-2);
  }
  .preset {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 9px 11px;
    color: var(--muted);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .preset:hover {
    color: var(--text);
  }
  .preset.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    color: var(--text);
  }
  @media (max-width: 760px) {
    .head,
    .card-head,
    .row {
      flex-direction: column;
      align-items: flex-start;
      gap: var(--space-3);
    }
    .engine {
      grid-template-columns: 16px 1fr;
    }
    .eicon {
      display: none;
    }
    .eactions {
      grid-column: 1 / -1;
      flex-wrap: wrap;
    }
    .modes {
      grid-template-columns: 1fr;
    }
    .rctl.narrow {
      width: 100%;
    }
  }
</style>
