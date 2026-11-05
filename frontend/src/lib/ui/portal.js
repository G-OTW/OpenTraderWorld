/**
 * Svelte action: renders an element inside `target` instead of where it is declared. The
 * component keeps owning the element (state, events, teardown); only its place in the DOM
 * moves. A null target leaves it where it is.
 */
export function portal(node, target) {
  const move = (to) => {
    if (to && node.parentNode !== to) to.appendChild(node);
  };
  move(target);
  return {
    update: move,
    destroy() {
      node.remove();
    }
  };
}
