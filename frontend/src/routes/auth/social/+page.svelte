<script>
  // Where the identity provider sends the browser back after a social sign-in or a link
  // from Settings. This is the redirect URI the owner registers, so the path must stay
  // `/auth/social`. The page hands the code to the backend and moves on; the only thing it
  // may ask for is the authenticator code of an account that has one.
  import '$lib/ui/auth-card.css';
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { enterApp } from '$lib/signIn.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let status = $state('working'); // working | code | failed
  let error = $state('');
  let code = $state('');
  let busy = $state(false);

  onMount(async () => {
    const q = new URLSearchParams(window.location.search);
    try {
      const r = await api.socialCallback({
        state: q.get('state') ?? '',
        code: q.get('code') ?? '',
        // The provider's own wording when the user declined or a policy refused.
        error: q.get('error_description') || q.get('error') || ''
      });
      if (r.kind === 'link') {
        await goto('/settings#security');
        return;
      }
      await enterApp(r.next, { mustChange: r.must_change_password });
    } catch (e) {
      if (e?.code === 'totp_required') {
        status = 'code';
        queueMicrotask(() => document.getElementById('totp')?.focus());
        return;
      }
      status = 'failed';
      error = e.message;
    }
  });

  async function submitCode() {
    error = '';
    busy = true;
    try {
      const r = await api.socialTotp(code.trim());
      await enterApp(r.next, { mustChange: r.must_change_password });
    } catch (e) {
      error = e.message;
      code = '';
    } finally {
      busy = false;
    }
  }
</script>

{#if status === 'code'}
  <form class="auth-card" onsubmit={(e) => { e.preventDefault(); submitCode(); }}>
    <h1>{$t('login.title')}</h1>
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
    <ErrorText error={error} compact copyable />
    <button class="primary" type="submit" disabled={busy}>
      {busy ? $t('login.working') : $t('login.submit')}
    </button>
  </form>
{:else}
  <div class="auth-card">
    <h1>{status === 'working' ? $t('login.social.working') : $t('login.social.failed')}</h1>
    {#if status === 'failed'}
      <ErrorText error={error} compact copyable />
      <a class="link" href="/">{$t('login.social.back')}</a>
    {/if}
  </div>
{/if}
