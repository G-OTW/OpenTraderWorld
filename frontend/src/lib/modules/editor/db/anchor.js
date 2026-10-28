// Anchor a floating panel to its trigger using fixed positioning.
//
// The table scrolls (`overflow-x: auto`), which clips absolutely positioned
// descendants no matter their z-index. Fixed positioning escapes that, but then
// the panel has to be placed by hand: this action measures the trigger and
// keeps the panel next to it, flipping above when it would run off the bottom.

const MARGIN = 8;

/**
 * @param {HTMLElement} node the floating panel
 * @param {{ trigger: HTMLElement, align?: 'left' | 'right' }} opts
 */
export function anchored(node, opts) {
  let current = opts;

  const place = () => {
    const trigger = current?.trigger;
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    const { innerWidth: vw, innerHeight: vh } = window;

    node.style.position = 'fixed';
    node.style.maxHeight = `${vh - 2 * MARGIN}px`;
    node.style.overflowY = 'auto';

    const w = node.offsetWidth;
    const h = node.offsetHeight;

    // Below the trigger, above it when there isn't room.
    const below = r.bottom;
    const top = below + h + MARGIN > vh && r.top - h - MARGIN > 0 ? r.top - h : below;
    node.style.top = `${Math.max(MARGIN, Math.min(top, vh - h - MARGIN))}px`;

    const wanted = current.align === 'right' ? r.right - w : r.left;
    node.style.left = `${Math.max(MARGIN, Math.min(wanted, vw - w - MARGIN))}px`;
  };

  place();
  // Re-place on anything that can move the trigger.
  window.addEventListener('scroll', place, true);
  window.addEventListener('resize', place);
  const ro = new ResizeObserver(place);
  ro.observe(node);

  return {
    update(next) {
      current = next;
      place();
    },
    destroy() {
      window.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
      ro.disconnect();
    }
  };
}
