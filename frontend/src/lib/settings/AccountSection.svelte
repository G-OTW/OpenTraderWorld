<script>
  // Account: change username/password (current password required), and logout. A password
  // change revokes all sessions server-side, so we redirect to /login afterward.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  let username = $state('');
  let newUsername = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let currentPassword = $state('');

  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let ok = $state('');

  onMount(async () => {
    try {
      const me = await settingsApi.me();
      username = me.username;
      newUsername = me.username;
    } finally {
      loading = false;
    }
  });

  async function save() {
    error = '';
    ok = '';
    if (newPassword && newPassword !== confirmPassword) {
      error = $t('settings.account.err.mismatch');
      return;
    }
    if (!currentPassword) {
      error = $t('settings.account.err.currentRequired');
      return;
    }
    saving = true;
    try {
      const input = { current_password: currentPassword };
      if (newUsername && newUsername !== username) input.username = newUsername.trim();
      if (newPassword) input.new_password = newPassword;
      const res = await settingsApi.updateAccount(input);
      if (res.password_changed) {
        // Sessions were revoked — send to login.
        await goto('/login');
        return;
      }
      username = input.username ?? username;
      ok = $t('settings.account.updated');
      currentPassword = '';
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function logout() {
    try {
      await settingsApi.logout();
    } finally {
      await goto('/login');
    }
  }
</script>

<div class="section">
  <h2>{$t('settings.account.title')}</h2>
  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <label class="field">
        <span>{$t('settings.account.username')}</span>
        <input bind:value={newUsername} autocomplete="username" />
      </label>

      <div class="grid2">
        <label class="field">
          <span>{$t('settings.account.newPassword')}</span>
          <input
            type="password"
            bind:value={newPassword}
            autocomplete="new-password"
            placeholder={$t('settings.account.newPasswordPlaceholder')}
          />
        </label>
        <label class="field">
          <span>{$t('settings.account.confirmPassword')}</span>
          <input type="password" bind:value={confirmPassword} autocomplete="new-password" />
        </label>
      </div>

      <label class="field">
        <span>{$t('settings.account.currentPassword')}</span>
        <input
          type="password"
          bind:value={currentPassword}
          autocomplete="current-password"
          placeholder={$t('settings.account.currentPasswordPlaceholder')}
        />
      </label>

      {#if error}<p class="err">{error}</p>{/if}
      {#if ok}<p class="ok">{ok}</p>{/if}

      <div class="actions">
        <button type="button" class="ghost danger" onclick={logout}>{$t('settings.account.logout')}</button>
        <button type="submit" class="primary" disabled={saving}>
          {saving ? $t('common.saving') : $t('settings.account.saveChanges')}
        </button>
      </div>
      <p class="muted small">{$t('settings.account.revokeNote')}</p>
    </form>
  {/if}
</div>

<style>
  .section {
    max-width: 560px;
  }
  h2 {
    margin: 0 0 var(--space-4);
    font-size: 1.1rem;
    color: var(--text);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.78rem;
  }
  .err {
    color: var(--red);
    font-size: 0.85rem;
    margin: 0;
  }
  .ok {
    color: var(--green);
    font-size: 0.85rem;
    margin: 0;
  }
</style>
