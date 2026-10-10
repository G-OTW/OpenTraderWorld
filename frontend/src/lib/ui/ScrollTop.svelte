<script>
  // Back-to-top pill. Modules own their own scroll container (the shell's module context is
  // overflow:hidden), so instead of watching `window` we listen to scroll in the capture
  // phase and remember whichever element actually scrolled.
  import Icon from '$lib/ui/Icon.svelte';
  import { page } from '$app/stores';
  import { t } from '$lib/i18n';

  let { stacked = false } = $props();

  let target = null;
  let visible = $state(false);

  function isPageScroller(node) {
    if (node === document.scrollingElement) return true;
    const context = document.querySelector('.module-context');
    if (!context?.contains(node) || node === context) return false;
    if (node.closest('.otw-widget, [role="dialog"], .mobile-drawer')) return false;
    if (!['auto', 'scroll'].includes(getComputedStyle(node).overflowY)) return false;
    for (let parent = node.parentElement; parent && parent !== context; parent = parent.parentElement) {
      if (
        ['auto', 'scroll'].includes(getComputedStyle(parent).overflowY) &&
        parent.scrollHeight > parent.clientHeight + 2
      ) return false;
    }
    return true;
  }

  function onScroll(e) {
    const node = e.target === document ? document.scrollingElement : e.target;
    if (!node || node.nodeType !== 1) return;
    if (window.innerWidth > 767) {
      if (node.scrollHeight - node.clientHeight < 40) return;
      target = node;
      visible = node.scrollTop > node.clientHeight / 2;
      return;
    }
    if (!isPageScroller(node)) return;
    target = node;
    const threshold = Math.max(160, node.clientHeight / 2);
    visible = node.scrollHeight - node.clientHeight > threshold && node.scrollTop > threshold;
  }

  function toTop() {
    target?.scrollTo({ top: 0, behavior: 'smooth' });
    visible = false;
  }

  $effect(() => {
    document.addEventListener('scroll', onScroll, true);
    return () => document.removeEventListener('scroll', onScroll, true);
  });

  // A new page means a new scroll container: forget the old one.
  $effect(() => {
    $page.url.pathname;
    target = null;
    visible = false;
  });
</script>

{#if visible}
  <button
    class="scroll-top"
    class:stacked
    title={$t('shell.backToTop')}
    aria-label={$t('shell.backToTop')}
    onclick={toTop}
  >
    <Icon name="arrow-up" size={18} />
  </button>
{/if}

<style>
  .scroll-top {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
    /* Under the assistant, so an open panel simply covers it. */
    z-index: calc(var(--z-modal) - 11);
    animation: scroll-top-in 140ms ease-out;
  }
  /* Sits on top of the floating assistant when that one is on the page. */
  .scroll-top.stacked {
    bottom: calc(var(--space-4) + 44px + var(--space-2));
  }
  .scroll-top:hover {
    color: var(--accent);
    background: var(--surface-2);
  }
  @media (max-width: 767px) {
    /* The mobile topbar reserves the slot between its title and actions. */
    .scroll-top,
    .scroll-top.stacked {
      top: calc((var(--topbar-height) - 44px) / 2);
      right: 64px;
      bottom: auto;
      border-color: transparent;
      border-radius: var(--radius-sm);
      background: transparent;
      box-shadow: none;
    }
  }
  @keyframes scroll-top-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scroll-top {
      animation: none;
    }
  }
</style>
