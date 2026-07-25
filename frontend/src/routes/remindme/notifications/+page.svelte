<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Notifications page — the full history with unread pastilles, an "Acknowledge all"
  // action, per-item read/open/delete, and a link back to reminders.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { remindApi, linkFor, kindLabel, fmtTime } from '$lib/modules/remindme/api.js';
  import { notifStore } from '$lib/modules/remindme/store.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let notifications = $state([]);
  let unread = $state(0);
  let loading = $state(true);

  onMount(load);

  async function load() {
    const r = await remindApi.notifications(200);
    notifications = r.notifications;
    unread = r.unread;
    loading = false;
  }

  async function ackAll() {
    await notifStore.ackAll();
    await load();
  }

  async function openNotif(n) {
    if (!n.read_at) {
      await remindApi.markRead(n.id);
      notifStore.noteRead();
    }
    goto(linkFor(n));
  }

  async function markRead(n) {
    if (n.read_at) return;
    await remindApi.markRead(n.id);
    notifStore.noteRead();
    await load();
  }

  async function del(n) {
    await remindApi.removeNotif(n.id);
    await load();
  }
</script>

<div class="notifs">
  <header class="head">
    <div class="title">
      <button class="back" onclick={() => goto('/remindme')} aria-label={$t('remindme.notifs.back')}>←</button>
      <h1>{$t('remindme.notifs.title')}</h1>
      {#if unread > 0}<span class="pastille">{$t('remindme.notifs.unread', { count: unread })}</span>{/if}
    </div>
    <button class="ack" onclick={ackAll} disabled={unread === 0}>{$t('remindme.notifs.ackAll')}</button>
  </header>

  {#if loading}
    <ul class="list" aria-busy="true">
      {#each Array.from({ length: 5 }, (_, i) => i) as i (i)}
        <li class="sk-item">
          <Skeleton circle size="8px" />
          <span class="sk-body">
            <Skeleton height="1rem" width="55%" />
            <Skeleton height="0.75rem" width="35%" />
          </span>
        </li>
      {/each}
    </ul>
  {:else if notifications.length === 0}
    <!-- An empty inbox is "you're caught up", not a void — and there is nothing to create,
         notifications arrive on their own. So: no action button. -->
    <EmptyState icon="check-circle" title={$t('remindme.notifs.empty')} />
  {:else}
    <ul class="list">
      {#each notifications as n (n.id)}
        <li class="item" class:unread={!n.read_at}>
          <span class="dot" aria-hidden="true"></span>
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="body" onclick={() => openNotif(n)}>
            <div class="row1">
              <span class="name">{n.name}</span>
              <span class="time">{fmtTime(n.created_at)}</span>
            </div>
            <div class="meta">
              {kindLabel(n.kind)}{#if n.linked_id} · {$t('remindme.notifs.open')} <Icon name="external-link" size={11} />{/if}
            </div>
            {#if n.details}<p class="details">{n.details}</p>{/if}
          </div>
          <div class="actions">
            {#if !n.read_at}
              <button class="icon" title={$t('remindme.notifs.markRead')} onclick={() => markRead(n)}><Icon name="check" size={14} /></button>
            {/if}
            <button class="icon danger-hover" title={$t('remindme.notifs.delete')} onclick={() => del(n)}><Icon name="trash" size={14} /></button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .notifs {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
    max-width: 760px;
    margin: 0 auto;
    width: 100%;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .back {
    background: transparent;
    border: 0.5px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    width: 30px;
    height: 30px;
    cursor: pointer;
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
  }
  .pastille {
    background: var(--red);
    color: var(--red-contrast);
    border-radius: 0;
    padding: 1px 8px;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .ack {
    background: transparent;
    border: 0.5px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 7px 13px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .ack:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .muted {
    color: var(--dim);
  }
  .sk-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) 0;
  }
  .sk-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1;
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
  }
  .item .dot {
    flex: none;
    width: 8px;
    height: 8px;
    margin-top: 6px;
    border-radius: 0;
    background: transparent;
  }
  .item.unread .dot {
    background: var(--accent);
  }
  .item.unread {
    border-left: 1.5px solid var(--accent);
  }
  .body {
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }
  .row1 {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .name {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .time {
    flex: none;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .meta {
    font-size: var(--text-xs);
    color: var(--muted);
    margin-top: 2px;
  }
  .details {
    margin-top: 4px;
    font-size: var(--text-sm);
    color: var(--muted);
    white-space: pre-wrap;
  }
  .actions {
    display: flex;
    gap: 2px;
  }
</style>
