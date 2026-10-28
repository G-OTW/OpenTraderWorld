<!--
  The password prompt behind step-up re-authentication. Mounted once, in the root layout:
  the API wrappers raise it from wherever a request was refused with `reauth_required`, so
  no individual screen has to own a password field. The queue lives in $lib/reauth.svelte.js.
  With the trading lock on, an order refused with `trading_locked` raises it too, asking for
  the authenticator code alone and opening trading for this session.
-->
<script>
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { brokersApi } from '$lib/brokers/api.js';
  import { currentRequest } from '$lib/reauth.svelte.js';
  import { t } from '$lib/i18n';

  let password = $state('');
  let code = $state('');
  let needCode = $state(false);
  let error = $state('');
  let busy = $state(false);

  const request = $derived(currentRequest());
  const open = $derived(request !== null);
  const unlock = $derived(!!request?.unlock);

  function reset() {
    password = '';
    code = '';
    needCode = false;
    error = '';
    busy = false;
  }

  function cancel() {
    const pending = request;
    reset();
    pending?.done(false);
  }

  async function submit() {
    error = '';
    busy = true;
    try {
      if (unlock) await brokersApi.unlock(code.trim());
      else await settingsApi.reauth(password, code, !!request?.once);
      const pending = request;
      reset();
      pending?.done(true);
    } catch (e) {
      // Second wrong answer: the server closed the session and the client is already on
      // its way to sign-in, so the prompt just goes away.
      if (e?.status === 401) {
        cancel();
        return;
      }
      if (e?.code === 'totp_required') {
        needCode = true;
        error = '';
      } else {
        error = e.message;
        code = '';
      }
    } finally {
      busy = false;
    }
  }
</script>

<Modal {open} title={$t(unlock ? 'reauth.unlockTitle' : 'reauth.title')} size="sm" fit onclose={cancel}>
  <form
    class="body"
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    {#if unlock}
      <p class="lead">{$t('reauth.unlockLead')}</p>
      <label for="reauth-unlock">{$t('reauth.code')}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="reauth-unlock"
        bind:value={code}
        inputmode="numeric"
        autocomplete="one-time-code"
        maxlength="6"
        placeholder="123456"
        autofocus
        required
      />
    {:else}
      <p class="lead">{$t(request?.once ? 'reauth.leadOnce' : 'reauth.lead')}</p>

      <label for="reauth-pw">{$t('reauth.password')}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="reauth-pw"
        type="password"
        bind:value={password}
        autocomplete="current-password"
        autofocus
        required
      />
    {/if}

    {#if needCode && !unlock}
      <label for="reauth-code">{$t('reauth.code')}</label>
      <input
        id="reauth-code"
        bind:value={code}
        inputmode="numeric"
        autocomplete="one-time-code"
        maxlength="6"
        placeholder="123456"
        required
      />
    {/if}

    <ErrorText error={error} compact />

    <div class="actions">
      <button type="button" class="ghost" onclick={cancel}>{$t('common.cancel')}</button>
      <button type="submit" class="primary" disabled={busy || (unlock ? code.trim().length !== 6 : !password)}>
        {busy ? $t('reauth.working') : $t('reauth.submit')}
      </button>
    </div>
  </form>
</Modal>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .lead {
    margin: 0 0 var(--space-1);
    color: var(--muted);
    font-size: 13px;
  }
  label {
    font-size: 12px;
    color: var(--muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
