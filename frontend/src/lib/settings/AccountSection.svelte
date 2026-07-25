<script>
  // Account: change username/password (current password required), and logout. A password
  // change revokes all sessions server-side, so we redirect to /login afterward.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let username = $state('');
  let newUsername = $state('');
  let email = $state('');
  let emailVerified = $state(false);
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
      email = me.email ?? '';
      emailVerified = !!me.email_verified;
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

  // Only leave for /login once the session is actually gone. Redirecting unconditionally
  // would strand a demo visitor on a sign-in form: there the call is denied (403
  // `demo_disabled`, toasted globally) and auto-login keeps the session alive anyway.
  async function logout() {
    try {
      await settingsApi.logout();
    } catch {
      return;
    }
    await goto('/login');
  }
</script>

<div class="section">
  <h2>{$t('settings.account.title')}</h2>
  {#if loading}
    <!-- Four label+input pairs, the shape of the form below, so the panel does not
         collapse to one line and snap back open when the account arrives. -->
    <div class="form" aria-busy="true">
      {#each Array.from({ length: 4 }, (_, i) => i) as i (i)}
        <div class="field">
          <Skeleton height="0.85rem" width="30%" />
          <Skeleton height="2.25rem" />
        </div>
      {/each}
    </div>
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

      {#if email}
        <div class="field">
          <span>{$t('settings.account.email')}</span>
          <div class="email-row">
            <span class="email">{email}</span>
            {#if emailVerified}
              <span class="badge ok">{$t('settings.account.verified')}</span>
            {:else}
              <span class="badge warn">{$t('settings.account.unverified')}</span>
            {/if}
          </div>
        </div>
      {/if}

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

      <ErrorText error={error} />
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
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
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
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .ok {
    color: var(--green-ink);
    font-size: var(--text-base);
    margin: 0;
  }
  .field span {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .email-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .email {
    color: var(--text);
    font-size: var(--text-base);
  }
  .badge {
    font-size: var(--text-xs);
    padding: 2px 8px;
    border-radius: var(--radius);
    border: var(--hairline) solid var(--border);
  }
  .badge.ok {
    color: var(--green-ink);
  }
  .badge.warn {
    color: var(--amber-ink);
  }
</style>
