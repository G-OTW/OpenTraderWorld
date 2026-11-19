<script>
  // Signing a Microsoft mailbox in. Two shapes, decided by the backend from this
  // instance's own address:
  //
  // * redirect (authorisation code + PKCE) — the current flow. A new tab goes to
  //   Microsoft and comes back to /mailbox/oauth, which completes the exchange; this
  //   modal only waits.
  // * device code — the fallback for an instance served over plain HTTP on a LAN
  //   address, which has no redirect URI Microsoft would accept. The code is displayed
  //   big and copyable because it is typed by hand on another screen.
  import { onDestroy } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { mailboxApi } from './api.js';
  import { t } from '$lib/i18n';

  let {
    /** Account to sign in; setting it opens the modal. */
    account = null,
    /** Called once the mailbox is connected. */
    ondone = () => {},
    /** Called when the user closes the modal (success or not). */
    onclose = () => {}
  } = $props();

  let mode = $state('code'); // 'code' | 'device'
  let signIn = $state(null); // { authorize_url, redirect_uri }
  let device = $state(null); // { user_code, verification_uri, expires_in, interval }
  let flow = $state(null);
  let status = $state('starting'); // starting | waiting | done | failed
  let error = $state('');
  let copied = $state(false);
  let timer = null;

  const open = $derived(!!account);

  $effect(() => {
    if (account) start(account.id);
    else stop();
  });

  onDestroy(stop);

  function stop() {
    clearTimeout(timer);
    timer = null;
  }

  async function start(id, forced) {
    stop();
    device = null;
    signIn = null;
    flow = null;
    error = '';
    copied = false;
    status = 'starting';
    try {
      const r = await mailboxApi.oauthStart(id, forced);
      mode = r.mode ?? 'device';
      device = r.device ?? null;
      signIn = r.sign_in ?? null;
      flow = r.flow;
      status = 'waiting';
      // The redirect tab answers within a second or two; a device code is polled at the
      // interval Microsoft asked for.
      schedule(mode === 'device' ? (device?.interval ?? 5) : 2);
    } catch (e) {
      error = e.message;
      status = 'failed';
    }
  }

  function schedule(seconds) {
    stop();
    timer = setTimeout(tick, Math.max(2, seconds) * 1000);
  }

  async function tick() {
    if (!flow) return;
    try {
      const r = await mailboxApi.oauthPoll(flow);
      if (r.status === 'pending') {
        schedule(r.interval ?? 5);
      } else if (r.status === 'done') {
        status = 'done';
        stop();
        ondone();
      } else {
        status = 'failed';
        error = r.error || $t('mailbox.oauth.failed');
        stop();
      }
    } catch (e) {
      status = 'failed';
      error = e.message;
      stop();
    }
  }

  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* clipboard unavailable — the value is on screen anyway */
    }
  }

  function close() {
    stop();
    onclose();
  }
</script>

<Modal {open} size="sm" title={$t('mailbox.oauth.title')} onclose={close}>
  {#if status === 'starting'}
    <Skeleton rows={3} />
  {:else if status === 'done'}
    <p class="done">
      <Icon name="check-circle" size={16} />
      {$t('mailbox.oauth.done')}
    </p>
  {:else if status === 'failed'}
    <ErrorText {error} />
    <p class="hint">
      {mode === 'device' ? $t('mailbox.oauth.failedHintDevice') : $t('mailbox.oauth.failedHint')}
    </p>
  {:else if mode === 'code' && signIn}
    <p class="lead">{$t('mailbox.oauth.redirectLead', { email: account?.email ?? '' })}</p>
    <a class="cta" href={signIn.authorize_url} target="_blank" rel="noopener noreferrer">
      <Icon name="external-link" size={14} />
      {$t('mailbox.oauth.openSignIn')}
    </a>
    <p class="waiting">
      <span class="dot"></span>
      {$t('mailbox.oauth.waiting')}
    </p>
    <p class="hint">
      {$t('mailbox.oauth.redirectHint')}
      <code class="inline">{signIn.redirect_uri}</code>
    </p>
  {:else if device}
    <ol class="steps">
      <li>
        {$t('mailbox.oauth.step1')}
        <a href={device.verification_uri} target="_blank" rel="noopener noreferrer">
          {device.verification_uri}
          <Icon name="external-link" size={11} />
        </a>
      </li>
      <li>
        {$t('mailbox.oauth.step2')}
        <div class="code">
          <code>{device.user_code}</code>
          <button onclick={() => copy(device.user_code)} aria-label={$t('common.copy')}>
            <Icon name={copied ? 'check' : 'copy'} size={13} />
          </button>
        </div>
      </li>
      <li>{$t('mailbox.oauth.step3', { email: account?.email ?? '' })}</li>
    </ol>
    <p class="waiting">
      <span class="dot"></span>
      {$t('mailbox.oauth.waiting')}
    </p>
    <p class="hint">{$t('mailbox.oauth.deviceBlockedHint')}</p>
  {/if}

  {#snippet footer()}
    {#if status !== 'done' && mode === 'code'}
      <Button variant="ghost" onclick={() => start(account.id, 'device')}>
        {$t('mailbox.oauth.useCode')}
      </Button>
    {/if}
    {#if status === 'failed'}
      <Button variant="secondary" onclick={() => start(account.id, mode)}>
        {$t('mailbox.oauth.retry')}
      </Button>
    {/if}
    <Button variant={status === 'done' ? 'primary' : 'ghost'} onclick={close}>
      {status === 'done' ? $t('common.done') : $t('common.close')}
    </Button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-4);
    font-size: var(--text-sm);
    color: var(--text);
    line-height: 1.55;
  }
  .cta {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--accent);
    color: #fff;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    text-decoration: none;
  }
  .cta:hover {
    filter: brightness(1.08);
  }
  code.inline {
    font-size: var(--text-xs);
    letter-spacing: 0;
    padding: 1px var(--space-1);
    word-break: break-all;
  }
  .steps {
    margin: 0;
    padding-left: 1.2em;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text);
    line-height: 1.55;
  }
  .steps a {
    color: var(--accent);
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .code {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  code {
    font-size: var(--text-lg);
    letter-spacing: 0.18em;
    font-weight: var(--fw-medium);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
  }
  .code button {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
  }
  .code button:hover {
    color: var(--text);
  }
  .waiting {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-4) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .dot {
    width: 7px;
    height: 7px;
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
  .done {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-sm);
    color: var(--green);
  }
  .hint {
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }
</style>
