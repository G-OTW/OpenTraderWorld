/**
 * Key-based i18n.
 *
 * Five UI language packs. English (`en`) is the source of truth; other packs may be
 * partial — any missing key falls back to `en`, and if `en` lacks it too, to the raw key.
 *
 * Components render user-visible text with `$t('some.key')`, never hardcoded strings.
 * Internal identifiers, log messages and technical labels stay English in code and are
 * not keyed.
 *
 * The chosen locale is persisted server-side (settings key `locale`) and mirrored to
 * localStorage so the correct pack applies on first paint, before the backend responds.
 */
import { derived, writable } from 'svelte/store';
import en from './en.json';
import fr from './fr.json';
import it from './it.json';
import es from './es.json';
import de from './de.json';
import pt from './pt.json';
import zh from './zh.json';

const packs = { en, fr, it, es, de, pt, zh };

/**
 * Supported locales, in display order. `label` is the language's own endonym; `flag` is a
 * regional-indicator emoji for the selector. `html` is the value written to `<html lang>`
 * (BCP-47 tag; defaults to `code` when omitted). Keep codes in sync with the backend
 * `SUPPORTED_LOCALES`.
 *
 * pt/zh both use the Latin/CJK glyphs already covered by the system font fallback — no RTL,
 * no font shipping needed.
 */
export const LOCALES = [
  { code: 'en', label: 'English', flag: '🇬🇧' },
  { code: 'fr', label: 'Français', flag: '🇫🇷' },
  { code: 'it', label: 'Italiano', flag: '🇮🇹' },
  { code: 'es', label: 'Español', flag: '🇪🇸' },
  { code: 'de', label: 'Deutsch', flag: '🇩🇪' },
  { code: 'pt', label: 'Português (BR)', flag: '🇧🇷', html: 'pt-BR' },
  { code: 'zh', label: '中文', flag: '🇨🇳', html: 'zh-Hans' }
];

const CODES = LOCALES.map((l) => l.code);
const STORAGE_KEY = 'otw.locale';

function initialLocale() {
  if (typeof localStorage === 'undefined') return 'en';
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved && CODES.includes(saved) ? saved : 'en';
}

export const locale = writable(initialLocale());

/** BCP-47 tag for a locale code, for `<html lang>` (falls back to the code itself). */
function htmlLang(code) {
  return LOCALES.find((l) => l.code === code)?.html ?? code;
}

// Keep `<html lang>` in sync with the active locale (accessibility + correct CJK/locale
// glyph selection). Fires on init and on every switch.
if (typeof document !== 'undefined') {
  locale.subscribe((code) => {
    document.documentElement.setAttribute('lang', htmlLang(code));
  });
}

/**
 * Translation function as a store: `$t('some.key', { name: 'x' })`.
 *
 * Lookup order: active pack → English → raw key. Optional `params` interpolate `{name}`
 * placeholders in the resolved string.
 */
export const t = derived(locale, ($locale) => {
  const pack = packs[$locale] ?? en;
  return (key, params) => {
    let s = pack[key] ?? en[key] ?? key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        s = s.replaceAll(`{${k}}`, v);
      }
    }
    return s;
  };
});

/**
 * Switch the active UI language. Updates the store immediately (re-renders all `$t`),
 * persists to localStorage, and — unless `persist` is false — writes the choice to the
 * backend so it survives across devices/reloads.
 */
export async function setLocale(code, { persist = true } = {}) {
  if (!CODES.includes(code)) code = 'en';
  locale.set(code);
  if (typeof localStorage !== 'undefined') localStorage.setItem(STORAGE_KEY, code);
  if (persist) {
    try {
      await fetch('/api/settings/defaults', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ locale: code })
      });
    } catch {
      /* best-effort; localStorage still holds the choice */
    }
  }
}

/**
 * Load the persisted locale from the backend at app boot and apply it. Falls back to the
 * localStorage/`en` value already in the store on any failure.
 */
export async function initLocale() {
  try {
    const res = await fetch('/api/settings/defaults', { headers: { 'content-type': 'application/json' } });
    if (!res.ok) return;
    const d = await res.json();
    if (d?.locale && CODES.includes(d.locale)) setLocale(d.locale, { persist: false });
  } catch {
    /* keep current locale */
  }
}
