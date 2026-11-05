<script>
  // Mailbox module. Reads newsletters, news mail and broker mail from IMAP mailboxes
  // you connect, groups them by sender and category, and keeps a curated store of the
  // newsletters you follow. Read-only against the mail server; credentials live in the
  // vault; remote images never load unless asked.
  import { onMount } from 'svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import MailPane from '$lib/modules/mailbox/MailPane.svelte';
  import StorePane from '$lib/modules/mailbox/StorePane.svelte';
  import MailboxesPane from '$lib/modules/mailbox/MailboxesPane.svelte';
  import { mailboxApi } from '$lib/modules/mailbox/api.js';
  import { t } from '$lib/i18n';

  const TAB_KEY = 'otw.mailbox.tab';

  let tab = $state('mail');
  let accounts = $state([]);
  let presets = $state([]);
  let counts = $state({ unread: 0, total: 0, pending_senders: 0 });
  let error = $state('');
  let fetching = $state(false);
  let refreshKey = $state(0);

  onMount(async () => {
    try {
      const saved = localStorage.getItem(TAB_KEY);
      if (saved) tab = saved;
    } catch {
      /* private mode — the default tab is fine */
    }
    await reload();
    // First run: nothing is connected, so the useful screen is the one that fixes that.
    if (!accounts.length && !localStorage.getItem(TAB_KEY)) tab = 'mailboxes';
  });

  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      /* non-fatal */
    }
  });

  async function reload() {
    try {
      const r = await mailboxApi.overview();
      accounts = r.accounts ?? [];
      presets = r.presets ?? [];
      counts = r.counts ?? counts;
      error = '';
    } catch (e) {
      error = e.message;
    }
  }

  async function fetchAll() {
    fetching = true;
    try {
      await mailboxApi.pollAll();
      await reload();
      refreshKey++;
    } catch (e) {
      error = e.message;
    } finally {
      fetching = false;
    }
  }

  const tabs = $derived([
    { id: 'mail', label: $t('mailbox.tab.mail') },
    { id: 'store', label: $t('mailbox.tab.store') },
    { id: 'mailboxes', label: $t('mailbox.tab.mailboxes') }
  ]);

  const subtitle = $derived(
    counts.total
      ? $t('mailbox.subtitle', { unread: counts.unread, total: counts.total })
      : $t('mailbox.subtitleEmpty')
  );
</script>

<RequireModule module="mailbox">
  <div class="page">
    <PageHeader title={$t('mailbox.title')} {subtitle}>
      {#snippet actions()}
        {#if accounts.length}
          <Button icon="refresh-cw" loading={fetching} onclick={fetchAll}>
            {$t('mailbox.fetchAll')}
          </Button>
        {/if}
      {/snippet}
      <div class="tabsrow">
        <Tabs {tabs} bind:value={tab} ariaLabel={$t('mailbox.title')} />
        {#if counts.pending_senders > 0 && tab !== 'mail'}
          <button class="nudge" onclick={() => (tab = 'mail')}>
            {$t('mailbox.pendingNudge', { count: counts.pending_senders })}
          </button>
        {/if}
      </div>
    </PageHeader>

    {#if error}<ErrorText {error} />{/if}

    {#if tab === 'mail'}
      <MailPane {accounts} {refreshKey} oncounts={reload} />
    {:else if tab === 'store'}
      <StorePane />
    {:else}
      <MailboxesPane {accounts} {presets} onreload={reload} />
    {/if}
  </div>
</RequireModule>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  .tabsrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-top: var(--space-3);
  }
  .nudge {
    background: none;
    border: 1px solid var(--amber);
    color: var(--amber);
    border-radius: 999px;
    padding: 2px var(--space-3);
    font-size: var(--text-xs);
    cursor: pointer;
  }
</style>
