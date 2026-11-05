<script>
  // Data broker — the dedicated page behind the modal every data module opens. Same
  // component, more room: create a connector, plug its key from the vault, and choose
  // which modules may use it.
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ConnectorAdmin from '$lib/connectors/ConnectorAdmin.svelte';
  import { t } from '$lib/i18n';
</script>

<div class="page">
  <PageHeader title={$t('connectors.title')} subtitle={$t('connectors.subtitle')} />
  <ConnectorAdmin />
</div>

<style>
  .page {
    height: 100%;
    padding: var(--space-6);
    overflow-y: auto;
  }
</style>
