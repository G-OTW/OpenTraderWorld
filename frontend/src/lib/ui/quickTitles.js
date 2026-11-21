/**
 * Svelte action: the native `title` tooltips under `node` show after `delay` ms instead of
 * the browser's own second or so, which cannot be shortened. The hovered element's title is
 * lifted while the pointer is on it (so the browser's box never shows on top) and put back
 * when it leaves; the text is drawn by the app's one tooltip host (`TipHost`).
 *
 * Delegated: one pair of listeners on `node` covers every button inside it, including those
 * a portal moves in later.
 */
import { tip } from './tip.svelte.js';

export function quickTitles(node, delay = 150) {
  let el = null;
  let stash = '';
  let timer = 0;

  function restore() {
    clearTimeout(timer);
    // A title the component rewrote meanwhile wins over the one lifted.
    if (el && stash && !el.hasAttribute('title')) el.setAttribute('title', stash);
    el = null;
    stash = '';
    tip.hide();
  }

  function over(e) {
    if (el && el.contains(e.target)) return;
    const target = e.target.closest?.('[title]');
    restore();
    if (!target || !node.contains(target)) return;
    const text = target.getAttribute('title');
    if (!text) return;
    el = target;
    stash = text;
    target.removeAttribute('title');
    timer = setTimeout(() => {
      const r = target.getBoundingClientRect();
      tip.show({ clientX: r.left, clientY: r.bottom - 6 }, { text });
    }, delay);
  }

  function out(e) {
    if (el && !el.contains(e.relatedTarget)) restore();
  }

  node.addEventListener('pointerover', over);
  node.addEventListener('pointerout', out);
  node.addEventListener('pointerdown', restore, true);
  return {
    update(d) {
      delay = d;
    },
    destroy() {
      restore();
      node.removeEventListener('pointerover', over);
      node.removeEventListener('pointerout', out);
      node.removeEventListener('pointerdown', restore, true);
    }
  };
}
