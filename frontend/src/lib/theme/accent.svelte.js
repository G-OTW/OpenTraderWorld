/**
 * App-accent store — a user-chosen primary color that overrides the theme's default
 * accent app-wide.
 *
 * How it applies: the theme ships `--accent` (+ derived `--accent-hover`/`--glow`) per
 * light/dark block in default.css. A custom accent is written as an INLINE style on
 * <html> (`--accent` + `--accent-contrast`), which wins over those blocks by inline
 * specificity — so hover/glow, being `color-mix(var(--accent) …)`, follow for free and
 * we never touch the theme blocks at runtime. `--accent-contrast` (the ink that sits ON
 * the accent fill) is computed here from the chosen color's luminance (WCAG), so a light
 * accent gets dark text and vice-versa — no illegible white-on-yellow.
 *
 * Persistence is two-layered: localStorage seeds the value pre-paint (app.html) and
 * gives an instant read on load; the backend (`settings.accent` via /settings/defaults)
 * is the source of truth synced across devices. `null` = follow the theme default.
 */

const STORAGE_KEY = 'otw-accent';
const HEX = /^#[0-9a-fA-F]{6}$/;

/** Relative luminance (WCAG). Returns black or white ink for text sitting ON `hex`. */
export function contrastInk(hex) {
  const c = hex.replace('#', '');
  const lin = (v) => {
    const s = parseInt(v, 16) / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const L = 0.2126 * lin(c.slice(0, 2)) + 0.7152 * lin(c.slice(2, 4)) + 0.0722 * lin(c.slice(4, 6));
  // Threshold at 0.4: contrast against white (1.05/(L+0.05)) vs black ((L+0.05)/0.05)
  // cross near here; below → dark ink reads better, above → white text on a light fill.
  return L > 0.4 ? '#0c0d10' : '#ffffff';
}

function apply(hex) {
  if (typeof document === 'undefined') return;
  const el = document.documentElement;
  if (hex && HEX.test(hex)) {
    el.style.setProperty('--accent', hex);
    el.style.setProperty('--accent-contrast', contrastInk(hex));
  } else {
    el.style.removeProperty('--accent');
    el.style.removeProperty('--accent-contrast');
  }
}

function readCache() {
  if (typeof localStorage === 'undefined') return null;
  const v = localStorage.getItem(STORAGE_KEY);
  return v && HEX.test(v) ? v : null;
}

class AccentStore {
  /** Custom accent hex, or null to follow the theme default. */
  value = $state(readCache());

  /** Preset shortcuts shown next to the free picker. First is the theme default
   *  (null = muted gold). The rest are subdued, institutional tones. */
  presets = [
    { name: 'gold', hex: null },
    { name: 'sage', hex: '#7fb894' },
    { name: 'brick', hex: '#c9776b' },
    { name: 'slate', hex: '#7d8a99' },
    { name: 'khaki', hex: '#b7a878' },
    { name: 'ivory', hex: '#c6c3b8' }
  ];

  /** Set (and persist) the accent. Pass null/'' to clear back to the theme default. */
  set(hex) {
    const next = hex && HEX.test(hex) ? hex : null;
    this.value = next;
    if (typeof localStorage !== 'undefined') {
      if (next) localStorage.setItem(STORAGE_KEY, next);
      else localStorage.removeItem(STORAGE_KEY);
    }
    apply(next);
  }

  /** Re-assert the cached accent against the DOM (app.html already applied it pre-paint). */
  init() {
    apply(this.value);
  }

  /** Adopt the backend value (source of truth) once it loads; keeps the cache in sync. */
  hydrate(hex) {
    const next = hex && HEX.test(hex) ? hex : null;
    this.value = next;
    if (typeof localStorage !== 'undefined') {
      if (next) localStorage.setItem(STORAGE_KEY, next);
      else localStorage.removeItem(STORAGE_KEY);
    }
    apply(next);
  }
}

export const accent = new AccentStore();
