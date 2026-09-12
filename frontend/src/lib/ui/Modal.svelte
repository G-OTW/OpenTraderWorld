<script module>
  // Modals stack in the order they open, not in DOM order: a confirm raised from inside
  // another modal has to paint over it wherever it sits in the markup.
  let openLayers = 0;
</script>

<script>
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  // Reusable modal dialog. Closes on backdrop click and Escape.
  // props: open (bindable), title, size ('sm'|'md'|'lg'|'xl'), onclose, children, footer,
  // dismissible (false: the backdrop no longer closes, so a multi-step picker is left only
  // through its own Cancel; Escape still cancels).
  let {
    open = $bindable(false),
    title = '',
    size = 'sm',
    dismissible = true,
    onclose = () => {},
    children,
    footer
  } = $props();

  function close() {
    open = false;
    onclose();
  }

  let layer = $state(0);
  $effect(() => {
    if (!open) return;
    layer = ++openLayers;
    return () => (openLayers = Math.max(0, openLayers - 1));
  });

  // The backdrop closes only on a gesture that both starts and ends on the backdrop itself.
  // A click bubbling from a child, or a text selection dragged out of an input and released
  // over the backdrop, is not a request to close.
  let pressedBackdrop = false;

  function onBackdropDown(e) {
    pressedBackdrop = dismissible && e.target === e.currentTarget;
  }

  function onBackdropClick(e) {
    if (!pressedBackdrop || e.target !== e.currentTarget) return;
    pressedBackdrop = false;
    close();
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
  }

  // The dialog itself is the scroller, so the head and the foot are sticky rather than
  // pushed out of view by a long body: a picker with fifty rows must still show its title
  // and its action buttons. The separating filet is only drawn when the body really
  // overflows, so a modal that fits keeps exactly the layout it had.
  let box = $state(null);
  let overflows = $state(false);

  // The class itself adds padding to the foot, so it grows the box it is measured from: a
  // modal whose body lands within that padding of the limit would set it, no longer fit,
  // clear it, fit again, forever. The observer would re-fire on each flip and lock the
  // page. Hence the hysteresis, and no write when nothing changed.
  $effect(() => {
    if (!box) return;
    let on = false;
    const measure = () => {
      const slack = box.scrollHeight - box.clientHeight;
      const next = on ? slack > -24 : slack > 1;
      if (next === on) return;
      on = next;
      overflows = next;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(box);
    for (const child of box.children) ro.observe(child);
    measure();
    return () => ro.disconnect();
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    style="z-index: calc(var(--z-modal) + {layer})"
    onpointerdown={onBackdropDown}
    onclick={onBackdropClick}
    role="presentation"
  >
    <div
      bind:this={box}
      class="modal {size}"
      class:overflows
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      {#if title}
        <header class="modal-head">
          <h3>{title}</h3>
          <button class="x" onclick={close} aria-label={$t('common.close')}><Icon name="x" size={15} /></button>
        </header>
      {/if}
      <div class="modal-body">
        {@render children?.()}
      </div>
      {#if footer}
        <footer class="modal-foot">
          {@render footer()}
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal); /* raised per open modal, see `layer` */
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(2px);
    padding: var(--space-4);
  }
  /* A modal is one restrained level above its workspace surface. */
  .modal {
    width: 100%;
    max-width: 440px;
    max-height: calc(100vh - 2 * var(--space-8));
    overflow-y: auto;
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
    outline: none;
    animation: pop var(--dur-fast) var(--ease);
  }
  .modal.md {
    max-width: 640px;
  }
  .modal.lg {
    max-width: 860px;
  }
  /* For wide data tables: takes the viewport, minus the backdrop padding. */
  .modal.xl {
    max-width: 1400px;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .modal-head {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4);
    background: var(--surface-raised);
    border-bottom: var(--hairline) solid var(--border);
  }
  .modal-head h3 {
    color: var(--text);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    margin: 0;
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
  }
  .x:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .modal-body {
    padding: var(--space-4);
  }
  .modal-foot {
    position: sticky;
    bottom: 0;
    z-index: 1;
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding: 0 var(--space-4) var(--space-4);
    background: var(--surface-raised);
  }
  .modal.overflows .modal-foot {
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
</style>
