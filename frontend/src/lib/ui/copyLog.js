/** Svelte action: click an error/log element to copy its full text to the clipboard.
 *
 * Usage: <div use:copyLog={fullText}>short preview…</div>
 * The element gets `cursor: pointer`, a "click to copy" title hint, and a brief
 * `data-copied` flash on success that callers can style. Pass the full untruncated
 * text as the argument (the visible text is often clipped). Falls back to the
 * element's own textContent when no argument is given. */
export function copyLog(node, text) {
  let current = text;

  async function doCopy() {
    const payload = (current ?? node.textContent ?? '').trim();
    if (!payload) return;
    try {
      await navigator.clipboard.writeText(payload);
    } catch {
      // Fallback for insecure contexts / older browsers.
      const ta = document.createElement('textarea');
      ta.value = payload;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand('copy');
      } catch {
        /* give up silently */
      }
      ta.remove();
    }
    node.setAttribute('data-copied', 'true');
    setTimeout(() => node.removeAttribute('data-copied'), 1200);
  }

  function onKey(e) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      doCopy();
    }
  }

  const prevTitle = node.getAttribute('title');
  node.style.cursor = 'pointer';
  if (!node.hasAttribute('title')) node.setAttribute('title', 'Click to copy log');
  node.setAttribute('role', node.getAttribute('role') ?? 'button');
  node.setAttribute('tabindex', node.getAttribute('tabindex') ?? '0');
  node.addEventListener('click', doCopy);
  node.addEventListener('keydown', onKey);

  return {
    update(next) {
      current = next;
    },
    destroy() {
      node.removeEventListener('click', doCopy);
      node.removeEventListener('keydown', onKey);
      if (prevTitle == null) node.removeAttribute('title');
    }
  };
}
