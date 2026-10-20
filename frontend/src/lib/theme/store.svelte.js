/**
 * Theme store — light / dark / system, client-side only (no backend).
 *
 * The choice persists in localStorage and is applied by stamping `data-theme`
 * on <html>:
 *   - 'light' / 'dark' → explicit, stamps that value.
 *   - 'system'         → removes the attribute; default.css then follows the OS
 *                        via `prefers-color-scheme`.
 *
 * A tiny inline snippet in app.html applies the persisted choice before first
 * paint to avoid a flash; this store keeps it in sync at runtime.
 */

const STORAGE_KEY = 'otw-theme';
const CHOICES = ['light', 'dark', 'system'];

function read() {
  if (typeof localStorage === 'undefined') return 'system';
  const v = localStorage.getItem(STORAGE_KEY);
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

  /** The concrete theme in effect right now ('light' | 'dark'), resolving 'system'. */
  get resolved() {
    if (this.choice !== 'system') return this.choice;
    if (typeof matchMedia === 'undefined') return 'light';
    return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  set(choice) {
    if (!CHOICES.includes(choice)) return;
    this.choice = choice;
    if (typeof localStorage !== 'undefined') localStorage.setItem(STORAGE_KEY, choice);
    apply(choice);
  }

  /** Re-assert the current choice against the DOM (call once on mount). */
  init() {
    apply(this.choice);
  }
}

export const theme = new ThemeStore();
