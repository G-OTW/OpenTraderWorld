<script>
  // Connected mailboxes: what is plugged in, whether it is working, and the module's
  // settings. Connecting is a two-step wizard — pick the provider, then fill in what
  // only you can know — because "IMAP host / port / security" is not a question a
  // normal person should be asked cold.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import OauthModal from './OauthModal.svelte';
  import { mailboxApi } from './api.js';
  import { fmtDateTime } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    accounts = [],
    presets = [],
    /** Reload the page-level data (accounts + counts). */
    onreload = () => {}
  } = $props();

  let settings = $state(null);
  let settingsOpen = $state(false);
  let settingsSaving = $state(false);
  let settingsError = $state('');

  // Wizard
  let wizardOpen = $state(false);
  let step = $state('provider'); // 'provider' | 'form'
  let preset = $state(null);
  let editing = $state(null);
  let form = $state(blank());
  let vaultItemId = $state(null);
  let formError = $state('');
  let saving = $state(false);

  let busy = $state('');
  let results = $state({}); // account id → { ok, text }
  let confirmDelete = $state(null);
  // Account currently going through a device-code sign-in.
  let signingIn = $state(null);

  function blank() {
    return {
      name: '',
      email: '',
      preset: 'generic',
      host: '',
      port: 993,
      security: 'ssl',
      username: '',
      folder: 'INBOX',
      interval_secs: 900,
      enabled: true,
      auth_kind: 'password',
      oauth_provider: '',
      oauth_client_id: '',
      oauth_tenant: 'common'
    };
  }

  // True while the form describes an OAuth mailbox (Microsoft): no password field, a
  // client id instead, and a sign-in right after saving.
  const isOauth = $derived(form.auth_kind === 'oauth');

  // Providers where "create an app password" is not the whole story — something to switch
  // on first, a host that depends on the region, a bridge to run. The straightforward ones
  // deliberately get no note: an extra paragraph on every screen teaches nothing.
  const SETUP_NOTES = {
    gmail: 'mailbox.setup.gmail',
    icloud: 'mailbox.setup.icloud',
    yahoo: 'mailbox.setup.yahoo',
    zoho: 'mailbox.setup.zoho',
    'proton-bridge': 'mailbox.setup.protonBridge'
  };
  const setupNote = $derived(!isOauth && preset ? (SETUP_NOTES[preset.id] ?? null) : null);

  // The redirect URI to register in the app registration — this instance's own address.
  // Microsoft only accepts https, or http on loopback; a plain-HTTP LAN address has to
  // fall back to the device code, which most Microsoft 365 tenants now block.
  const redirectUri = `${window.location.origin}/mailbox/oauth`;
  const secure = window.location.protocol === 'https:';
  const redirectOk = secure || ['localhost', '127.0.0.1'].includes(window.location.hostname);
  // The Entra portal's Redirect URIs box refuses an http loopback typed as 127.0.0.1 —
  // only `http://localhost` goes in without hand-editing the application manifest. Same
  // machine either way, so the fix is simply to open the app on localhost.
  const preferLocalhost = !secure && window.location.hostname === '127.0.0.1';
  let redirectCopied = $state(false);

  async function copyRedirect() {
    try {
      await navigator.clipboard.writeText(redirectUri);
      redirectCopied = true;
      setTimeout(() => (redirectCopied = false), 1500);
    } catch {
      /* clipboard unavailable — the URL is on screen anyway */
    }
  }

  $effect(() => {
    loadSettings();
  });

  async function loadSettings() {
    try {
      settings = await mailboxApi.getSettings();
    } catch (e) {
      settingsError = e.message;
    }
  }

  async function saveSettings() {
    settingsSaving = true;
    settingsError = '';
    try {
      settings = await mailboxApi.putSettings(settings);
      settingsOpen = false;
    } catch (e) {
      settingsError = e.message;
    } finally {
      settingsSaving = false;
    }
  }

  function openWizard() {
    editing = null;
    preset = null;
    step = 'provider';
    form = blank();
    vaultItemId = null;
    formError = '';
    wizardOpen = true;
  }

  function pickPreset(p) {
    preset = p;
    form = {
      ...blank(),
      preset: p.id,
      host: p.host,
      port: p.port,
      security: p.security,
      name: p.id === 'generic' ? '' : p.label,
      auth_kind: p.auth === 'oauth' ? 'oauth' : 'password',
      oauth_provider: p.auth === 'oauth' ? 'microsoft' : ''
    };
    step = 'form';
  }

  function openEdit(a) {
    editing = a;
    preset = presets.find((p) => p.id === a.preset) ?? null;
    form = {
      name: a.name,
      email: a.email,
      preset: a.preset,
      host: a.host,
      port: a.port,
      security: a.security,
      username: a.username,
      folder: a.folder,
      interval_secs: a.interval_secs,
      enabled: a.enabled,
      auth_kind: a.auth_kind,
      oauth_provider: a.oauth_provider,
      oauth_client_id: a.oauth_client_id,
      oauth_tenant: a.oauth_tenant
    };
    vaultItemId = null;
    formError = '';
    step = 'form';
    wizardOpen = true;
  }

  async function save() {
    if (!form.host.trim() || !form.username.trim()) {
      formError = $t('mailbox.err.hostUser');
      return;
    }
    if (isOauth && !form.oauth_client_id.trim()) {
      formError = $t('mailbox.err.clientId');
      return;
    }
    if (!isOauth && !editing && !vaultItemId) {
      formError = $t('mailbox.err.password');
      return;
    }
    saving = true;
    formError = '';
    try {
      const body = { ...form, port: Number(form.port), interval_secs: Number(form.interval_secs) };
      if (editing) {
        await mailboxApi.updateAccount(editing.id, {
          ...body,
          vault_item_id: vaultItemId ?? undefined
        });
        wizardOpen = false;
        onreload();
      } else {
        const created = await mailboxApi.createAccount({
          ...body,
          vault_item_id: isOauth ? null : vaultItemId
        });
        wizardOpen = false;
        onreload();
        // An OAuth mailbox is created unauthenticated: sign in straight away rather than
        // leaving a dead account on the screen.
        if (isOauth) signingIn = created;
      }
    } catch (e) {
      formError = e.message;
    } finally {
      saving = false;
    }
  }

  async function test(a) {
    busy = `test:${a.id}`;
    try {
      const r = await mailboxApi.testAccount(a.id);
      results[a.id] = r.ok
        ? { ok: true, text: $t('mailbox.testOk', { count: r.messages }) }
        : { ok: false, text: r.error };
    } catch (e) {
      results[a.id] = { ok: false, text: e.message };
    } finally {
      busy = '';
    }
  }

  async function fetchNow(a) {
    busy = `poll:${a.id}`;
    try {
      const r = await mailboxApi.pollAccount(a.id);
      results[a.id] = r.ok
        ? { ok: true, text: $t('mailbox.fetchOk', { count: r.new_messages }) }
        : { ok: false, text: r.error };
      onreload();
    } catch (e) {
      results[a.id] = { ok: false, text: e.message };
    } finally {
      busy = '';
    }
  }

  async function toggle(a) {
    await mailboxApi.updateAccount(a.id, { enabled: !a.enabled });
    onreload();
  }

  async function remove() {
    const a = confirmDelete;
    confirmDelete = null;
    if (!a) return;
    await mailboxApi.deleteAccount(a.id);
    onreload();
  }

  const securityOptions = $derived([
    { value: 'ssl', label: $t('mailbox.sec.ssl') },
    { value: 'starttls', label: $t('mailbox.sec.starttls') },
    { value: 'none', label: $t('mailbox.sec.none') }
  ]);

  const intervalOptions = $derived([
    { value: 300, label: $t('mailbox.every5') },
    { value: 900, label: $t('mailbox.every15') },
    { value: 3600, label: $t('mailbox.every60') },
    { value: 21600, label: $t('mailbox.every6h') }
  ]);

  function statusOf(a) {
    if (a.needs_reauth) return { tone: 'warn', key: 'mailbox.status.reauth' };
    if (!a.enabled) return { tone: 'neutral', key: 'mailbox.status.paused' };
    if (a.last_error) return { tone: 'danger', key: 'mailbox.status.error' };
    if (a.last_success_at) return { tone: 'success', key: 'mailbox.status.ok' };
    return { tone: 'warn', key: 'mailbox.status.pending' };
  }
</script>

<section class="head">
  <div>
    <h2>{$t('mailbox.mailboxes')}</h2>
    <p>{$t('mailbox.mailboxesHint')}</p>
  </div>
  <div class="headacts">
    <!-- Settings live behind the cog: four switches you set once do not deserve a
         permanent block under the accounts. -->
    <button
      class="icon"
      onclick={() => (settingsOpen = true)}
      title={$t('mailbox.settings')}
      aria-label={$t('mailbox.settings')}
    >
      <Icon name="settings" size={16} />
    </button>
    <!-- Only once there is a list to add to: while the pane is empty the empty state
         already carries the one call to action. -->
    {#if accounts.length}
      <Button variant="primary" icon="plus" onclick={openWizard}>{$t('mailbox.connect')}</Button>
    {/if}
  </div>
</section>

{#if !accounts.length}
  <EmptyState
    icon="mail"
    title={$t('mailbox.noAccountTitle')}
    description={$t('mailbox.noAccountDesc')}
  >
    {#snippet action()}
      <Button variant="primary" icon="plus" onclick={openWizard}>{$t('mailbox.connect')}</Button>
    {/snippet}
  </EmptyState>
{:else}
  <div class="accounts">
    {#each accounts as a (a.id)}
      {@const st = statusOf(a)}
      <article class="acct">
        <header>
          <div class="who">
            <h3>{a.name || a.host}</h3>
            <p>{a.email || a.username} · {a.host}:{a.port}</p>
          </div>
          <Badge tone={st.tone}>{$t(st.key)}</Badge>
        </header>

        <dl>
          <div>
            <dt>{$t('mailbox.lastFetch')}</dt>
            <dd>{a.last_success_at ? fmtDateTime(a.last_success_at) : '—'}</dd>
          </div>
          <div>
            <dt>{$t('mailbox.folder')}</dt>
            <dd>{a.folder}</dd>
          </div>
          <div>
            <dt>{$t('mailbox.interval')}</dt>
            <dd>{Math.round(a.interval_secs / 60)} min</dd>
          </div>
        </dl>

        {#if a.last_error}
          <p class="err"><Icon name="alert-triangle" size={12} /> {a.last_error}</p>
        {/if}
        {#if results[a.id]}
          <p class="result" class:bad={!results[a.id].ok}>{results[a.id].text}</p>
        {/if}

        <footer>
          {#if a.needs_reauth}
            <Button size="sm" variant="primary" icon="key" onclick={() => (signingIn = a)}>
              {$t('mailbox.reconnect')}
            </Button>
          {/if}
          <Button
            size="sm"
            variant="secondary"
            icon="plug"
            loading={busy === `test:${a.id}`}
            onclick={() => test(a)}
          >
            {$t('mailbox.test')}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            icon="refresh-cw"
            loading={busy === `poll:${a.id}`}
            onclick={() => fetchNow(a)}
          >
            {$t('mailbox.fetchNow')}
          </Button>
          <Button size="sm" variant="ghost" icon="pencil" onclick={() => openEdit(a)}>
            {$t('common.edit')}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon={a.enabled ? 'pause' : 'play'}
            onclick={() => toggle(a)}
          >
            {a.enabled ? $t('mailbox.pause') : $t('mailbox.resume')}
          </Button>
          <span class="spacer"></span>
          <Button size="sm" variant="danger" icon="trash" onclick={() => (confirmDelete = a)}>
            {$t('common.delete')}
          </Button>
        </footer>
      </article>
    {/each}
  </div>
{/if}

<Modal bind:open={settingsOpen} size="md" title={$t('mailbox.settings')}>
  {#if settings}
    <!-- One setting per row, control on the right: four unrelated switches in a grid
         read as a form to fill in rather than as choices already made. -->
    <div class="rows">
      <label class="row">
        <span class="lbl">
          {$t('mailbox.set.images')}
          <em>{$t('mailbox.set.imagesHint')}</em>
        </span>
        <input type="checkbox" bind:checked={settings.load_remote_images} />
      </label>
      <label class="row">
        <span class="lbl">
          {$t('mailbox.set.attachments')}
          <em>{$t('mailbox.set.attachmentsHint')}</em>
        </span>
        <input type="checkbox" bind:checked={settings.keep_attachments} />
      </label>
      <label class="row">
        <span class="lbl">
          {$t('mailbox.set.notify')}
          <em>{$t('mailbox.set.notifyHint')}</em>
        </span>
        <input type="checkbox" bind:checked={settings.notify_on_new} />
      </label>
      {#if settings.notify_on_new}
        <!-- Where those notifications go beyond the in-app bell: the shared broker,
             filtered to the channels granted to Mailbox. -->
        <div class="row">
          <span class="lbl">
            {$t('mailbox.set.channels')}
            <em>{$t('mailbox.set.channelsHint')}</em>
          </span>
          <ChannelButton module="mailbox" size="sm" />
        </div>
      {/if}
      <label class="row">
        <span class="lbl">
          {$t('mailbox.set.retention')}
          <em>{$t('mailbox.set.retentionHint')}</em>
        </span>
        <span class="days">
          <input type="number" min="0" bind:value={settings.retention_days} />
          {$t('mailbox.set.days')}
        </span>
      </label>
    </div>
    {#if settingsError}<ErrorText error={settingsError} />{/if}
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (settingsOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={settingsSaving} onclick={saveSettings}>
      {$t('common.save')}
    </Button>
  {/snippet}
</Modal>

<Modal
  bind:open={wizardOpen}
  size="md"
  title={editing ? $t('mailbox.editMailbox') : $t('mailbox.connect')}
>
  {#if step === 'provider'}
    <p class="lead">{$t('mailbox.pickProvider')}</p>
    <div class="presets">
      {#each presets as p (p.id)}
        <button
          class="preset"
          class:unsupported={!p.supported}
          disabled={!p.supported}
          onclick={() => pickPreset(p)}
        >
          <span class="plbl">{p.label}</span>
          {#if p.auth === 'oauth'}<span class="tag no">{$t('mailbox.oauthSignIn')}</span>{/if}
        </button>
      {/each}
    </div>
    <p class="note">{$t('mailbox.oauthNote')}</p>
  {:else}
    <div class="form">
      {#if isOauth}
        <div class="oauthlead">
          <p>{$t('mailbox.oauth.setupIntro', { provider: preset?.label ?? 'Microsoft' })}</p>
          <ol>
            <li>{$t('mailbox.oauth.setup1')}</li>
            <li>
              {$t('mailbox.oauth.setup2')}
              <div class="redirect">
                <code>{redirectUri}</code>
                <button onclick={copyRedirect} aria-label={$t('common.copy')}>
                  <Icon name={redirectCopied ? 'check' : 'copy'} size={13} />
                </button>
              </div>
              {#if !redirectOk}
                <span class="warn">{$t('mailbox.oauth.setupRedirectLan')}</span>
              {:else if preferLocalhost}
                <span class="warn">
                  {$t('mailbox.oauth.setupRedirectLocalhost', {
                    url: `${window.location.protocol}//localhost:${window.location.port}`
                  })}
                </span>
              {/if}
            </li>
            <li>{$t('mailbox.oauth.setup2b')}</li>
            <li>{$t('mailbox.oauth.setup3')}</li>
          </ol>
          {#if preset?.help_url}
            <a href={preset.help_url} target="_blank" rel="noopener noreferrer">
              {$t('mailbox.oauth.setupLink')} <Icon name="external-link" size={11} />
            </a>
          {/if}
        </div>
      {:else if preset?.help_url}
        <p class="lead">
          {$t('mailbox.appPasswordHint', { provider: preset.label })}
          <a href={preset.help_url} target="_blank" rel="noopener noreferrer">
            {$t('mailbox.appPasswordLink')} <Icon name="external-link" size={11} />
          </a>
        </p>
      {/if}
      {#if setupNote}
        <p class="setupnote">
          <Icon name="info" size={13} />
          <span>{$t(setupNote)}</span>
        </p>
      {/if}
      <div class="row">
        <Input label={$t('mailbox.accountName')} bind:value={form.name} />
        <Input label={$t('mailbox.address')} type="email" bind:value={form.email} />
      </div>
      <div class="row">
        <Input label={$t('mailbox.host')} bind:value={form.host} required />
        <Input label={$t('mailbox.port')} type="number" bind:value={form.port} />
      </div>
      <div class="row">
        <Select label={$t('mailbox.security')} options={securityOptions} bind:value={form.security} />
        <Input label={$t('mailbox.folder')} bind:value={form.folder} />
      </div>
      <Input label={$t('mailbox.username')} bind:value={form.username} required />

      {#if isOauth}
        <Input
          label={$t('mailbox.clientId')}
          bind:value={form.oauth_client_id}
          placeholder="00000000-0000-0000-0000-000000000000"
          hint={$t('mailbox.clientIdHint')}
          required
        />
        <Input
          label={$t('mailbox.tenant')}
          bind:value={form.oauth_tenant}
          hint={$t('mailbox.tenantHint')}
        />
        <p class="vhint">{$t('mailbox.oauth.storageHint')}</p>
      {:else}
        <div class="vault">
          <span class="vlabel">{$t('mailbox.password')}</span>
          <VaultPicker bind:vaultItemId hasSecret={!!editing} />
          <p class="vhint">{$t('mailbox.passwordHint')}</p>
        </div>
      {/if}

      <Select
        label={$t('mailbox.checkEvery')}
        options={intervalOptions}
        bind:value={form.interval_secs}
      />
      {#if formError}<ErrorText error={formError} />{/if}
    </div>
  {/if}

  {#snippet footer()}
    {#if step === 'form' && !editing}
      <Button variant="ghost" onclick={() => (step = 'provider')}>{$t('common.back')}</Button>
    {/if}
    <Button variant="ghost" onclick={() => (wizardOpen = false)}>{$t('common.cancel')}</Button>
    {#if step === 'form'}
      <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
    {/if}
  {/snippet}
</Modal>

<OauthModal
  account={signingIn}
  ondone={onreload}
  onclose={() => {
    signingIn = null;
    onreload();
  }}
/>

<ConfirmModal
  open={!!confirmDelete}
  title={$t('mailbox.deleteMailbox')}
  message={$t('mailbox.deleteMailboxMsg')}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={remove}
  oncancel={() => (confirmDelete = null)}
/>

<style>
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-4);
    flex-wrap: wrap;
  }
  .head h2 {
    margin: 0;
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .head p {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
    max-width: 62ch;
    line-height: 1.5;
  }

  .accounts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: var(--space-3);
  }
  .acct {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-3);
  }
  .acct header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .acct h3 {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .acct header p {
    margin: 2px 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  dl {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    margin: var(--space-3) 0 0;
  }
  dt {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  dd {
    margin: 2px 0 0;
    font-size: var(--text-xs);
    color: var(--text);
  }
  .err {
    margin: var(--space-2) 0 0;
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--red);
    overflow-wrap: anywhere;
  }
  .result {
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--green);
  }
  .result.bad {
    color: var(--red);
  }
  .acct footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-3);
  }
  .spacer {
    flex: 1;
  }

  .headacts {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .rows {
    border: var(--hairline) solid var(--border);
    background: var(--surface);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    cursor: pointer;
  }
  .row:last-child {
    border-bottom: none;
  }
  .lbl {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .lbl em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
    margin-top: 2px;
    line-height: 1.45;
  }
  .row input[type='checkbox'] {
    flex: none;
  }
  .days {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .days input {
    width: 5.5rem;
    text-align: right;
  }

  .lead {
    margin: 0 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: 1.55;
  }
  .lead a {
    color: var(--accent);
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: var(--space-2);
  }
  .preset {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
  }
  .preset:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .preset.unsupported {
    opacity: 0.55;
    cursor: not-allowed;
  }
  .tag {
    font-size: 10px;
    border-radius: 999px;
    padding: 0 6px;
    line-height: 16px;
    flex: none;
  }
  .tag.no {
    color: var(--muted);
    border: 1px solid var(--border);
  }
  .note {
    margin: var(--space-3) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  @media (max-width: 560px) {
    .row {
      grid-template-columns: 1fr;
    }
  }
  .setupnote {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.55;
  }
  .setupnote :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .oauthlead {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.55;
  }
  .oauthlead p {
    margin: 0 0 var(--space-2);
  }
  .oauthlead ol {
    margin: 0;
    padding-left: 1.2em;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .oauthlead a {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin-top: var(--space-2);
    color: var(--accent);
  }
  .oauthlead ol > li {
    padding: 1px 0;
  }
  .redirect {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-1) 0;
  }
  .redirect code {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px var(--space-2);
    color: var(--text);
    word-break: break-all;
  }
  .redirect button {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
    line-height: 0;
  }
  .redirect button:hover {
    color: var(--text);
  }
  .warn {
    display: block;
    color: var(--amber);
  }

  .vault {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .vlabel {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .vhint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.45;
  }
</style>
