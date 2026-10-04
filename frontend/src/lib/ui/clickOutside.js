/**
 * Svelte action: run `handler` when a pointerdown lands outside the node.
 *
 *   <div use:clickOutside={() => (open = false)}>…</div>
 *
 * pointerdown (not click) so a dropdown closes before the click lands on
 * whatever is underneath, and capture so a stopPropagation() inside some other
 * widget cannot swallow it.
 */
export function clickOutside(node, handler) {
  let cb = handler;
  function onDown(e) {
    if (!node.contains(e.target)) cb?.(e);
  }
  document.addEventListener('pointerdown', onDown, true);
  return {
    update(next) {
      cb = next;
    },
    destroy() {
      document.removeEventListener('pointerdown', onDown, true);
    }
  };
}
