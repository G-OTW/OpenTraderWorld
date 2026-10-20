<script>
  import '$lib/ui/auth-card.css';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { goto } from '$app/navigation';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  import { onMount } from 'svelte';

  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  // Bootstrap token. Only a network-facing instance asks for one, so the field appears only
  // when /api/setup/status says it will be checked.
  let setupToken = $state('');
  let needToken = $state(false);
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    try {
      const status = await api.setupStatus();
      needToken = !!status?.token_required;
    } catch {
      /* the server will refuse with `setup_token_required` if it matters */
    }
  });

  async function submit() {
    error = '';
    if (password !== confirm) {
      error = $t('setup.error.mismatch');
      return;
    }
    busy = true;
    try {
      // Length and the breach screen are the server's call, not a duplicated rule here:
      // one policy, one place, and its message says exactly what to fix.
      await api.createAdmin(username, password, setupToken);
      await goto('/'); // admin created + session set → dashboard
    } catch (e) {
      if (e?.code === 'setup_token_required') needToken = true;
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

  {#if needToken}
    <label for="tok">{$t('setup.token')}</label>
    <input id="tok" bind:value={setupToken} autocomplete="off" spellcheck="false" required />
    <p class="hint">{$t('setup.token.hint')}</p>
  {/if}

  <ErrorText error={error} compact copyable />

  <button class="primary" type="submit" disabled={busy}>
    {busy ? $t('setup.working') : $t('setup.submit')}
  </button>
</form>

