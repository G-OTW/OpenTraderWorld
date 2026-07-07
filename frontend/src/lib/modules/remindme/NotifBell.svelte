<script>
  // Topbar bell with an unread pastille. Links to the notifications page. Mounted once in
  // the global layout, next to the core-status indicator.
  import { goto } from '$app/navigation';
  import { notifStore } from './store.svelte.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
</script>

<button class="bell" onclick={() => goto('/remindme/notifications')} title={$t('remindme.notifications')} aria-label={$t('remindme.notifications')}>
  <span class="icon"><Icon name="bell" size={17} /></span>
  {#if notifStore.unread > 0}
    <span class="pastille">{notifStore.unread > 99 ? '99+' : notifStore.unread}</span>
  {/if}
</button>

<style>
  .bell {
    position: relative;
    background: transparent;
    border: none;
    cursor: pointer;
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    color: var(--muted);
    line-height: 1;
  }
  .bell:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .icon {
    display: inline-flex;
  }
  .pastille {
    position: absolute;
    top: -2px;
    right: -2px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--red);
    color: #fff;
    font-size: 0.62rem;
    font-weight: 700;
    line-height: 16px;
    text-align: center;
  }
</style>
