<script>
  // Where Microsoft sends the browser back after an authorisation-code sign-in. This is
  // the app's registered redirect URI, so the path must stay `/mailbox/oauth`.
  //
  // The tab does one thing: hand the code to the backend, which exchanges it and seals the
  // refresh token. The window that started the sign-in learns about it by polling, so this
  // one can simply be closed — and the exchange happens even if it was closed already.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { mailboxApi } from '$lib/modules/mailbox/api.js';
  import { t } from '$lib/i18n';

  let status = $state('working'); // working | done | failed
  let error = $state('');

  onMount(async () => {
    const q = new URLSearchParams(window.location.search);
    const flow = q.get('state');
    if (!flow) {
      status = 'failed';
      error = $t('mailbox.oauth.cbNoState');
      return;
    }
    try {
      const r = await mailboxApi.oauthCallback({
        flow,
        code: q.get('code'),
        // Microsoft's own wording when the user declines or a policy refuses.
        error: q.get('error_description') || q.get('error')
      });
      status = r.ok ? 'done' : 'failed';
      error = r.error ?? '';
    } catch (e) {
      status = 'failed';
      error = e.message;
    }
  });
</script>

<div class="wrap">
  <div class="card">
    {#if status === 'working'}
      <span class="dot"></span>
      <h1>{$t('mailbox.oauth.cbWorking')}</h1>
    {:else if status === 'done'}
      <Icon name="check-circle" size={28} />
      <h1>{$t('mailbox.oauth.cbDone')}</h1>
      <p>{$t('mailbox.oauth.cbClose')}</p>
    {:else}
      <Icon name="alert-triangle" size={28} />
      <h1>{$t('mailbox.oauth.cbFailed')}</h1>
      {#if error}<p class="err">{error}</p>{/if}
      <p>{$t('mailbox.oauth.cbRetry')}</p>
    {/if}
    {#if status !== 'working'}
      <Button variant="ghost" onclick={() => window.close()}>{$t('common.close')}</Button>
    {/if}
  </div>
</div>

<style>
  .wrap {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: var(--space-6);
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    max-width: 420px;
    text-align: center;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-8) var(--space-6);
  }
  h1 {
    margin: 0;
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  p {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: 1.55;
  }
  .err {
    color: var(--red);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--amber);
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }
</style>
