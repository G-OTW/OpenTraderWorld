<script>
  // Where a tab's data came from: one line per dataset with the provider and the fetch
  // time, or the error naming the fix (usually a connector to add), or the providers left
  // alone to spare their quota. The refresh button refetches every dataset of the tab now,
  // asking those too.
  import Icon from '$lib/ui/Icon.svelte';
  import { deferredText } from './deferred.js';
  import { t } from '$lib/i18n';

  // props: notes ([{ dataset, provider, fetched_at, error, deferred }]), onrefresh, busy
  let { notes = [], onrefresh, busy = false } = $props();

  const when = (ms) => (ms ? new Date(ms).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' }) : '');
</script>

{#if notes.length}
  <div class="note">
    <ul>
      {#each notes as n (n.dataset)}
        <li class:err={n.error}>
          <span class="ds">{$t(`fundamentals.ds.${n.dataset}`)}</span>
          {#if n.provider}<span>{n.provider} · {when(n.fetched_at)}</span>{/if}
          {#if n.error}<span class="msg">{n.error}</span>{/if}
          {#if n.deferred?.length}<span class="wait">{deferredText($t, n.deferred)}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if onrefresh}
      <button class="link" onclick={onrefresh} disabled={busy} title={$t('fundamentals.refresh')}>
        <Icon name="refresh-cw" size={13} />
        {$t('fundamentals.refresh')}
      </button>
    {/if}
  </div>
{/if}

<style>
  .note {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    justify-content: space-between;
    font-size: 12px;
    color: var(--muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  li {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .ds {
    font-weight: 600;
  }
  .err .msg {
    color: var(--red);
  }
  .wait {
    color: var(--amber);
  }
  .link {
    display: inline-flex;
    gap: var(--space-1);
    align-items: center;
    white-space: nowrap;
  }
</style>
