<script>
  // "Back to top" affordance for long scrolling views. It finds its own scroll
  // container (the nearest scrollable ancestor, falling back to the window), so a
  // component can drop it in without knowing the page shell's layout.
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  // Show the button once the container is scrolled past this many pixels.
  let { threshold = 400 } = $props();

  // Zero-size marker: the DOM foothold used to walk up to the scroller. Reactive so the
  // effect re-runs once the binding lands.
  let anchor = $state(null);
  let scroller = null;
  let visible = $state(false);

  function scrollParent(node) {
    for (let el = node?.parentElement; el; el = el.parentElement) {
      const oy = getComputedStyle(el).overflowY;
      if (oy === 'auto' || oy === 'scroll') return el;
    }
    return null; // no scrollable ancestor — the window scrolls
  }

  $effect(() => {
    if (!anchor) return;
    scroller = scrollParent(anchor);
    const target = scroller ?? window;
    const onScroll = () => {
      visible = (scroller ? scroller.scrollTop : window.scrollY) > threshold;
    };
    target.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
    return () => target.removeEventListener('scroll', onScroll);
  });

  function toTop() {
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    (scroller ?? window).scrollTo({ top: 0, behavior: reduce ? 'auto' : 'smooth' });
  }
</script>

<span class="anchor" bind:this={anchor} aria-hidden="true"></span>
{#if visible}
  <button class="to-top" onclick={toTop} title={$t('common.backToTop')} aria-label={$t('common.backToTop')}>
    <Icon name="arrow-up" size={16} />
  </button>
{/if}

<style>
  .anchor {
    display: none;
  }
  /* Floats over the content, bottom-right. Below toasts (--z-toast) so a message
     never ends up hidden behind it. */
  .to-top {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-8);
    z-index: var(--z-sticky);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    padding: 0;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--muted);
    cursor: pointer;
    box-shadow: var(--shadow-1);
  }
  .to-top:hover {
    background: var(--surface-2);
    color: var(--text);
  }
</style>
