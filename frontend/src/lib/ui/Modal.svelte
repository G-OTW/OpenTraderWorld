<script>
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  // Reusable modal dialog. Closes on backdrop click and Escape.
  // props: open (bindable), title, size ('sm'|'md'|'lg'), onclose, children, footer
  let {
    open = $bindable(false),
    title = '',
    size = 'sm',
    onclose = () => {},
    children,
    footer
  } = $props();

  function close() {
    open = false;
    onclose();
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={close} role="presentation">
    <div
      class="modal {size}"
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
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(2px);
    padding: var(--space-4);
  }
  .modal {
    width: 100%;
    max-width: 440px;
    max-height: calc(100vh - 2 * var(--space-8));
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.55);
    outline: none;
    animation: pop 0.12s ease-out;
  }
  .modal.md {
    max-width: 640px;
  }
  .modal.lg {
    max-width: 860px;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.98);
    }
  }
  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-4) 0;
  }
  .modal-head h3 {
    color: var(--text);
    font-size: 1rem;
    font-weight: 600;
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
    border-radius: var(--radius);
  }
  .x:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .modal-body {
    padding: var(--space-4);
  }
  .modal-foot {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding: 0 var(--space-4) var(--space-4);
  }
</style>
