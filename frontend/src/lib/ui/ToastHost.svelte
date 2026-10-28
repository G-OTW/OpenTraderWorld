<script>
  // Renders the app-wide toast queue in the bottom-right. Mount once at the root.
  import { toast } from '$lib/ui/toast.svelte.js';
  import { t } from '$lib/i18n';
  import { fly, fade } from 'svelte/transition';
</script>

<div class="toast-host" role="region" aria-live="polite" aria-label={$t('common.notifications')}>
  <!-- `item`, not `t`: `t` is the i18n translator imported above, and an {#each ... as t}
       would shadow it for every $t() inside the block. -->
  {#each toast.items as item (item.id)}
    <!-- An error interrupts the screen reader; a success waits its turn.
         The store emits 'err', never 'error' — this compared against the wrong string,
         so every toast, errors included, was announced politely and queued behind
         whatever was already being read. The .toast.err CSS rule had it right. -->
    <div
      class="toast {item.kind}"
      role={item.kind === 'err' ? 'alert' : 'status'}
      in:fly={{ y: 12, duration: 160 }}
      out:fade={{ duration: 120 }}
    >
      <span class="txt">{item.text}</span>
      <button class="x" aria-label={$t('common.dismiss')} onclick={() => toast.dismiss(item.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .toast-host {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    z-index: var(--z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: min(360px, calc(100vw - 2 * var(--space-4)));
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
  }
  .txt {
    flex: 1;
    min-width: 0;
  }
  .x {
    background: none;
    border: none;
    color: var(--muted);
    font-size: var(--text-md);
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
  }
  .x:hover {
    color: var(--text);
  }
</style>
