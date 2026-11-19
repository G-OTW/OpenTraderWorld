// The short sound an alert plays in the open app. Every sound is synthesized here, so
// there are no audio files to ship or license. The choice is a user preference
// (`$lib/prefs.svelte.js`), so it follows the user to another browser.
//
// A tone is one oscillator with a quick attack and a decay to silence: `at` and `dur` are
// seconds from the start, `to` glides the pitch. A sound stays under a second.
import { getPref, onPref, setPref } from '$lib/prefs.svelte.js';

const KEY = 'otw.alert.sound';

export const ALERT_SOUNDS = [
  {
    id: 'coin',
    tones: [
      { type: 'square', freq: 1175, at: 0, dur: 0.08, gain: 0.1 },
      { type: 'square', freq: 1568, at: 0.08, dur: 0.4, gain: 0.1 }
    ]
  },
  {
    id: 'ring',
    tones: [0, 0.1, 0.2, 0.3, 0.4, 0.5].flatMap((at) => [
      { type: 'sine', freq: 1000, at, dur: 0.09, gain: 0.16 },
      { type: 'sine', freq: 1250, at, dur: 0.09, gain: 0.16 }
    ])
  },
  {
    id: 'bell',
    tones: [
      { type: 'sine', freq: 880, at: 0, dur: 0.9, gain: 0.25 },
      { type: 'sine', freq: 1760, at: 0, dur: 0.6, gain: 0.12 },
      { type: 'sine', freq: 2640, at: 0, dur: 0.35, gain: 0.06 }
    ]
  },
  {
    id: 'chime',
    tones: [
      { type: 'sine', freq: 784, at: 0, dur: 0.3, gain: 0.25 },
      { type: 'sine', freq: 988, at: 0.12, dur: 0.3, gain: 0.25 },
      { type: 'sine', freq: 1175, at: 0.24, dur: 0.5, gain: 0.25 }
    ]
  },
  { id: 'ping', tones: [{ type: 'sine', freq: 1320, at: 0, dur: 0.35, gain: 0.3 }] },
  {
    id: 'alarm',
    tones: [0, 0.2, 0.4].map((at) => ({ type: 'square', freq: 880, at, dur: 0.14, gain: 0.1 }))
  },
  {
    id: 'scream',
    tones: [
      { type: 'sawtooth', freq: 1100, to: 420, at: 0, dur: 0.6, gain: 0.08 },
      { type: 'sawtooth', freq: 1135, to: 440, at: 0, dur: 0.6, gain: 0.08 }
    ]
  }
];

/** The notification kinds written when an alert fires (chart, watchlist, scanner). */
const ALERT_KINDS = new Set(['chart', 'watchlist', 'scanner']);

export const isAlertNotification = (n) => ALERT_KINDS.has(n?.kind);

/** The chosen sound's id, or `off`. */
export function getAlertSound() {
  const id = getPref(KEY, 'off');
  return ALERT_SOUNDS.some((s) => s.id === id) ? id : 'off';
}

export function setAlertSound(id) {
  setPref(KEY, id);
}

/** Calls `fn(id)` when the choice changes (here or from the server); returns the unsubscribe. */
export function onAlertSound(fn) {
  return onPref(KEY, () => fn(getAlertSound()));
}

let ctx = null;

function audio() {
  const Ctx = window.AudioContext ?? window.webkitAudioContext;
  if (!ctx && Ctx) ctx = new Ctx();
  return ctx;
}

/** Browsers keep audio suspended until a gesture, Safari until one resumes it directly. */
export function armAlertSound() {
  const events = ['pointerdown', 'keydown'];
  const unlock = () => {
    events.forEach((e) => window.removeEventListener(e, unlock));
    if (getAlertSound() !== 'off') audio()?.resume().catch(() => {});
  };
  events.forEach((e) => window.addEventListener(e, unlock));
}

/** Play a sound; an unknown id (`off`) plays nothing. */
export async function playAlertSound(id) {
  const sound = ALERT_SOUNDS.find((s) => s.id === id);
  const c = sound && audio();
  if (!c) return;
  // A blocked resume never settles. Give up on it, or the tones would wait in the context
  // and all play at the user's next click, long after the alert.
  if (c.state !== 'running') {
    await Promise.race([c.resume(), new Promise((done) => setTimeout(done, 300))]).catch(() => {});
    if (c.state !== 'running') return;
  }
  const start = c.currentTime + 0.01;
  for (const { type, freq, to, at, dur, gain } of sound.tones) {
    const osc = c.createOscillator();
    const env = c.createGain();
    const from = start + at;
    osc.type = type;
    osc.frequency.setValueAtTime(freq, from);
    if (to) osc.frequency.exponentialRampToValueAtTime(to, from + dur);
    env.gain.setValueAtTime(0.0001, from);
    env.gain.linearRampToValueAtTime(gain, from + 0.005);
    env.gain.exponentialRampToValueAtTime(0.0001, from + dur);
    osc.connect(env).connect(c.destination);
    osc.start(from);
    osc.stop(from + dur + 0.02);
  }
}
