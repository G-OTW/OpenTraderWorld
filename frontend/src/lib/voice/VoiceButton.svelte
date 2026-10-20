<script>
  // Top-bar microphone and the two push-to-talk shortcuts.
  //
  // Mouse: a click starts listening and the next click stops; holding the button past
  // HOLD_MS turns the release into the stop instead (walkie-talkie). The button always
  // commands. Keyboard: each shortcut is held while speaking. The command one always
  // commands, even from inside a text field; the dictation one always types into the
  // focused field and is never read as a command. Escape cancels whatever is in flight.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { voice } from './voice.svelte.js';
  import { matchesShortcut, parseShortcut, shortcutKeys } from './shortcut.js';

  const HOLD_MS = 400;

  let pressedAt = 0;
  let held = ''; // the shortcut being held, or ''

  const listening = $derived(voice.phase === 'listening' || voice.phase === 'starting');
  const keys = $derived(shortcutKeys(voice.settings?.shortcut ?? ''));
  const title = $derived(
    listening
      ? $t('voice.mic.stop')
      : `${$t('voice.mic.start')}${keys.length ? ` (${keys.join(' ')})` : ''}`
  );

  function onPointerDown(e) {
    if (e.button !== 0) return;
    if (listening) {
      voice.release();
      pressedAt = 0;
      return;
    }
    pressedAt = performance.now();
    voice.press('command');
  }

  function onPointerUp() {
    if (pressedAt && performance.now() - pressedAt > HOLD_MS) voice.release();
    pressedAt = 0;
  }

  function onKeydown(e) {
    if (!voice.enabled || voice.suspended) return;
    if (held && e.code === parseShortcut(held).code) {
      e.preventDefault(); // auto-repeat while held
      return;
    }
    const { shortcut, dictation_shortcut } = voice.settings;
    const mode = matchesShortcut(e, shortcut) ? 'command' : matchesShortcut(e, dictation_shortcut) ? 'dictation' : '';
    if (mode && !e.repeat) {
      e.preventDefault();
      e.stopPropagation();
      held = mode === 'command' ? shortcut : dictation_shortcut;
      voice.press(mode);
      return;
    }
    if (e.key === 'Escape' && voice.busy && voice.phase !== 'running') {
      e.preventDefault();
      voice.cancel();
    }
  }

  function onKeyup(e) {
    if (!held) return;
    const p = parseShortcut(held);
    const modGone = (p.alt && !e.altKey) || (p.ctrl && !e.ctrlKey) || (p.meta && !e.metaKey) || (p.shift && !e.shiftKey);
    if (e.code === p.code || modGone) {
      held = '';
      voice.release();
    }
  }

  // Switching tabs mid-press never delivers the keyup: treat it as the release.
  function onBlur() {
    if (held) {
      held = '';
      voice.release();
    }
  }
</script>

<svelte:window onkeydowncapture={onKeydown} onkeyupcapture={onKeyup} onblur={onBlur} />

{#if voice.enabled}
  <button
    class="mic"
    class:live={listening}
    class:busy={voice.phase === 'transcribing' || voice.phase === 'running'}
    style:--lvl={voice.level}
    {title}
    aria-label={title}
    aria-pressed={listening}
    onpointerdown={onPointerDown}
    onpointerup={onPointerUp}
    onpointerleave={onPointerUp}
  >
    <span class="ring" aria-hidden="true"></span>
    <Icon name="mic" size={17} />
  </button>
{/if}

<style>
  .mic {
    position: relative;
    display: inline-flex;
    width: 32px;
    height: 32px;
    align-items: center;
    justify-content: center;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    touch-action: none;
  }
  .mic:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .mic.busy {
    color: var(--accent);
  }
  .mic.live {
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 40%, var(--border-control));
    background: color-mix(in srgb, var(--red) 10%, var(--surface));
  }
  /* The ring breathes with the input level, so a silent mic is visible at a glance. */
  .ring {
    position: absolute;
    inset: -3px;
    border-radius: calc(var(--radius-sm) + 3px);
    border: 2px solid var(--red);
    opacity: 0;
    transform: scale(0.9);
    pointer-events: none;
  }
  .mic.live .ring {
    opacity: calc(0.25 + var(--lvl) * 0.75);
    transform: scale(calc(1 + var(--lvl) * 0.18));
    transition: transform 80ms linear, opacity 80ms linear;
  }
  @media (prefers-reduced-motion: reduce) {
    .mic.live .ring {
      transform: none;
      transition: none;
    }
  }
</style>
