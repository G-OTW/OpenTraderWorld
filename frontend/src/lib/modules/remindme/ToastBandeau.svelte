<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Slide-in notification toasts, stacked at the top-right. Each shows the reminder name
  // and links to its target (goal/todo/reminders). Mounted once in the global layout.
  import { goto } from '$app/navigation';
  import { notifStore } from './store.svelte.js';
  import { linkFor, kindLabel } from './api.js';
  import { t } from '$lib/i18n';

  function open(n) {
    goto(linkFor(n));
    notifStore.dismiss(n.id);
  }
</script>

<div class="bandeau" aria-live="polite">
  {#each notifStore.toasts as n (n.id)}
    <div class="toast">
      <span class="bell"><Icon name="bell" size={16} /></span>
      <button class="content" onclick={() => open(n)}>
        <span class="name">{n.name}</span>
        <span class="meta">{$t('remindme.toast.meta', { kind: kindLabel(n.kind) })} <Icon name="external-link" size={11} /></span>
      </button>
      <button class="close" onclick={() => notifStore.dismiss(n.id)} aria-label={$t('remindme.dismiss')}><Icon name="x" size={13} /></button>
    </div>
  {/each}
</div>

<style>
  /* A fired reminder must show over anything, including an open dialog. The raw 300 tied
     --z-modal, so a modal rendered later in DOM order painted over the toast. */
  .bandeau {
    position: fixed;
    top: var(--space-4);
    right: var(--space-4);
    z-index: var(--z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 320px;
    max-width: calc(100vw - 2 * var(--space-4));
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-left: 1.5px solid var(--accent);
    border-radius: 0;
    box-shadow: none;
    padding: var(--space-3);
    animation: slide-in 0.28s cubic-bezier(0.2, 0.9, 0.3, 1.2);
  }
  @keyframes slide-in {
    from {
      transform: translateX(calc(100% + var(--space-4)));
      opacity: 0;
    }
  }
  .bell {
    font-size: var(--text-md);
    flex: none;
  }
  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    color: var(--text);
  }
  .name {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .close {
    flex: none;
    background: transparent;
    border: none;
    color: var(--dim);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 2px 4px;
    border-radius: 0;
  }
  .close:hover {
    color: var(--text);
    background: var(--surface-2);
  }
</style>
