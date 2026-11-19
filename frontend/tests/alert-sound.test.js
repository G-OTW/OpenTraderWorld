import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  ALERT_SOUNDS,
  getAlertSound,
  isAlertNotification,
  playAlertSound,
  setAlertSound
} from '../src/lib/notifications/alertSound.js';

beforeEach(() => {
  const data = new Map();
  globalThis.localStorage = {
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => data.set(k, v)
  };
});

test('offers seven sounds, each a second or less', () => {
  assert.equal(ALERT_SOUNDS.length, 7);
  assert.equal(new Set(ALERT_SOUNDS.map((s) => s.id)).size, 7);
  for (const s of ALERT_SOUNDS) {
    const end = Math.max(...s.tones.map((t) => t.at + t.dur));
    assert.ok(end <= 1, `${s.id} lasts ${end}s`);
  }
});

test('stays silent until a sound is chosen, and on a stale choice', () => {
  assert.equal(getAlertSound(), 'off');
  setAlertSound('coin');
  assert.equal(getAlertSound(), 'coin');
  setAlertSound('removed-sound');
  assert.equal(getAlertSound(), 'off');
});

test('rings for the three alert kinds only', () => {
  for (const kind of ['chart', 'watchlist', 'scanner']) assert.ok(isAlertNotification({ kind }));
  for (const kind of ['reminder', 'automator', undefined]) assert.ok(!isAlertNotification({ kind }));
});

test('a blocked audio context plays nothing, now or at the next click', async () => {
  let tones = 0;
  globalThis.window = {
    AudioContext: class {
      state = 'suspended';
      resume = () => new Promise(() => {}); // what a browser does until a gesture
      createOscillator() {
        tones += 1;
      }
    }
  };
  await playAlertSound('coin');
  assert.equal(tones, 0);
});
