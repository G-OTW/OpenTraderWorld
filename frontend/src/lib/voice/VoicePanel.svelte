<script>
  // The floating voice card: what is heard, what it will do, and how it went. Not a modal:
  // the page stays usable behind it, and the agent widget it may hand off to stays visible.
  // Enter runs a plan under review, Escape cancels (handled with the shortcut, in VoiceButton).
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { voice } from './voice.svelte.js';
  import { shortcutKeys } from './shortcut.js';

  const STEP_ICON = {
    navigate: 'arrow-right',
    agent: 'brain',
    automator: 'zap',
    theme: 'moon',
    privacy: 'eye-off',
    say: 'volume-2'
  };
  const BARS = 5;

  const show = $derived(voice.phase !== 'idle');
  const keys = $derived(
    shortcutKeys((voice.mode === 'dictation' ? voice.settings?.dictation_shortcut : voice.settings?.shortcut) ?? '')
  );

  function stepLabel(s) {
    switch (s.kind) {
      case 'navigate':
        return $t('voice.step.navigate', { target: s.label || s.target });
      case 'agent':
        return $t('voice.step.agent', { prompt: s.prompt });
      case 'automator':
        return $t('voice.step.automator', { name: s.name || s.workflow_id });
      case 'theme':
        return $t(`voice.step.theme.${s.value}`);
      case 'privacy':
        return $t(`voice.step.privacy.${s.value}`);
      case 'say':
        return $t('voice.step.say', { text: s.text });
      default:
        return s.kind;
    }
  }

  // The step index each entry starts at, so the plan reads grouped by where it came from.
  const groups = $derived.by(() => {
    if (!voice.plan) return [];
    let at = 0;
    return voice.plan.entries.map((e) => {
      const start = at;
      at += e.steps?.length ?? 0;
      return { entry: e, start };
    });
  });

  function onKey(e) {
    if (voice.phase === 'review' && e.key === 'Enter' && !voice.plan?.error) {
      const el = document.activeElement;
      if (el && el !== document.body && !el.closest('.voice-card')) return;
      e.preventDefault();
      voice.run();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if show}
  <div
    class="voice-card"
    class:error={voice.phase === 'error'}
    role="dialog"
    aria-modal="false"
    aria-label={$t('voice.panel.title')}
  >
    <header>
      <span class="mode" class:dict={voice.mode === 'dictation'}>
        <Icon name={voice.mode === 'dictation' ? 'type' : 'mic'} size={12} />
        {voice.mode === 'dictation' ? $t('voice.mode.dictation') : $t('voice.mode.command')}
      </span>
      <span class="phase" aria-live="polite">
        {#if voice.phase === 'starting'}
          {$t('voice.phase.starting')}
        {:else if voice.phase === 'listening'}
          {$t('voice.phase.listening')}
        {:else if voice.phase === 'transcribing'}
          {$t('voice.phase.transcribing')}
        {:else if voice.phase === 'review'}
          {voice.plan?.error ? $t('voice.phase.cannotRun') : $t('voice.phase.review')}
        {:else if voice.phase === 'running'}
          {$t('voice.phase.running')}
        {:else if voice.phase === 'done'}
          {$t('voice.phase.done')}
        {:else if voice.phase === 'error'}
          {$t('voice.phase.error')}
        {/if}
      </span>
      <button class="x" aria-label={$t('common.close')} title={$t('voice.panel.cancelHint')} onclick={() => voice.cancel()}>
        <Icon name="x" size={14} />
      </button>
    </header>

    {#if voice.phase === 'listening' || voice.phase === 'starting'}
      <div class="listen">
        <span class="bars" aria-hidden="true">
          {#each Array.from({ length: BARS }, (_, i) => i) as i (i)}
            <span style:--h={Math.max(0.15, Math.min(1, voice.level * (1.4 - Math.abs(i - 2) * 0.3)))}></span>
          {/each}
        </span>
        <p class="said" class:placeholder={!voice.interim}>
          {voice.interim || $t('voice.panel.speakNow')}
        </p>
      </div>
      <p class="hint">
        {#if keys.length && voice.mode === 'dictation'}
          {$t('voice.panel.releaseKeys', { keys: keys.join(' ') })}
        {:else if keys.length}
          {$t('voice.panel.releaseCommand', { keys: keys.join(' ') })}
        {:else}
          {$t('voice.panel.releaseOrClick')}
        {/if}
      </p>
    {:else if voice.phase === 'transcribing'}
      <div class="listen">
        <span class="spinner" aria-hidden="true"></span>
        <p class="said placeholder">{$t('voice.panel.transcribing')}</p>
      </div>
    {:else}
      {#if voice.transcript}
        <blockquote class="said">“{voice.transcript}”</blockquote>
      {/if}

      {#if voice.phase === 'error' && voice.errorKey !== 'step'}
        <div class="problem" role="alert">
          <Icon name="alert-triangle" size={14} />
          <div>
            <p>{$t(`voice.err.${voice.errorKey}`)}</p>
            {#if voice.error}<p class="detail">{voice.error}</p>{/if}
            {#if ['noEngine', 'engine', 'insecure'].includes(voice.errorKey)}
              <a href="/settings#voice" onclick={() => voice.dismiss()}>{$t('voice.panel.openSettings')}</a>
            {/if}
          </div>
        </div>
      {/if}

      {#if voice.plan && voice.mode === 'command'}
        <ol class="plan">
          {#each groups as g, gi (gi)}
            {@const e = g.entry}
            <li class="entry">
              <div class="source">
                {#if e.type === 'command'}
                  <span class="tag cmd"><Icon name="mic" size={11} /> {e.command.phrase}</span>
                  {#if e.command.bypass_confirm}<span class="tag quiet">{$t('voice.panel.noConfirm')}</span>{/if}
                {:else if e.type === 'open'}
                  <span class="tag">{$t('voice.panel.builtIn')}</span>
                {:else if e.type === 'agent'}
                  <span class="tag agent"><Icon name="brain" size={11} /> {$t('voice.panel.toAgent')}</span>
                {:else if e.type === 'ambiguous'}
                  <span class="tag bad">{$t('voice.panel.ambiguous')}</span>
                {:else}
                  <span class="tag bad">{$t('voice.panel.unknown')}</span>
                {/if}
              </div>
              {#if e.type === 'ambiguous'}
                <p class="why">
                  {$t('voice.panel.ambiguousBody', { spoken: e.spoken })}
                  {#each e.candidates as c, ci (c.path)}<strong>{c.label}</strong>{ci < e.candidates.length - 1 ? ', ' : ''}{/each}
                </p>
              {:else if e.type === 'unknown'}
                <p class="why">{$t('voice.panel.unknownBody', { text: e.text })}</p>
              {:else}
                <ul class="steps">
                  {#each e.steps as s, si (si)}
                    {@const st = voice.status[g.start + si]}
                    <li class="step {st?.state ?? 'pending'}">
                      <span class="state" aria-hidden="true">
                        {#if st?.state === 'running'}
                          <span class="spinner sm"></span>
                        {:else if st?.state === 'ok'}
                          <Icon name="check" size={13} />
                        {:else if st?.state === 'error'}
                          <Icon name="x" size={13} />
                        {:else}
                          <Icon name={STEP_ICON[s.kind] ?? 'arrow-right'} size={13} />
                        {/if}
                      </span>
                      <span class="label">{stepLabel(s)}</span>
                      {#if st?.note && st.state === 'error'}
                        <span class="note">{st.note}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}

      {#if voice.phase === 'review'}
        <footer>
          <button class="ghost" onclick={() => voice.cancel()}>{$t('common.cancel')} <kbd>Esc</kbd></button>
          {#if !voice.plan?.error}
            <button class="primary" onclick={() => voice.run()}>
              <Icon name="play" size={12} /> {$t('voice.panel.run')} <kbd>↵</kbd>
            </button>
          {:else}
            <button class="primary" onclick={() => voice.press('command')}>
              <Icon name="mic" size={12} /> {$t('voice.panel.retry')}
            </button>
          {/if}
        </footer>
      {:else if voice.phase === 'error'}
        <footer>
          <button class="ghost" onclick={() => voice.dismiss()}>{$t('common.close')}</button>
          {#if voice.mode === 'command'}
            <button class="primary" onclick={() => voice.press('command')}>
              <Icon name="mic" size={12} /> {$t('voice.panel.retry')}
            </button>
          {/if}
        </footer>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .voice-card {
    position: fixed;
    left: 50%;
    bottom: calc(var(--space-6) + env(safe-area-inset-bottom, 0px));
    z-index: 900;
    display: flex;
    width: min(520px, calc(100vw - 32px));
    max-height: min(70vh, 560px);
    flex-direction: column;
    gap: var(--space-3);
    overflow-y: auto;
    transform: translateX(-50%);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: 0 18px 50px -12px rgb(0 0 0 / 0.35);
    padding: var(--space-3) var(--space-4) var(--space-4);
    animation: rise 160ms ease-out;
  }
  .voice-card.error {
    border-color: color-mix(in srgb, var(--red) 35%, var(--border-control));
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 8px);
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .mode {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    color: var(--accent);
    padding: 3px 9px;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .mode.dict {
    background: color-mix(in srgb, var(--amber) 14%, var(--surface));
    color: var(--amber-ink, var(--amber));
  }
  .phase {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .x {
    margin-left: auto;
    display: inline-flex;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--dim);
    cursor: pointer;
  }
  .x:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .listen {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 44px;
  }
  .bars {
    display: inline-flex;
    height: 28px;
    align-items: center;
    gap: 3px;
    flex: none;
  }
  .bars span {
    width: 4px;
    height: calc(var(--h) * 28px);
    border-radius: 2px;
    background: var(--red);
    transition: height 80ms linear;
  }
  .said {
    margin: 0;
    color: var(--text);
    font-size: var(--text-base);
    line-height: 1.45;
  }
  blockquote.said {
    border-left: var(--active-rule, 2px) solid var(--border-strong, var(--border));
    padding-left: var(--space-3);
    font-size: var(--text-lg);
  }
  .said.placeholder {
    color: var(--dim);
  }
  .hint {
    margin: 0;
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .problem {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--red) 8%, var(--surface));
    color: var(--red);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
  }
  .problem p {
    margin: 0;
    color: var(--text);
  }
  .problem .detail {
    margin-top: 4px;
    color: var(--muted);
    font-size: var(--text-xs);
    word-break: break-word;
  }
  .problem a {
    display: inline-block;
    margin-top: 6px;
    color: var(--accent);
    font-size: var(--text-sm);
  }
  .plan {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
    counter-reset: entry;
  }
  .entry {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }
  .source {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
    padding: 2px 7px;
    font-size: var(--text-xs);
  }
  .tag.cmd {
    color: var(--accent);
    font-weight: var(--fw-medium);
  }
  .tag.agent {
    color: var(--text);
  }
  .tag.quiet {
    background: color-mix(in srgb, var(--amber) 12%, var(--surface));
    color: var(--amber-ink, var(--amber));
  }
  .tag.bad {
    background: color-mix(in srgb, var(--red) 10%, var(--surface));
    color: var(--red);
  }
  .why {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .why strong {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .step {
    display: grid;
    grid-template-columns: 18px 1fr;
    align-items: start;
    column-gap: var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
    line-height: 1.4;
  }
  .state {
    display: inline-flex;
    height: 1.4em;
    align-items: center;
    color: var(--dim);
  }
  .step.running .state {
    color: var(--accent);
  }
  .step.ok .state {
    color: var(--green);
  }
  .step.error .state,
  .step.error .note {
    color: var(--red);
  }
  .step.skipped {
    color: var(--dim);
    text-decoration: line-through;
  }
  .label {
    overflow-wrap: anywhere;
  }
  .note {
    grid-column: 2;
    font-size: var(--text-xs);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  kbd {
    margin-left: 4px;
    border: var(--hairline) solid currentColor;
    border-radius: 4px;
    padding: 0 4px;
    font-family: inherit;
    font-size: 0.75em;
    opacity: 0.6;
  }
  .spinner {
    display: inline-block;
    width: 22px;
    height: 22px;
    flex: none;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  .spinner.sm {
    width: 11px;
    height: 11px;
    border-width: 1.5px;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (max-width: 767px) {
    .voice-card {
      bottom: calc(76px + env(safe-area-inset-bottom, 0px));
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .voice-card {
      animation: none;
    }
    .spinner {
      animation-duration: 2s;
    }
  }
</style>
