/**
 * Voice control: one press, one utterance, one plan.
 *
 *   press → listening → (release) → transcribing → review → running → done
 *
 * The mode is fixed when the press starts, by which shortcut started it. `command` builds a
 * plan from the sentence; `dictation` types the text into the field that had focus and never
 * reads it as a command. The top-bar microphone always commands.
 *
 * A plan stops on review unless every part of it is a saved command marked to bypass the
 * confirmation. A plan with a part that cannot run (an ambiguous page, nothing understood)
 * is shown and never run.
 */
import { get } from 'svelte/store';
import { goto } from '$app/navigation';
import { t, locale } from '$lib/i18n';
import { modules, visibleModules } from '$lib/modules/registry';
import { installedIds } from '$lib/modules/installed.js';
import { theme } from '$lib/theme/store.svelte.js';
import { privacy, togglePrivacy } from '$lib/theme/privacy.svelte.js';
import { automatorApi } from '$lib/modules/automator/api.js';
import { assistantBus } from '$lib/modules/agent/assistantBus.svelte.js';
import { voiceApi } from './api.js';
import { browserRecognizer, micBlocker, speak, startRecording } from './audio.js';
import { buildPlan, navTargets } from './parse.js';
import { isTextTarget } from './shortcut.js';

/** Shorter than this is a tap on the key, not speech. */
const MIN_MS = 350;
/** One utterance; past this the recording stops by itself. */
const MAX_MS = 60_000;
/** A finished plan stays on screen this long before the panel folds away. */
const DONE_MS = 2600;

class VoiceStore {
  settings = $state(null);
  commands = $state([]);
  engines = $state([]);

  phase = $state('idle'); // idle | starting | listening | transcribing | review | running | done | error
  mode = $state('command'); // command | dictation
  level = $state(0);
  interim = $state('');
  transcript = $state('');
  plan = $state(null);
  status = $state([]); // per step: { state: 'pending'|'running'|'ok'|'error', note }
  error = $state('');
  errorKey = $state('');
  /** True while a settings field captures a new shortcut: the old one must not fire. */
  suspended = $state(false);

  #rec = null; // recording handle, or the browser recognizer
  #target = null; // the field dictation types into
  #stopWanted = false;
  #limitTimer = null;
  #doneTimer = null;
  #browserText = '';

  get enabled() {
    return !!this.settings?.enabled;
  }
  get busy() {
    return this.phase !== 'idle';
  }
  get lang() {
    return this.settings?.language || get(locale) || 'en';
  }

  async load() {
    try {
      const [s, c] = await Promise.all([voiceApi.settings(), voiceApi.commands()]);
      this.settings = s;
      this.commands = c ?? [];
    } catch {
      /* the settings page says what is wrong; the microphone just stays hidden */
    }
  }

  // ── Capture ─────────────────────────────────────────────────────────────────

  async press(mode = 'command') {
    if (this.phase === 'starting' || this.phase === 'listening' || this.phase === 'transcribing') return;
    if (this.phase === 'running') return;
    this.#reset();
    this.mode = mode;
    this.#target = mode === 'dictation' ? document.activeElement : null;
    if (mode === 'dictation' && !isTextTarget(this.#target)) return this.#fail('noField');

    const engine = this.settings?.engine ?? '';
    if (!engine) return this.#fail('noEngine');
    const blocked = micBlocker();
    if (blocked) return this.#fail(blocked === 'insecure' ? 'insecure' : 'noMic');

    this.phase = 'starting';
    this.#stopWanted = false;
    try {
      if (engine === 'browser') await this.#startBrowser();
      else this.#rec = await startRecording({ onLevel: (v) => (this.level = this.level * 0.6 + v * 0.4) });
    } catch (e) {
      this.#rec = null;
      return this.#fail(e?.name === 'NotAllowedError' || e?.error === 'not-allowed' ? 'denied' : 'noMic', e?.message);
    }
    if (this.phase !== 'starting') return; // cancelled while the mic was opening
    this.phase = 'listening';
    this.#limitTimer = setTimeout(() => this.release(), MAX_MS);
    if (this.#stopWanted) this.release();
  }

  async release() {
    if (this.phase === 'starting') {
      this.#stopWanted = true;
      return;
    }
    if (this.phase !== 'listening') return;
    clearTimeout(this.#limitTimer);
    const rec = this.#rec;
    this.#rec = null;
    this.level = 0;

    if (this.settings.engine === 'browser') {
      this.phase = 'transcribing';
      const text = await rec.finish();
      return this.#heard(text);
    }
    if (rec.ms < MIN_MS) {
      rec.cancel();
      return this.#fail('tooShort');
    }
    this.phase = 'transcribing';
    try {
      const wav = await rec.stop();
      const r = await voiceApi.transcribe(wav, this.settings.language);
      if (this.phase !== 'transcribing') return; // cancelled meanwhile
      this.#heard(r.text);
    } catch (e) {
      this.#fail('engine', e.message);
    }
  }

  /** The browser recognizer streams words as they come; `finish()` waits for the last. */
  #startBrowser() {
    const Rec = browserRecognizer();
    if (!Rec) return Promise.reject(Object.assign(new Error('unsupported'), { name: 'Unsupported' }));
    const rec = new Rec();
    rec.lang = this.lang;
    rec.continuous = true;
    rec.interimResults = true;
    this.#browserText = '';
    let finals = '';
    let ended;
    const done = new Promise((r) => (ended = r));
    rec.onresult = (e) => {
      let interim = '';
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const r = e.results[i];
        if (r.isFinal) finals += r[0].transcript;
        else interim += r[0].transcript;
      }
      this.#browserText = finals;
      this.interim = (finals + interim).trim();
      this.level = 0.5;
    };
    rec.onend = () => ended(this.#browserText || this.interim);
    return new Promise((resolve, reject) => {
      rec.onerror = (e) => {
        if (this.phase === 'starting') reject(e);
        else if (e.error !== 'no-speech' && e.error !== 'aborted') this.#fail('engine', e.error);
      };
      rec.onstart = () => {
        this.#rec = {
          finish: () => {
            rec.stop();
            return done;
          },
          cancel: () => rec.abort()
        };
        resolve();
      };
      rec.start();
    });
  }

  #heard(raw) {
    const text = String(raw ?? '').trim();
    this.transcript = text;
    this.interim = '';
    if (!text) return this.#fail('nothing');
    if (this.mode === 'dictation') return this.#dictate(text);

    const plan = buildPlan(text, {
      commands: this.commands,
      targets: this.#targets(),
      lang: this.lang,
      agentFallback: this.settings.agent_fallback
    });
    this.plan = plan;
    this.status = plan.steps.map(() => ({ state: 'pending', note: '' }));
    if (!plan.error && !plan.confirm) this.run();
    else this.phase = 'review';
  }

  #targets() {
    const tr = get(t);
    const installed = visibleModules(get(installedIds)).filter((m) => !m.home);
    return navTargets(installed.length ? installed : modules.filter((m) => !m.home), [
      { label: tr('nav.settings'), path: '/settings', keys: ['settings', tr('nav.settings')] },
      { label: tr('voice.target.dashboard'), path: '/', keys: ['dashboard', 'home', tr('voice.target.dashboard')] },
      {
        label: tr('settings.nav.voiceCommands'),
        path: '/settings#voice-commands',
        keys: ['voice commands', tr('settings.nav.voiceCommands')]
      }
    ]);
  }

  // ── Dictation ───────────────────────────────────────────────────────────────

  #dictate(text) {
    const el = this.#target;
    if (!el?.isConnected) return this.#fail('focusLost');
    el.focus();
    if (el.isContentEditable) {
      const before = window.getSelection()?.anchorNode?.textContent ?? '';
      const lead = before && !/\s$/.test(before) ? ' ' : '';
      document.execCommand('insertText', false, lead + text);
    } else {
      const start = el.selectionStart ?? el.value.length;
      const end = el.selectionEnd ?? start;
      const lead = start > 0 && !/\s$/.test(el.value.slice(0, start)) ? ' ' : '';
      el.setRangeText(lead + text, start, end, 'end');
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }
    this.phase = 'done';
    this.#doneTimer = setTimeout(() => this.dismiss(), 1200);
  }

  // ── Running a plan ──────────────────────────────────────────────────────────

  async run() {
    if (!this.plan || this.plan.error || this.phase === 'running') return;
    this.phase = 'running';
    const steps = this.plan.steps;
    for (let i = 0; i < steps.length; i++) {
      if (this.phase !== 'running') return; // cancelled
      this.status[i] = { state: 'running', note: '' };
      try {
        const note = await this.#exec(steps[i]);
        this.status[i] = { state: 'ok', note: note ?? '' };
      } catch (e) {
        this.status[i] = { state: 'error', note: this.#stepError(e) };
        for (let k = i + 1; k < steps.length; k++) this.status[k] = { state: 'skipped', note: '' };
        this.phase = 'error';
        this.errorKey = 'step';
        this.error = this.status[i].note;
        if (this.settings.speak) speak(get(t)('voice.say.failed', { n: i + 1 }), this.lang);
        return;
      }
    }
    this.phase = 'done';
    if (this.settings.speak && !steps.some((s) => s.kind === 'say')) speak(get(t)('voice.say.done'), this.lang);
    this.#doneTimer = setTimeout(() => this.dismiss(), DONE_MS);
  }

  async #exec(step) {
    switch (step.kind) {
      case 'navigate':
        await goto(step.target);
        return '';
      case 'agent':
        await assistantBus.ask(step.prompt);
        return '';
      case 'automator': {
        const run = await automatorApi.run(step.workflow_id);
        return run?.status ?? '';
      }
      case 'theme': {
        const next = step.value === 'toggle' ? (theme.resolved === 'dark' ? 'light' : 'dark') : step.value;
        theme.set(next);
        return '';
      }
      case 'privacy': {
        const want = step.value === 'toggle' ? !privacy.hidden : step.value === 'on';
        if (want !== privacy.hidden) togglePrivacy();
        return '';
      }
      case 'say':
        await speak(step.text, this.lang);
        return '';
      default:
        throw new Error(get(t)('voice.err.unknownStep', { kind: step.kind }));
    }
  }

  #stepError(e) {
    const tr = get(t);
    const known = { unavailable: 'voice.err.agentHidden', busy: 'voice.err.agentBusy', 'not-ready': 'voice.err.agentNotReady' };
    return known[e?.message] ? tr(known[e.message]) : e?.message || tr('voice.err.generic');
  }

  // ── Panel ───────────────────────────────────────────────────────────────────

  cancel() {
    clearTimeout(this.#limitTimer);
    this.#rec?.cancel?.();
    this.#rec = null;
    this.dismiss();
  }

  dismiss() {
    clearTimeout(this.#doneTimer);
    this.#reset();
    this.phase = 'idle';
  }

  #fail(key, detail = '') {
    this.phase = 'error';
    this.errorKey = key;
    this.error = detail;
    this.level = 0;
  }

  #reset() {
    clearTimeout(this.#doneTimer);
    this.level = 0;
    this.interim = '';
    this.transcript = '';
    this.plan = null;
    this.status = [];
    this.error = '';
    this.errorKey = '';
  }
}

export const voice = new VoiceStore();
