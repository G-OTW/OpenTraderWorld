<script>
  // External notification channels for RemindMe. The user plugs in their own accounts
  // (email/telegram/slack/discord); a fired reminder is pushed to every channel they have
  // explicitly enabled. Secrets are write-only — never returned by the API, so an existing
  // channel shows "secret set" and only replaces it if the user types a new one.
  import { remindApi, CHANNEL_KINDS, channelMeta } from './api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let channels = $state([]);
  let loading = $state(true);

  // Editor state: null = list view; otherwise the draft being added/edited.
  let draft = $state(null);
  let draftId = $state(null); // null when adding
  let secretInput = $state(''); // typed secret (blank keeps existing on edit)
  let saving = $state(false);
  let testMsg = $state(null); // { id, ok, text }
  let error = $state(null);

  async function load() {
    channels = await remindApi.channels();
    loading = false;
  }
  load();

  function startAdd(kind) {
    const meta = channelMeta(kind);
    draftId = null;
    secretInput = '';
    error = null;
    const config = {};
    for (const f of meta.fields) config[f.key] = '';
    draft = { kind, name: meta.label, config, enabled: false };
  }

  function startEdit(ch) {
    draftId = ch.id;
    secretInput = '';
    error = null;
    draft = {
      kind: ch.kind,
      name: ch.name,
      config: { ...(ch.config ?? {}) },
      enabled: ch.enabled,
      has_secret: ch.has_secret
    };
  }

  function cancel() {
    draft = null;
    draftId = null;
    error = null;
  }

  const draftMeta = $derived(draft ? channelMeta(draft.kind) : null);

  async function save() {
    if (!draft) return;
    error = null;
    // A brand-new channel needs a secret; editing may leave it blank to keep the old one.
    if (draftId === null && !secretInput.trim()) {
      error = $t('remindme.channels.secretRequired');
      return;
    }
    saving = true;
    try {
      const payload = {
        kind: draft.kind,
        name: draft.name?.trim() || draftMeta.label,
        config: draft.config,
        enabled: draft.enabled
      };
      // Only send `secret` when the user typed one (blank on edit = untouched).
      if (secretInput.trim()) payload.secret = secretInput.trim();
      if (draftId === null) await remindApi.addChannel(payload);
      else await remindApi.updateChannel(draftId, payload);
      await load();
      cancel();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function toggleEnabled(ch) {
    // Enabling requires a secret to be present.
    if (!ch.enabled && !ch.has_secret) {
      testMsg = { id: ch.id, ok: false, text: $t('remindme.channels.needSecretFirst') };
      return;
    }
    await remindApi.updateChannel(ch.id, {
      kind: ch.kind,
      name: ch.name,
      config: ch.config,
      enabled: !ch.enabled
    });
    await load();
  }

  // Deleting a channel is not undoable — ConfirmModal, not the browser's confirm().
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function remove(ch) {
    pendingDelete = ch;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const ch = pendingDelete;
    pendingDelete = null;
    if (!ch) return;
    await remindApi.removeChannel(ch.id);
    await load();
  }

  async function test(ch) {
    testMsg = { id: ch.id, ok: null, text: $t('remindme.channels.testing') };
    try {
      await remindApi.testChannel(ch.id);
      testMsg = { id: ch.id, ok: true, text: $t('remindme.channels.testOk') };
    } catch (e) {
      testMsg = { id: ch.id, ok: false, text: e.message };
    }
    await load();
  }

  const kindLabel = (id) => channelMeta(id)?.label ?? id;
</script>

<div class="ch">
  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if draft}
    <!-- Add / edit form -->
    <div class="form">
      <div class="form-head">
        <button class="back" onclick={cancel}><Icon name="chevron-left" size={14} /> {$t('common.back')}</button>
        <span class="form-title">{kindLabel(draft.kind)}</span>
      </div>

      <label class="field">
        <span>{$t('remindme.channels.name')}</span>
        <input bind:value={draft.name} placeholder={draftMeta.label} />
      </label>

      {#each draftMeta.fields as f (f.key)}
        <label class="field">
          <span>{f.label}{#if f.required}<em class="req">*</em>{/if}</span>
          <input
            type={f.type === 'number' ? 'number' : 'text'}
            bind:value={draft.config[f.key]}
            placeholder={f.placeholder ?? ''}
          />
        </label>
      {/each}

      <label class="field">
        <span>
          {draftMeta.secret.label}
          {#if draft.has_secret}<em class="hint">· {$t('remindme.channels.secretSet')}</em>{/if}
        </span>
        <input
          type="password"
          autocomplete="new-password"
          bind:value={secretInput}
          placeholder={draft.has_secret ? $t('remindme.channels.leaveBlankKeep') : ''}
        />
        {#if draftMeta.secret.help}<small class="help">{draftMeta.secret.help}</small>{/if}
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={draft.enabled} />
        <span>{$t('remindme.channels.enableSend')}</span>
      </label>

      <ErrorText error={error} />

      <div class="form-actions">
        <button class="ghost" onclick={cancel}>{$t('common.cancel')}</button>
        <button class="primary" onclick={save} disabled={saving}>{$t('common.save')}</button>
      </div>
    </div>
  {:else}
    <!-- List + add buttons -->
    <p class="intro">{$t('remindme.channels.intro')}</p>

    {#if channels.length === 0}
      <div class="empty">{$t('remindme.channels.empty')}</div>
    {:else}
      <ul class="list">
        {#each channels as ch (ch.id)}
          <li class="row" class:on={ch.enabled}>
            <div class="row-main">
              <span class="badge">{kindLabel(ch.kind)}</span>
              <span class="cname">{ch.name}</span>
              {#if !ch.has_secret}<span class="warn">{$t('remindme.channels.noSecret')}</span>{/if}
              {#if ch.last_ok === false && ch.last_error}
                <span class="warn" title={ch.last_error}>{$t('remindme.channels.lastFailed')}</span>
              {/if}
            </div>
            <div class="row-actions">
              <label class="switch" title={$t('remindme.channels.enableSend')}>
                <input type="checkbox" checked={ch.enabled} onchange={() => toggleEnabled(ch)} />
                <span class="slider"></span>
              </label>
              <button class="icon" title={$t('remindme.channels.test')} onclick={() => test(ch)}><Icon name="send" size={14} /></button>
              <button class="icon" title={$t('remindme.edit')} onclick={() => startEdit(ch)}><Icon name="pencil" size={14} /></button>
              <button class="icon danger-hover" title={$t('remindme.delete')} onclick={() => remove(ch)}><Icon name="trash" size={14} /></button>
            </div>
            {#if testMsg && testMsg.id === ch.id}
              <p class="test" class:ok={testMsg.ok === true} class:bad={testMsg.ok === false}>{testMsg.text}</p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <div class="add">
      <span class="add-label">{$t('remindme.channels.add')}</span>
      <div class="add-btns">
        {#each CHANNEL_KINDS as k (k.id)}
          <button class="ghost" onclick={() => startAdd(k.id)}><Icon name="plus" size={13} /> {k.label}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<!-- This component renders inside the parent's Modal; the ConfirmModal comes after it in
     DOM order, so at equal --z-modal it stacks on top. -->
<ConfirmModal
  bind:open={confirmOpen}
  title={$t('remindme.delete')}
  message={pendingDelete ? $t('remindme.channels.confirmDelete', { name: pendingDelete.name }) : ''}
  confirmLabel={$t('remindme.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .ch {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .muted { color: var(--muted); }
  .intro {
    font-size: var(--text-base);
    color: var(--muted);
    margin: 0;
  }
  .empty {
    padding: var(--space-6) var(--space-4);
    text-align: center;
    color: var(--muted);
    border: 1px dashed var(--border);
    border-radius: var(--radius);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 12px;
    background: var(--surface);
  }
  .row.on {
    border-color: var(--accent);
  }
  .row-main {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .badge {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 2px 8px;
  }
  .cname {
    font-weight: var(--fw-semibold);
    font-size: var(--text-base);
  }
  .warn {
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .row-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .icon {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius);
  }
  .icon:hover { color: var(--text); }
  .danger-hover:hover { color: var(--red); }
  .test {
    flex-basis: 100%;
    margin: 4px 0 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .test.ok { color: var(--green); }
  .test.bad { color: var(--red); }

  /* toggle switch */
  .switch {
    position: relative;
    display: inline-block;
    width: 34px;
    height: 18px;
    cursor: pointer;
  }
  .switch input { opacity: 0; width: 0; height: 0; }
  .slider {
    position: absolute;
    inset: 0;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    transition: 0.15s;
  }
  .slider::before {
    content: '';
    position: absolute;
    height: 12px;
    width: 12px;
    left: 2px;
    top: 2px;
    background: var(--muted);
    border-radius: 50%;
    transition: 0.15s;
  }
  .switch input:checked + .slider {
    background: var(--accent);
    border-color: var(--accent);
  }
  .switch input:checked + .slider::before {
    transform: translateX(16px);
    background: var(--accent-contrast);
  }

  .add {
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
  .add-label {
    display: block;
    font-size: var(--text-sm);
    color: var(--muted);
    margin-bottom: var(--space-2);
  }
  .add-btns {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  /* form */
  .form { display: flex; flex-direction: column; gap: var(--space-3); }
  .form-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .back {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-sm);
  }
  .back:hover { color: var(--text); }
  .form-title { font-weight: var(--fw-semibold); }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-sm);
  }
  .field > span { color: var(--muted); }
  .req { color: var(--red); font-style: normal; margin-left: 2px; }
  .hint { color: var(--green); font-style: normal; font-size: var(--text-xs); }
  .help { color: var(--muted); font-size: var(--text-xs); }
  .field input {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 7px 10px;
    font-size: var(--text-base);
  }
  .field input:focus { border-color: var(--accent); outline: none; }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
  }
  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
