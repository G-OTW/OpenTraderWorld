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
      const res = await api.login(username, password);
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
    } catch {
      error = $t('login.error');
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

  <ErrorText error={error} compact copyable />

  <button class="primary" type="submit" disabled={busy}>
    {busy ? $t('login.working') : $t('login.submit')}
  </button>

  <div class="links">
    <a href="/request-reset">Forgot password?</a>
    {#if signupPolicy === 'open' || signupPolicy === 'invite'}
      <a href="/signup">Create account</a>
    {/if}
  </div>
</form>
