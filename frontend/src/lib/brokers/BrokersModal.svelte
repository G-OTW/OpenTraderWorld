<script>
  // The account broker, in a modal. Every module that reads a broker account opens this
  // one instead of carrying its own settings tab.
  import Modal from '$lib/ui/Modal.svelte';
  import BrokerAdmin from './BrokerAdmin.svelte';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), module = null, onchanged } = $props();
</script>

<Modal bind:open title={$t('brokers.title')} size="lg">
  <p class="hint">{$t('brokers.modalHint')}</p>
  {#if open}<BrokerAdmin {module} {onchanged} />{/if}
  {#snippet footer()}
    <a class="full" href="/settings#brokers">{$t('brokers.openSettings')}</a>
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
