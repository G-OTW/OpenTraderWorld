<script>
  // Settings → Brokers: the same account broker the modules open in a modal. One
  // component, so an account is created, credentialed and granted identically wherever
  // the user reaches it from.
  import BrokerAdmin from '$lib/brokers/BrokerAdmin.svelte';
  import { t } from '$lib/i18n';
</script>

<div class="section">
  <div class="head">
    <h2>{$t('brokers.title')}</h2>
  </div>
  <p class="muted small">{$t('brokers.subtitle')}</p>

  <BrokerAdmin />
</div>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
  }
</style>
