<script>
  import '$lib/ui/auth-card.css';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { goto } from '$app/navigation';
  import { ensureInstalled } from '$lib/modules/installed.js';
  import { notifStore } from '$lib/modules/remindme/store.svelte.js';
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

  onMount(async () => {
    try {
      const status = await api.setupStatus();
      signupPolicy = status?.signup_policy ?? 'closed';
    } catch {
      /* leave closed */
    }
  });

  async function submit() {
    error = '';
    busy = true;
    try {
      const res = await api.login(username, password, code);
      // Bootstrap admin (auto-generated password from the installer): force a change before
      // entering the app. Pass the just-used password so the change screen can authorize it.
      if (res?.must_change_password) {
        await goto(`/change-password?u=${encodeURIComponent(username)}`);
        return;
      }
      // The root layout mounted while unauthenticated, so its initial installed-set fetch
      // got a 401 and bailed. Now that we have a session, (re)load it before navigating so
      // the dashboard + switcher render without needing a manual refresh.
      await ensureInstalled(true).catch(() => {});
      notifStore.start();
      await goto('/');
    } catch (e) {
      if (e?.code === 'totp_required') {
        // The password was right. Ask for the code and keep everything else typed.
        needCode = true;
        error = '';
        queueMicrotask(() => document.getElementById('totp')?.focus());
        return;
      }
      // A wrong code is reported like a wrong password: the form does not say which half
      // failed, and at this point the attacker would already hold the password anyway.
      error = needCode ? $t('login.error.code') : $t('login.error');
      code = '';
    } finally {
      busy = false;
    }
  }
</script>

<form class="auth-card" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <h1>{$t('login.title')}</h1>

  <label for="u">{$t('login.username')}</label>
  <input id="u" bind:value={username} autocomplete="username" required />

  <label for="p">{$t('login.password')}</label>
  <input id="p" type="password" bind:value={password} autocomplete="current-password" required />

  {#if needCode}
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
  {/if}

  <ErrorText error={error} compact copyable />

  <button class="primary" type="submit" disabled={busy}>
    {busy ? $t('login.working') : $t('login.submit')}
  </button>

  <div class="links">
    <a href="/request-reset">{$t('login.forgot')}</a>
    {#if signupPolicy === 'open' || signupPolicy === 'invite'}
      <a href="/signup">Create account</a>
    {/if}
  </div>
</form>

