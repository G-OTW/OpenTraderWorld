/** Svelte action: fire a callback when a sentinel element scrolls into view.
 *
 * Used to grow a client-side "visible slice" of a long list as the user scrolls
 * (infinite scroll), so we render a page at a time instead of the whole list.
 *
 * Usage:
 *   <div use:infiniteScroll={{ onLoadMore: () => (visible += PAGE), disabled: allShown }}></div>
 *
 * Put the sentinel right after the last rendered row. `onLoadMore` runs whenever the
 * sentinel becomes visible; guard it yourself (e.g. stop growing past the list length).
 * Pass `disabled: true` to stop observing once everything is shown. `root` scopes the
 * observer to a scroll container (defaults to the viewport). */
export function infiniteScroll(node, params = {}) {
  let { onLoadMore, disabled = false, root = null, rootMargin = '200px' } = params;

  const observer = new IntersectionObserver(
    (entries) => {
      if (disabled) return;
      if (entries.some((e) => e.isIntersecting)) onLoadMore?.();
    },
    { root, rootMargin }
  );
  observer.observe(node);

  return {
    update(next = {}) {
      onLoadMore = next.onLoadMore ?? onLoadMore;
      disabled = next.disabled ?? false;
      // If the sentinel is already on screen after a state change (e.g. the container
      // grew but the sentinel stays in view), nudge another load so short pages fill.
      if (!disabled) {
        const r = node.getBoundingClientRect();
        const vh = window.innerHeight || document.documentElement.clientHeight;
        if (r.top < vh + 200) onLoadMore?.();
      }
    },
    destroy() {
      observer.disconnect();
    }
  };
}
