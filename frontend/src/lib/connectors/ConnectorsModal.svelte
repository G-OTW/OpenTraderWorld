<script>
  // The data broker, in a modal. Every module that consumes market data opens this one
  // instead of carrying its own settings tab, so a connector is created once and granted
  // from the same screen wherever the user happens to be.
  import Modal from '$lib/ui/Modal.svelte';
  import ConnectorAdmin from './ConnectorAdmin.svelte';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), module = null, onchanged } = $props();
</script>

<Modal bind:open title={$t('connectors.title')} size="lg">
  <p class="hint">{$t('connectors.modalHint')}</p>
  <ConnectorAdmin {module} {onchanged} />
  {#snippet footer()}
    <a class="full" href="/connectors">{$t('connectors.openPage')}</a>
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
