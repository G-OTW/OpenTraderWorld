<script>
  import '$lib/ui/auth-card.css';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { enterApp } from '$lib/signIn.js';
  import { onMount } from 'svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let username = $state('');
  let password = $state('');
  // Second factor. Hidden until the server says this account has one: asking every user
  // for a code they do not have is how a login form teaches people to ignore it.
  let code = $state('');
  let needCode = $state(false);
  let error = $state('');
  let busy = $state(false);
  // Signup policy drives whether the "Create account" link shows (open/invite → yes).
  let signupPolicy = $state('closed');
  // Which ways in this instance offers. Password stays the default until the server says
  // otherwise, so a failed lookup never hides the form.
  let methods = $state({ password: true, social: null, recovery: false });
  // 'password' (the usual form) or 'recovery' (a recovery code, when social is unusable).
  let mode = $state('password');
  let recoveryCode = $state('');

  const PROVIDERS = { google: 'Google', microsoft: 'Microsoft', github: 'GitHub' };
  const providerName = $derived(
    methods.social ? (PROVIDERS[methods.social.provider] ?? $t('login.social.generic')) : ''
  );
  const next = () => new URLSearchParams(window.location.search).get('next') ?? '';

  onMount(async () => {
    const [status, m] = await Promise.all([
      api.setupStatus().catch(() => null),
      api.authMethods().catch(() => null)
    ]);
    signupPolicy = status?.signup_policy ?? 'closed';
    if (m) methods = m;
  });

  async function social() {
    error = '';
    busy = true;
    try {
      const { authorize_url } = await api.socialStart(next());
      window.location.href = authorize_url;
    } catch (e) {
      error = e.message;
      busy = false;
    }
  }

  function askForCode() {
    needCode = true;
    error = '';
    queueMicrotask(() => document.getElementById('totp')?.focus());
  }

  async function submit() {
    error = '';
    busy = true;
    try {
      const res = await api.login(username, password, code);
      // Bootstrap admin (auto-generated password from the installer): force a change before
      // entering the app.
      await enterApp(next(), { mustChange: res?.must_change_password, username });
    } catch (e) {
      if (e?.code === 'totp_required') {
        // The password was right. Ask for the code and keep everything else typed.
        askForCode();
        return;
      }
      // A wrong code is reported like a wrong password: the form does not say which half
      // failed, and at this point the attacker would already hold the password anyway.
      error = e?.code === 'password_login_disabled'
        ? e.message
        : needCode ? $t('login.error.code') : $t('login.error');
      code = '';
    } finally {
      busy = false;
    }
  }

  async function submitRecovery() {
    error = '';
    busy = true;
    try {
      await api.recoveryLogin(recoveryCode, code);
      // Straight to Security: the social account is presumably unusable, so relinking it
      // (or another one) is the next thing to do.
      await enterApp('/settings#security');
    } catch (e) {
      if (e?.code === 'totp_required') {
        askForCode();
        return;
      }
      error = e.message;
      code = '';
    } finally {
      busy = false;
    }
  }

  function switchMode(m) {
    mode = m;
    error = '';
    code = '';
    needCode = false;
  }
</script>

{#snippet codeField()}
  <label for="totp">{$t('login.code')}</label>
  <input
    id="totp"
    bind:value={code}
    inputmode="numeric"
    autocomplete="one-time-code"
    maxlength="6"
    placeholder="123456"
    required
  />
  <p class="hint">{$t('login.code.hint')}</p>
{/snippet}

{#if mode === 'recovery'}
  <form class="auth-card" onsubmit={(e) => { e.preventDefault(); submitRecovery(); }}>
    <h1>{$t('login.recovery.title')}</h1>
    <p class="sub">{$t('login.recovery.lead')}</p>

    <label for="rc">{$t('login.recovery.code')}</label>
    <input id="rc" bind:value={recoveryCode} autocomplete="off" spellcheck="false" placeholder="XXXX-XXXX-XXXX-XXXX" required />

    {#if needCode}{@render codeField()}{/if}

    <ErrorText error={error} compact copyable />

    <button class="primary" type="submit" disabled={busy}>
      {busy ? $t('login.working') : $t('login.submit')}
    </button>

    <div class="links">
      <a href="/login" onclick={(e) => { e.preventDefault(); switchMode('password'); }}>{$t('login.recovery.back')}</a>
      <a href="/request-reset">{$t('login.recovery.lost')}</a>
    </div>
  </form>
{:else}
  <form class="auth-card" onsubmit={(e) => { e.preventDefault(); submit(); }}>
    <h1>{$t('login.title')}</h1>

    {#if methods.social}
      <button class="primary" type="button" onclick={social} disabled={busy}>
        {$t('login.social.continue', { provider: providerName })}
      </button>
      {#if methods.password}<p class="or">{$t('login.social.or')}</p>{/if}
    {/if}

    {#if methods.password}
      <label for="u">{$t('login.username')}</label>
      <input id="u" bind:value={username} autocomplete="username" required />

      <label for="p">{$t('login.password')}</label>
      <input id="p" type="password" bind:value={password} autocomplete="current-password" required />

      {#if needCode}{@render codeField()}{/if}
    {/if}

    <ErrorText error={error} compact copyable />

    {#if methods.password}
      <button class="primary" type="submit" disabled={busy}>
        {busy ? $t('login.working') : $t('login.submit')}
      </button>
    {/if}

    <div class="links">
      {#if methods.password}
        <a href="/request-reset">{$t('login.forgot')}</a>
      {/if}
      {#if methods.recovery}
        <a href="/login" onclick={(e) => { e.preventDefault(); switchMode('recovery'); }}>{$t('login.recovery.use')}</a>
      {/if}
      {#if signupPolicy === 'open' || signupPolicy === 'invite'}
        <a href="/signup">Create account</a>
      {/if}
    </div>
  </form>
{/if}

<style>
  .or {
    margin: var(--space-4) 0 0;
    text-align: center;
    color: var(--dim);
    font-size: var(--text-xs);
  }
</style>
