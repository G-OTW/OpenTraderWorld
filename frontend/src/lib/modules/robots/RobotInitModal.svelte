<script>
  // What a new robot does in its first seconds: the server runs its first tick at once
  // (download the window, simulate, learn the book), and this follows it. Once the robot is
  // ready, a broker robot offers to be armed here. Closing leaves it running in the background.
  import { onDestroy } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import BackgroundTradingModal from '$lib/brokers/BackgroundTradingModal.svelte';
  import { robotsApi } from './api.js';
  import { nextStep } from './execution.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), robot = null, onclose = () => {} } = $props();

  let session = $state(null);
  let openCount = $state(0);
  let elapsed = $state(0);
  let account = $state(null);
  let armed = $state(false);
  let arming = $state(false);
  let needsCode = $state(false);
  let code = $state('');
  let armError = $state('');
  let signOpen = $state(false);
  let timer;
  let clock;

  const broker = $derived(robot?.execution?.target === 'broker');
  const failed = $derived(!!session?.last_error);
  const ready = $derived(!failed && !!session?.seeded_at);
  const phase = $derived(failed ? 'error' : ready ? 'ready' : 'init');

  async function poll() {
    if (!robot) return;
    try {
      const r = await robotsApi.get(robot.id);
      session = r.session;
      openCount = r.open?.length ?? 0;
      armed = !!r.session?.armed_at;
      if (ready || failed) stop();
    } catch {
      /* next poll */
    }
  }
  function stop() {
    clearInterval(timer);
    clearInterval(clock);
    timer = clock = undefined;
  }
  $effect(() => {
    if (!open || !robot) return;
    session = null;
    elapsed = 0;
    armed = false;
    armError = '';
    poll();
    timer = setInterval(poll, 1000);
    const t0 = Date.now();
    clock = setInterval(() => (elapsed = Math.floor((Date.now() - t0) / 1000)), 250);
    return stop;
  });
  onDestroy(stop);

  $effect(() => {
    if (ready && broker && robot && !account) {
      robotsApi.execution(robot.id).then((v) => (account = v?.account ?? null)).catch(() => {});
    }
  });

  async function arm() {
    arming = true;
    armError = '';
    try {
      await robotsApi.arm(robot.id, code);
      armed = true;
      needsCode = false;
    } catch (e) {
      const step = nextStep(e);
      if (step === 'code') needsCode = true;
      if (step === 'sign') signOpen = true;
      armError = e.message;
    } finally {
      arming = false;
    }
  }

  function close() {
    open = false;
    onclose();
  }
</script>

<Modal bind:open size="sm" title={$t('robots.init.title', { name: robot?.name ?? '' })} onclose={close}>
  <ol class="steps" aria-live="polite">
    <li class="done" in:fly={{ y: 6, duration: 200 }}>
      <span class="mark"><Icon name="check" size={13} /></span>
      <span>{$t('robots.init.created')}</span>
    </li>
    <li class:done={ready} class:bad={failed} class:run={phase === 'init'} in:fly={{ y: 6, duration: 200, delay: 120 }}>
      <span class="mark">
        {#if phase === 'init'}<span class="spin" aria-hidden="true"></span>{:else if failed}<Icon name="x" size={13} />{:else}<Icon name="check" size={13} />{/if}
      </span>
      <span>
        {$t('robots.init.loading')}
        {#if phase === 'init'}<em class="fig">{elapsed}s</em>{/if}
      </span>
    </li>
    {#if ready}
      <li class="done" in:fly={{ y: 6, duration: 220 }}>
        <span class="mark"><Icon name="check" size={13} /></span>
        <span>{$t('robots.init.ready', { bars: session.bars, open: openCount })}</span>
      </li>
    {/if}
  </ol>

  <div class="bar" class:full={ready} class:bad={failed} aria-hidden="true"><span></span></div>

  {#if failed}
    <p class="err" in:fade>{session.last_error}</p>
  {:else if phase === 'init'}
    <p class="hint" in:fade>{$t('robots.init.background')}</p>
  {:else if broker}
    <div class="arm" in:fly={{ y: 8, duration: 240 }}>
      {#if armed}
        <p class="ok"><Icon name="zap" size={13} /> {$t('robots.init.armed')}</p>
      {:else}
        <p>{$t('robots.exec.armBody', { account: account?.name ?? '' })}</p>
        {#if account?.level === 'live'}<p class="warn"><Icon name="alert-triangle" size={13} /> {$t('robots.exec.armBodyLive')}</p>{/if}
        {#if needsCode}
          <Input label={$t('robots.exec.codeLabel')} bind:value={code} hint={$t('robots.exec.codeHint')} autocomplete="one-time-code" inputmode="numeric" />
        {/if}
        {#if armError}<p class="err">{armError}</p>{/if}
      {/if}
    </div>
  {:else}
    <p class="hint" in:fade>{$t('robots.init.simulated')}</p>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={close}>{phase === 'init' ? $t('robots.init.runInBackground') : $t('robots.init.openRobot')}</Button>
    {#if ready && broker && !armed}
      <Button variant="primary" icon="zap" loading={arming} onclick={arm}>{$t('robots.exec.arm')}</Button>
    {/if}
  {/snippet}
</Modal>

<BackgroundTradingModal bind:open={signOpen} {account} onsigned={() => { signOpen = false; arm(); }} />

<style>
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-size: var(--text-sm);
  }
  .steps li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    transition: color 0.3s ease;
  }
  .steps li.done,
  .steps li.run {
    color: var(--text);
  }
  .mark {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: var(--hairline) solid var(--border);
    flex: none;
    transition: background 0.3s ease, color 0.3s ease, border-color 0.3s ease;
  }
  li.done .mark {
    background: var(--green);
    border-color: var(--green);
    color: var(--surface);
  }
  li.bad .mark {
    background: var(--red);
    border-color: var(--red);
    color: var(--surface);
  }
  .spin {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  .fig {
    font-style: normal;
    font-family: var(--mono);
    color: var(--muted);
    margin-left: var(--space-2);
  }
  .bar {
    margin: var(--space-4) 0;
    height: 3px;
    background: var(--surface-2);
    overflow: hidden;
    position: relative;
  }
  .bar span {
    position: absolute;
    inset: 0 auto 0 0;
    width: 35%;
    background: var(--accent);
    animation: slide 1.2s ease-in-out infinite;
  }
  .bar.full span {
    width: 100%;
    background: var(--green);
    animation: none;
    transition: width 0.4s ease;
  }
  .bar.bad span {
    width: 100%;
    background: var(--red);
    animation: none;
  }
  .hint,
  .arm p {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  .arm {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .ok {
    color: var(--green) !important;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .warn {
    color: var(--amber) !important;
  }
  .err {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--red);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes slide {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(300%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin,
    .bar span {
      animation: none;
    }
  }
</style>
