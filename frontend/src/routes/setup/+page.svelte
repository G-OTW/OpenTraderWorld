<script>
  import '$lib/ui/auth-card.css';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { goto } from '$app/navigation';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit() {
    error = '';
    if (password.length < 8) {
      error = $t('setup.error.short');
      return;
    }
    if (password !== confirm) {
      error = $t('setup.error.mismatch');
      return;
    }
    busy = true;
    try {
      await api.createAdmin(username, password);
      await goto('/'); // admin created + session set → dashboard
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<form class="auth-card" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <h1>{$t('setup.title')}</h1>
  <p class="sub">{$t('setup.subtitle')}</p>

  <label for="u">{$t('setup.username')}</label>
  <input id="u" bind:value={username} autocomplete="username" required />

  <label for="p">{$t('setup.password')}</label>
  <input id="p" type="password" bind:value={password} autocomplete="new-password" required />

  <label for="c">{$t('setup.password_confirm')}</label>
  <input id="c" type="password" bind:value={confirm} autocomplete="new-password" required />

  <ErrorText error={error} compact copyable />

  <button class="primary" type="submit" disabled={busy}>
    {busy ? $t('setup.working') : $t('setup.submit')}
  </button>
</form>
