<script>
  // Settings → Notifications: the same broker admin the modules open in a modal. One
  // component, so a channel is created, credentialed and granted identically wherever
  // the user reaches it from.
  import ChannelAdmin from '$lib/notifications/ChannelAdmin.svelte';
  import { t } from '$lib/i18n';
</script>

<div class="section">
  <h2>{$t('notifch.title')}</h2>
  <p class="muted small">{$t('notifch.subtitle')}</p>

  <ChannelAdmin />
</div>

<style>
  .section {
    display: flex;
    flex-direction: column;
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
