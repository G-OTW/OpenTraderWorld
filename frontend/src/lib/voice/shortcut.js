/**
 * Push-to-talk shortcuts, stored as `Mod+Mod+Code` with `KeyboardEvent.code` for the key so
 * the binding is the same physical key on every layout (`Alt+KeyV`).
 */

const MODS = ['Ctrl', 'Alt', 'Shift', 'Meta'];
const MOD_CODES = new Set([
  'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'ShiftLeft', 'ShiftRight',
  'MetaLeft', 'MetaRight'
]);

export function parseShortcut(s) {
  const parts = String(s || '').split('+').filter(Boolean);
  const code = parts.pop() ?? '';
  const mods = new Set(parts);
  return { code, ctrl: mods.has('Ctrl'), alt: mods.has('Alt'), shift: mods.has('Shift'), meta: mods.has('Meta') };
}

/** The shortcut a key press spells, or null for a lone modifier. */
export function shortcutFromEvent(e) {
  if (MOD_CODES.has(e.code) || !e.code) return null;
  const mods = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Meta');
  return [...mods, e.code].join('+');
}

/** A bare letter would swallow typing: require a modifier unless it is a function key. */
export function shortcutUsable(s) {
  const p = parseShortcut(s);
  if (!p.code) return false;
  return p.ctrl || p.alt || p.meta || /^F\d{1,2}$/.test(p.code);
}

export function matchesShortcut(e, s) {
  const p = parseShortcut(s);
  return (
    e.code === p.code &&
    e.ctrlKey === p.ctrl &&
    e.altKey === p.alt &&
    e.shiftKey === p.shift &&
    e.metaKey === p.meta
  );
}

const isMac = () => typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

function keyLabel(code) {
  if (code.startsWith('Key')) return code.slice(3);
  if (code.startsWith('Digit')) return code.slice(5);
  if (code.startsWith('Numpad')) return `Num ${code.slice(6)}`;
  return { Space: 'Space', Backquote: '`', Period: '.', Comma: ',', Slash: '/', Semicolon: ';', Quote: "'", BracketLeft: '[', BracketRight: ']', Backslash: '\\', Minus: '-', Equal: '=' }[code] ?? code;
}

/** Key caps for display: `['⌥', 'V']` on a Mac, `['Alt', 'V']` elsewhere. */
export function shortcutKeys(s) {
  const p = parseShortcut(s);
  if (!p.code) return [];
  const mac = isMac();
  const names = mac
    ? { Ctrl: '⌃', Alt: '⌥', Shift: '⇧', Meta: '⌘' }
    : { Ctrl: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Meta: 'Win' };
  return [...MODS.filter((m) => p[m.toLowerCase()]).map((m) => names[m]), keyLabel(p.code)];
}

/** A field that takes typed text: where the shortcut dictates instead of commanding. */
export function isTextTarget(el) {
  if (!el || el === document.body) return false;
  if (el.isContentEditable) return true;
  if (el.tagName === 'TEXTAREA') return !el.readOnly && !el.disabled;
  if (el.tagName === 'INPUT') {
    const type = (el.type || 'text').toLowerCase();
    return ['text', 'search', 'email', 'url', 'tel'].includes(type) && !el.readOnly && !el.disabled;
  }
  return false;
}
