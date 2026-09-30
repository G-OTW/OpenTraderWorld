<script>
  // The notification broker, in a modal. Every module that notifies opens this one
  // instead of carrying its own channel screen, so a destination is configured once and
  // granted from wherever the user happens to be.
  import Modal from '$lib/ui/Modal.svelte';
  import ChannelAdmin from './ChannelAdmin.svelte';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), module = null, onchanged } = $props();
</script>

<Modal bind:open title={$t('notifch.title')} size="lg">
  <p class="hint">{$t('notifch.modalHint')}</p>
  <!-- Mounted only while open: the admin self-loads, and a stale list is worse than a
       one-request wait. -->
  {#if open}<ChannelAdmin {module} {onchanged} />{/if}
  {#snippet footer()}
    <a class="full" href="/settings#notifications">{$t('notifch.openSettings')}</a>
  {/snippet}
</Modal>

<style>
  .hint {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
    margin-bottom: var(--space-3);
  }
  .full {
    color: var(--accent);
    text-decoration: none;
    font-size: var(--text-sm);
  }
  .full:hover {
    text-decoration: underline;
  }
</style>
