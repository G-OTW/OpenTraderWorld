<script>
  // The MCP token create/edit form, in a modal. Settings → MCP opens it from the token
  // table; a module that needs a token (the automator's workflow settings) opens it in place
  // so the user mints one without leaving what they were filling. The parent calls
  // `openCreate()` or `openEdit(tok)` through `bind:this`; `onsaved(result, edited)` gets the
  // create response (with the one-time plaintext `token`) or the edit response.
  import { settingsApi } from '$lib/settings/api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { t } from '$lib/i18n';

  /** `modules`: the permission rows. Left null, they are read from the MCP settings on open. */
  let { modules = null, onsaved } = $props();

  let open = $state(false);
  let editing = $state(null);
  let loaded = $state([]);
  const rows = $derived(modules ?? loaded);
  let formName = $state('');
  let formPerms = $state({});
  // Expiry as a days-from-now choice: '' = leave as it is (edit only), '0' = never.
  let formExpiry = $state('0');
  // Whether this token may be reached from outside the app (a chat binding). Off unless
  // asked for: it is a separate decision from what the token may touch.
  let formExternal = $state(false);
  let formError = $state('');
  let saving = $state(false);

  async function loadModules() {
    if (modules) return;
    try {
      loaded = (await settingsApi.mcpSettings()).modules ?? [];
    } catch (e) {
      formError = e.message;
    }
  }

  export function openCreate() {
    editing = null;
    formName = '';
    formPerms = {};
    formExpiry = '0';
    formExternal = false;
    formError = '';
    open = true;
    loadModules();
  }

  export function openEdit(tok) {
    editing = tok;
    formName = tok.name;
    formPerms = { ...(tok.permissions ?? {}) };
    formExpiry = '';
    formExternal = !!tok.external;
    formError = '';
    open = true;
    loadModules();
  }

  function setLevel(id, level) {
    const next = { ...formPerms };
    if (level) next[id] = level;
    else delete next[id];
    formPerms = next;
  }

  function setAll(level) {
    formPerms = level ? Object.fromEntries(rows.map((m) => [m.id, level])) : {};
  }

  async function save() {
    if (!formName.trim()) {
      formError = $t('settings.mcp.err.nameRequired');
      return;
    }
    saving = true;
    formError = '';
    try {
      // '' means "don't touch the expiry", so the field is left out of the payload.
      const expiry = formExpiry === '' ? {} : { expires_in_days: Number(formExpiry) };
      const wasEdit = !!editing;
      const r = editing
        ? await settingsApi.updateMcpToken(editing.id, {
            name: formName,
            permissions: formPerms,
            external: formExternal,
            ...expiry,
          })
        : await settingsApi.createMcpToken({
            name: formName,
            permissions: formPerms,
            external: formExternal,
            ...expiry,
          });
      open = false;
      await onsaved?.(r, wasEdit);
    } catch (e) {
      formError = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal bind:open title={editing ? $t('settings.mcp.editTitle') : $t('settings.mcp.newToken')} size="md">
  <div class="form">
    <label class="field">
      <span>{$t('settings.mcp.name')}</span>
      <input type="text" bind:value={formName} placeholder={$t('settings.mcp.namePlaceholder')} maxlength="80" />
    </label>

    <label class="field">
      <span>{$t('settings.mcp.expiry')}</span>
      <Dropdown
        value={formExpiry}
        onpick={(v) => (formExpiry = v)}
        ariaLabel={$t('settings.mcp.expiry')}
        options={[
          ...(editing ? [{ value: '', label: $t('settings.mcp.expiryKeep') }] : []),
          { value: '0', label: $t('settings.mcp.expiryNever') },
          { value: '30', label: $t('settings.mcp.expiryDays', { n: 30 }) },
          { value: '90', label: $t('settings.mcp.expiryDays', { n: 90 }) },
          { value: '365', label: $t('settings.mcp.expiryDays', { n: 365 }) }
        ]}
      />
    </label>
    <p class="muted small">{$t('settings.mcp.expiryHint')}</p>

    <label class="check">
      <input type="checkbox" bind:checked={formExternal} />
      <span>{$t('settings.mcp.external')}</span>
    </label>
    <p class="muted small">{$t('settings.mcp.externalHint')}</p>

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
      {#each rows as m (m.id)}
        <div class="permrow">
          <span class="mlabel">{m.label}</span>
          <Dropdown
            value={formPerms[m.id] ?? ''}
            onpick={(v) => setLevel(m.id, v)}
            ariaLabel={m.label}
            options={[
              { value: '', label: $t('settings.mcp.levelNone') },
              { value: 'r', label: $t('settings.mcp.levelRead') },
              { value: 'rw', label: $t('settings.mcp.levelRw') },
              { value: 'rwd', label: $t('settings.mcp.levelFull') }
            ]}
          />
        </div>
      {/each}
    </div>

    <ErrorText error={formError} />
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={saving} onclick={save}>
      {saving ? $t('common.saving') : editing ? $t('common.save') : $t('settings.mcp.create')}
    </button>
  {/snippet}
</Modal>

<style>
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  button.ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: var(--hairline) solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  button.ghost:hover {
    color: var(--text);
  }
  button.primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    color: var(--text);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: 7px 14px;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
    cursor: pointer;
  }
  button.primary:hover:not(:disabled) {
    background: var(--surface-2);
  }
  button.primary:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-base);
  }
  .field input {
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 7px 10px;
    font-size: var(--text-base);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.85rem;
  }
  .check input {
    width: auto;
  }

  .permhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--text-base);
  }
  .bulk {
    display: inline-flex;
    gap: var(--space-2);
  }
  .matrix {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    max-height: 320px;
    overflow-y: auto;
  }
  .permrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 6px 10px;
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
  }
  .permrow:last-child {
    border-bottom: none;
  }
  .permrow :global(.dd) {
    width: 130px;
    flex: none;
    font-size: var(--text-sm);
  }
</style>
