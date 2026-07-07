<script>
  import '$lib/ui/auth-card.css';
  import { t } from '$lib/i18n';
  import { settingsApi } from '$lib/settings/api.js';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';

  // Username carried over from login, shown for context (read-only).
  const username = $derived($page.url.searchParams.get('u') ?? '');

  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit() {
    error = '';
    if (newPassword.length < 8) {
      error = $t('changePassword.err.tooShort');
      return;
    }
    if (newPassword !== confirmPassword) {
      error = $t('changePassword.err.mismatch');
      return;
    }
    busy = true;
    try {
      // Reuses the account-update endpoint: verifies current_password, sets the new hash,
      // clears must_change_password, and revokes sessions — so we bounce back to login.
      await settingsApi.updateAccount({
        current_password: currentPassword,
        new_password: newPassword
      });
      await goto('/login');
    } catch (e) {
      error = e?.message ?? $t('changePassword.err.failed');
    } finally {
      busy = false;
    }
  }
</script>

<form class="auth-card" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <h1>{$t('changePassword.title')}</h1>
  <p class="muted">{$t('changePassword.intro')}</p>

  {#if username}
    <label for="u">{$t('login.username')}</label>
    <input id="u" value={username} readonly autocomplete="username" />
  {/if}

  <label for="cur">{$t('changePassword.current')}</label>
  <input id="cur" type="password" bind:value={currentPassword} autocomplete="current-password" required />

  <label for="np">{$t('changePassword.new')}</label>
  <input id="np" type="password" bind:value={newPassword} autocomplete="new-password" required />

  <label for="cp">{$t('changePassword.confirm')}</label>
  <input id="cp" type="password" bind:value={confirmPassword} autocomplete="new-password" required />

  {#if error}<p class="err">{error}</p>{/if}

  <button type="submit" disabled={busy}>
    {busy ? $t('login.working') : $t('changePassword.submit')}
  </button>
</form>
