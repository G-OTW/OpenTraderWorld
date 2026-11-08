// Runs code as if the browser's number locale were `tag`.
//
// format.js formats numbers and money with `toLocaleString(undefined, ...)`: separators
// follow the browser locale, by design. Node has one process-wide default, so a test that
// compares fr-FR with en-US swaps the default for the duration of `fn` only. An explicit
// locale argument is left untouched.
export function withNumberLocale(tag, fn) {
  const original = Number.prototype.toLocaleString;
  Number.prototype.toLocaleString = function (locales, options) {
    return original.call(this, locales ?? tag, options);
  };
  try {
    return fn();
  } finally {
    Number.prototype.toLocaleString = original;
  }
}

/**
 * Pins the process default number locale to `tag` for the whole test file, whatever the
 * machine's LANG says. Node reads its default locale once at startup, so this is the only
 * way to keep `toLocaleString(undefined)` output fixed. Called from the unit setup.
 */
export function pinDefaultNumberLocale(tag) {
  const toLocale = Number.prototype.toLocaleString;
  Number.prototype.toLocaleString = function (locales, options) {
    return toLocale.call(this, locales ?? tag, options);
  };
  const NumberFormat = Intl.NumberFormat;
  function Pinned(locales, options) {
    return new NumberFormat(locales ?? tag, options);
  }
  Pinned.prototype = NumberFormat.prototype;
  Pinned.supportedLocalesOf = NumberFormat.supportedLocalesOf;
  Intl.NumberFormat = Pinned;
}
