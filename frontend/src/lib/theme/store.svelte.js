/**
 * Theme store: light / dark / system.
 *
 * The choice persists as the `otw-theme` user preference (`$lib/prefs.svelte.js`: the
 * database, mirrored to localStorage under the same key and raw value) and is applied
 * by stamping `data-theme` on <html>:
 *   - 'light' / 'dark' → explicit, stamps that value.
 *   - 'system'         → removes the attribute; default.css then follows the OS
 *                        via `prefers-color-scheme`.
 *
 * A tiny inline snippet in app.html applies the persisted choice before first
 * paint to avoid a flash; this store keeps it in sync at runtime.
 */

import { getPref, onPref, setPref } from '$lib/prefs.svelte.js';

const STORAGE_KEY = 'otw-theme';
const CHOICES = ['light', 'dark', 'system'];

function read() {
  const v = getPref(STORAGE_KEY, 'system');
  return CHOICES.includes(v) ? v : 'system';
}

function apply(choice) {
  if (typeof document === 'undefined') return;
  const el = document.documentElement;
  if (choice === 'system') el.removeAttribute('data-theme');
  else el.setAttribute('data-theme', choice);
}

class ThemeStore {
  choice = $state(read());

  constructor() {
    // The server's choice arrives after first paint (another browser may have changed it).
    onPref(STORAGE_KEY, () => {
      this.choice = read();
      apply(this.choice);
    });
  }

  /** The concrete theme in effect right now ('light' | 'dark'), resolving 'system'. */
  get resolved() {
    if (this.choice !== 'system') return this.choice;
    if (typeof matchMedia === 'undefined') return 'light';
    return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  set(choice) {
    if (!CHOICES.includes(choice)) return;
    this.choice = choice;
    setPref(STORAGE_KEY, choice);
    apply(choice);
  }

  /** Re-assert the current choice against the DOM (call once on mount). */
  init() {
    apply(this.choice);
  }
}

export const theme = new ThemeStore();
