<script>
  // Mailbox widget: the latest unread mail, newest first. A click opens the module —
  // the dashboard says what arrived, the module is where you read it.
  import { mailboxApi } from '$lib/modules/mailbox/api.js';
  import { relativeTime } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();

  // `config.category` — 'all' or one of the sender categories.
  const category = $derived(item?.config?.category ?? 'all');

  let messages = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    void category;
    load();
  });

  async function load() {
    err = '';
    try {
      messages = await mailboxApi.listMessages({ category, unread: true, limit: 8 });
    } catch (e) {
      err = e.message;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.mailbox.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if messages === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else if !messages.length}
  <p class="hint">{$t('dashboard.widgets.mailbox.empty')}</p>
{:else}
  <ul>
    {#each messages as m (m.id)}
      <li>
        <a href="/mailbox">
          <span class="from">{m.sender_name}</span>
          <span class="subject">{m.subject || $t('mailbox.noSubject')}</span>
          <span class="when">{relativeTime(m.received_at)}</span>
        </a>
      </li>
    {/each}
  </ul>
  <a class="more" href="/mailbox">
    {$t('dashboard.widgets.mailbox.open')}
    <Icon name="chevron-right" size={12} />
  </a>
{/if}

<style>
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .sk {
    padding: var(--space-1) 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li + li {
    border-top: 1px solid var(--border);
  }
  li a {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 0 var(--space-2);
    padding: var(--space-2) 0;
    text-decoration: none;
    color: var(--text);
  }
  li a:hover .subject {
    color: var(--accent);
  }
  .from {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when {
    font-size: 11px;
    color: var(--muted);
    grid-row: 1;
    grid-column: 2;
  }
  .subject {
    grid-column: 1 / -1;
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .more {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    text-decoration: none;
  }
  .more:hover {
    color: var(--text);
  }
</style>
