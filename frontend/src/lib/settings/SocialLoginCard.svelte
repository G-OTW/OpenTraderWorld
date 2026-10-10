<script>
  // Settings → Security → Social sign-in. Three states, one card:
  //   1. no provider (or editing): the provider form and the redirect URI to register;
  //   2. configured, nothing linked: link an account (the browser goes to the provider);
  //   3. linked: the account sign-in is locked to, recovery codes, the password switch.
  //
  // Password sign-in can only be turned off from state 3, once recovery codes exist: they
  // are the way back in when the provider locks, deletes or expires the account.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  const PROVIDERS = [
    { value: 'google', label: 'Google' },
    { value: 'microsoft', label: 'Microsoft' },
    { value: 'github', label: 'GitHub' },
    { value: 'oidc', label: 'OpenID Connect' }
  ];
  const labelOf = (p) => PROVIDERS.find((x) => x.value === p)?.label ?? p;

  let data = $state(null);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let editing = $state(false);
  let form = $state({ provider: 'google', issuer: '', client_id: '', client_secret: '' });
  // A fresh set of recovery codes, on screen until the card reloads. Never fetched back.
  let codes = $state(null);
  let confirm = $state({ open: false, title: '', message: '', run: null });

  const redirectUri = $derived(
    typeof location !== 'undefined' && data ? `${location.origin}${data.callback_path}` : ''
  );
  // Google and Microsoft refuse a plain-HTTP redirect URI anywhere but on loopback.
  const insecure = $derived(
    typeof location !== 'undefined' &&
      location.protocol === 'http:' &&
      !['localhost', '127.0.0.1'].includes(location.hostname)
  );

  onMount(reload);

  async function reload() {
    loading = true;
    try {
      data = await settingsApi.socialStatus();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function run(fn) {
    error = '';
    busy = true;
    try {
      await fn();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function edit() {
    const c = data?.config;
    form = c
      ? { provider: c.provider, issuer: c.issuer, client_id: c.client_id, client_secret: '' }
      : { provider: 'google', issuer: '', client_id: '', client_secret: '' };
    editing = true;
  }

  const save = () =>
    run(async () => {
      await settingsApi.socialSave(form);
      editing = false;
      await reload();
    });

  const link = () =>
    run(async () => {
      const { authorize_url } = await settingsApi.socialLink();
      window.location.href = authorize_url;
    });

  const generate = () =>
    run(async () => {
      const r = await settingsApi.recoveryCodes();
      await reload();
      codes = r.codes;
    });

  const setPassword = (enabled) =>
    run(async () => {
      await settingsApi.setPasswordLogin(enabled);
      await reload();
    });

  function ask(title, message, fn) {
    confirm = { open: true, title, message, run: () => run(fn) };
  }

  const unlink = () =>
    ask($t('security.social.unlink'), $t('security.social.unlinkConfirm'), async () => {
      await settingsApi.socialUnlink();
      codes = null;
      await reload();
    });

  const remove = () =>
    ask($t('security.social.remove'), $t('security.social.removeConfirm'), async () => {
      await settingsApi.socialRemove();
      await reload();
    });

  function when(value) {
    if (!value) return '';
    const d = new Date(value);
    return Number.isNaN(d.getTime()) ? '' : d.toLocaleDateString();
  }
</script>

<header>
  <h2>{$t('security.social.title')}</h2>
  <p class="lead">{$t('security.social.lead')}</p>
</header>

<div class="card">
  {#if loading}
    <Skeleton rows={3} />
  {:else if data && (!data.config || editing)}
    <div class="grid">
      <Select label={$t('security.social.provider')} options={PROVIDERS} bind:value={form.provider} />
      {#if form.provider === 'microsoft'}
        <Input label={$t('security.social.tenant')} placeholder="common" bind:value={form.issuer} hint={$t('security.social.tenantHint')} />
      {:else if form.provider === 'oidc'}
        <Input label={$t('security.social.issuer')} placeholder="https://auth.example.com/application/o/otw" bind:value={form.issuer} />
      {/if}
      <Input label={$t('security.social.clientId')} bind:value={form.client_id} autocomplete="off" spellcheck="false" />
      <Input
        label={$t('security.social.clientSecret')}
        type="password"
        bind:value={form.client_secret}
        autocomplete="new-password"
        placeholder={data.config?.has_secret ? $t('security.social.secretKept') : ''}
      />
    </div>
    <p class="muted small">{$t(`security.social.help.${form.provider}`)}</p>
    <p class="muted small">{$t('security.social.redirect')}</p>
    <code class="uri" use:copyLog={redirectUri}>{redirectUri}</code>
    {#if insecure}
      <p class="warn small"><Icon name="alert-triangle" size={12} /> {$t('security.social.insecure')}</p>
    {/if}
    <div class="row end">
      {#if editing}
        <button class="ghost" onclick={() => (editing = false)} disabled={busy}>{$t('common.cancel')}</button>
      {/if}
      <button class="primary" onclick={save} disabled={busy || !form.client_id.trim()}>
        {$t('common.save')}
      </button>
    </div>
  {:else if data && !data.linked}
    <div class="row">
      <span class="state">
        <Icon name="alert-triangle" size={14} />
        {$t('security.social.notLinked', { provider: labelOf(data.config.provider) })}
      </span>
      <button class="ghost" onclick={edit} disabled={busy}>{$t('security.social.edit')}</button>
      <button class="ghost danger" onclick={remove} disabled={busy}>{$t('security.social.remove')}</button>
      <button class="primary" onclick={link} disabled={busy}>{$t('security.social.link')}</button>
    </div>
    <p class="muted small">{$t('security.social.linkHint')}</p>
  {:else if data}
    <div class="row">
      <span class="state on">
        <Icon name="check" size={14} />
        {$t('security.social.linked', {
          provider: labelOf(data.config.provider),
          account: data.linked.label || labelOf(data.config.provider),
          date: when(data.linked.linked_at)
        })}
      </span>
      <button class="ghost" onclick={edit} disabled={busy}>{$t('security.social.edit')}</button>
      <button class="ghost danger" onclick={unlink} disabled={busy}>{$t('security.social.unlink')}</button>
    </div>

    <div class="sub">
      <div class="row">
        <span class="label">{$t('security.social.codes')}</span>
        <span class="muted small grow">{$t('security.social.codesLeft', { n: data.recovery_codes_left })}</span>
        <button class="ghost" onclick={generate} disabled={busy}>
          {data.recovery_codes_left > 0 ? $t('security.social.codesRegenerate') : $t('security.social.codesGenerate')}
        </button>
      </div>
      {#if codes}
        <ul class="codes" use:copyLog={codes.join('\n')}>
          {#each codes as c (c)}<li>{c}</li>{/each}
        </ul>
        <p class="warn small">{$t('security.social.codesOnce')}</p>
      {/if}
    </div>

    <div class="sub">
      <div class="row">
        <span class="label">{$t('security.social.password')}</span>
        <span class="small grow" class:muted={data.password_login} class:ok={!data.password_login}>
          {data.password_login ? $t('security.social.passwordOn') : $t('security.social.passwordOff')}
        </span>
        {#if data.password_login}
          <button class="ghost" onclick={() => setPassword(false)} disabled={busy || data.recovery_codes_left === 0}>
            {$t('security.social.passwordDisable')}
          </button>
        {:else}
          <button class="ghost" onclick={() => setPassword(true)} disabled={busy}>
            {$t('security.social.passwordEnable')}
          </button>
        {/if}
      </div>
      <p class="muted small">
        {data.recovery_codes_left === 0 ? $t('security.social.passwordNeedsCodes') : $t('security.social.passwordHint')}
      </p>
    </div>
  {/if}

  <ErrorText error={error} compact copyable />
</div>

<ConfirmModal
  bind:open={confirm.open}
  title={confirm.title}
  message={confirm.message}
  confirmLabel={confirm.title}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={() => confirm.run?.()}
/>

<style>
  header h2 {
    margin: 0;
    font-size: 15px;
  }
  .lead {
    margin: var(--space-1) 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--space-3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .row.end {
    justify-content: flex-end;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 13px;
    color: var(--amber);
    margin-right: auto;
  }
  .state.on {
    color: var(--green);
  }
  .sub {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
  .label {
    font-size: 13px;
    font-weight: 500;
  }
  .grow {
    margin-right: auto;
  }
  .uri {
    display: block;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: var(--space-2);
    font-size: 12px;
  }
  .codes {
    list-style: none;
    margin: 0;
    padding: var(--space-2);
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: var(--space-1) var(--space-3);
    background: var(--surface-2);
    border-radius: var(--radius);
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    letter-spacing: 0.04em;
  }
  .muted {
    color: var(--muted);
  }
  .ok {
    color: var(--green);
  }
  .warn {
    color: var(--amber);
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .small {
    font-size: 12px;
    margin: 0;
  }
  .danger {
    color: var(--red);
  }
</style>
