// Pre-paint theme and accent, loaded before SvelteKit so the first frame is already in the
// user's colours (no flash of the default indigo on a dark page).
//
// This lives in its own file rather than inline in app.html because of the Content-Security
// Policy: `script-src 'self'` admits no inline script, and the alternative (a hash in the
// policy) would have to be recomputed by hand every time this file is touched. SvelteKit
// hashes its own injected inline script at build time; it does not hash ours.
(function () {
  try {
    var c = localStorage.getItem('otw-theme');
    if (c === 'light' || c === 'dark') document.documentElement.setAttribute('data-theme', c);
    // Pre-paint app-accent: apply the persisted custom primary color (if any) so
    // buttons/links don't flash the default indigo first. Mirrors accent.svelte.js.
    var a = localStorage.getItem('otw-accent');
    if (a && /^#[0-9a-fA-F]{6}$/.test(a)) {
      var el = document.documentElement;
      el.style.setProperty('--accent', a);
      var h = a.slice(1),
        lin = function (v) {
          var s = parseInt(v, 16) / 255;
          return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
        },
        L =
          0.2126 * lin(h.slice(0, 2)) +
          0.7152 * lin(h.slice(2, 4)) +
          0.0722 * lin(h.slice(4, 6));
      el.style.setProperty('--accent-contrast', L > 0.4 ? '#0c0d10' : '#ffffff');
    }
  } catch (e) {}
})();
