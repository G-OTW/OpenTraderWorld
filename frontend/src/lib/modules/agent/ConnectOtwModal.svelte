<script>
  // One-click connect for the built-in OpenTraderWorld gateway. Collapses what would
  // otherwise be a trip to Settings → AI agents into a single action: flip the global
  // `mcp_enabled` switch if it is off, mint a bearer token with the chosen per-module
  // permissions, then show that token once (the backend only stores its hash).
  //
  // Defaults to read-only on every module: the safe grant for "let the agent look at my
  // data". Write and delete stay opt-in — per module via the dropdowns, or in bulk via
  // the same shortcut row the settings modal offers.
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { settingsApi } from '$lib/settings/api.js';

  let { open = $bindable(false), ondone = () => {} } = $props();

  let modules = $state([]);
  let perms = $state({});
  let name = $state('');
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let wasEnabled = $state(true);

  // Set once the token exists: the modal switches to the copy-it-now view.
  let token = $state('');
  let copied = $state(false);

  const endpoint = $derived(
    typeof location !== 'undefined' ? `${location.origin}/api/mcp` : '/api/mcp'
  );

  // Load the module list when the modal opens; reset any previous run. Only `open` is
  // read reactively — untrack the rest so a language change can't retrigger the fetch.
  let lastOpen = false;
  $effect(() => {
    const isOpen = open;
    if (isOpen === lastOpen) return;
    lastOpen = isOpen;
    if (!isOpen) return;
    untrack(() => {
      token = '';
      copied = false;
      error = '';
      name = $t('agent.mcp.connect.defaultName');
      load();
    });
  });

  async function load() {
    loading = true;
    try {
      const s = await settingsApi.mcpSettings();
      wasEnabled = !!s.enabled;
      modules = s.modules ?? [];
      // Read on everything by default.
      perms = Object.fromEntries(modules.map((m) => [m.id, 'r']));
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function setLevel(id, level) {
    const next = { ...perms };
    if (level) next[id] = level;
    else delete next[id];
    perms = next;
  }
  function setAll(level) {
    perms = level ? Object.fromEntries(modules.map((m) => [m.id, level])) : {};
  }

  const granted = $derived(Object.keys(perms).length);

  async function connect() {
    if (!name.trim()) {
      error = $t('settings.mcp.err.nameRequired');
      return;
    }
    if (!granted) {
      error = $t('agent.mcp.connect.errNoPerms');
      return;
    }
    busy = true;
    error = '';
    try {
      // Enable the endpoint first — a token is useless while the gateway is off.
      if (!wasEnabled) {
        const r = await settingsApi.setMcpEnabled(true);
        wasEnabled = !!r.enabled;
      }
      const r = await settingsApi.createMcpToken({ name: name.trim(), permissions: perms });
      token = r.token;
      ondone();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(token);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch {
      /* clipboard blocked — the value stays selectable on screen */
    }
  }
</script>

<Modal bind:open title={$t('agent.mcp.connect.title')} size="md">
  {#if token}
    <div class="done">
      <p class="ok"><Icon name="check-circle" size={15} /> {$t('agent.mcp.connect.ready')}</p>
      <p class="warn"><Icon name="alert-triangle" size={13} /> {$t('agent.mcp.connect.once')}</p>

      <div class="field">
        <span>{$t('agent.mcp.connect.endpoint')}</span>
        <code class="mono">{endpoint}</code>
      </div>

      <div class="field">
        <span>{$t('agent.mcp.connect.token')}</span>
        <div class="tokrow">
          <code class="mono tok">{token}</code>
          <Button size="sm" icon={copied ? 'check' : 'copy'} onclick={copy}>
            {copied ? $t('common.copied') : $t('common.copy')}
          </Button>
        </div>
      </div>
    </div>
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <div class="form">
      <p class="muted">{$t('agent.mcp.connect.intro')}</p>

      <label class="field">
        <span>{$t('settings.mcp.name')}</span>
        <input type="text" bind:value={name} maxlength="80" />
      </label>

      <div class="permhead">
        <span>{$t('settings.mcp.permissions')}</span>
        <span class="bulk">
          <button class="ghost" onclick={() => setAll('r')}>{$t('settings.mcp.allRead')}</button>
          <button class="ghost" onclick={() => setAll('rw')}>{$t('settings.mcp.allRw')}</button>
          <button class="ghost" onclick={() => setAll('rwd')}>{$t('settings.mcp.allFull')}</button>
          <button class="ghost" onclick={() => setAll('')}>{$t('common.clear')}</button>
        </span>
      </div>
      <div class="matrix">
        {#each modules as m (m.id)}
          <div class="permrow">
            <span class="mlabel">{m.label}</span>
            <select value={perms[m.id] ?? ''} onchange={(e) => setLevel(m.id, e.currentTarget.value)}>
              <option value="">{$t('settings.mcp.levelNone')}</option>
              <option value="r">{$t('settings.mcp.levelRead')}</option>
              <option value="rw">{$t('settings.mcp.levelRw')}</option>
              <option value="rwd">{$t('settings.mcp.levelFull')}</option>
            </select>
          </div>
        {/each}
      </div>

      {#if !wasEnabled}
        <p class="note"><Icon name="zap" size={12} /> {$t('agent.mcp.connect.willEnable')}</p>
      {/if}
    </div>
  {/if}

  <ErrorText {error} />

  {#snippet footer()}
    {#if token}
      <Button variant="primary" onclick={() => (open = false)}>{$t('common.done')}</Button>
    {:else}
      <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
      <Button variant="primary" loading={busy} disabled={loading} onclick={connect}>
        {$t('agent.mcp.connect.action')}
      </Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form,
  .done {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field > span {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .field input {
    background: var(--surface-2);
    border: 1px solid var(--border-control, var(--border));
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 8px;
    font: inherit;
  }
  .permhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
    flex-wrap: wrap;
  }
  .bulk {
    display: flex;
    gap: var(--space-1);
  }
  .ghost {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 2px 8px;
  }
  .ghost:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .matrix {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 260px;
    overflow-y: auto;
  }
  .permrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 4px 0;
  }
  .mlabel {
    color: var(--text);
    font-size: var(--text-sm);
  }
  .permrow select {
    background: var(--surface-2);
    border: 1px solid var(--border-control, var(--border));
    border-radius: var(--radius);
    color: var(--text);
    padding: 3px 6px;
    font-size: var(--text-sm);
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--text-xs);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 8px;
    color: var(--text);
    word-break: break-all;
    user-select: all;
  }
  .tokrow {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .tok {
    flex: 1;
    min-width: 0;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--green);
    font-size: var(--text-sm);
  }
  .warn,
  .note {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--amber);
    font-size: var(--text-xs);
  }
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>
