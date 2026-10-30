<script>
  // Scanner: a universe of items (an instrument on a data connector at a timeframe), grouped
  // into hives, scanned by alerts that AND reusable signals on each closed bar. The module
  // only notifies: a hit links to the chart, where chart alerts and trades live.
  //
  // The page owns the shared lists (items, hives, signals, alerts) and hands them to the tab
  // that shows them; a tab that changes something asks the page to reload. `?tab=` and
  // `?alert=` come from a notification's link.
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import BoardTab from '$lib/modules/scanner/BoardTab.svelte';
  import ItemsTab from '$lib/modules/scanner/ItemsTab.svelte';
  import HivesTab from '$lib/modules/scanner/HivesTab.svelte';
  import SignalsTab from '$lib/modules/scanner/SignalsTab.svelte';
  import AlertsTab from '$lib/modules/scanner/AlertsTab.svelte';
  import HitsTab from '$lib/modules/scanner/HitsTab.svelte';
  import { scannerApi } from '$lib/modules/scanner/api.js';
  import { t } from '$lib/i18n';

  const TABS = ['board', 'items', 'hives', 'signals', 'alerts', 'hits'];
  const params = typeof location === 'undefined' ? new URLSearchParams() : new URLSearchParams(location.search);

  let tab = $state(TABS.includes(params.get('tab')) ? params.get('tab') : 'board');
  let focusAlert = $state(params.get('alert'));
  let items = $state([]);
  let hives = $state([]);
  let cap = $state(200);
  let signals = $state([]);
  let alerts = $state([]);
  let loading = $state(true);
  let error = $state('');

  const tabs = $derived(TABS.map((id) => ({ id, label: $t(`scanner.tab.${id}`) })));

  async function reload() {
    try {
      const [i, h, s, a] = await Promise.all([
        scannerApi.items(),
        scannerApi.hives(),
        scannerApi.signals(),
        scannerApi.alerts()
      ]);
      items = i;
      hives = h.hives;
      cap = h.cap;
      signals = s;
      alerts = a;
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    reload();
  });

  const subtitle = $derived(
    loading
      ? ''
      : $t('scanner.subtitle', { items: items.length, hives: hives.length, alerts: alerts.filter((a) => a.enabled).length })
  );
</script>

<RequireModule module="scanner">
  <div class="page">
    <PageHeader title={$t('scanner.title')} {subtitle}>
      {#snippet actions()}
        <ConnectorButton module="scanner" onchanged={reload} />
        <ChannelButton module="scanner" />
      {/snippet}
      <div class="tabsrow">
        <Tabs {tabs} bind:value={tab} ariaLabel={$t('scanner.title')} />
      </div>
    </PageHeader>

    {#if error}<ErrorText {error} />{/if}

    {#if tab === 'board'}
      <BoardTab {items} {hives} {signals} ongo={(t) => (tab = t)} />
    {:else if tab === 'items'}
      <ItemsTab {items} {hives} {cap} onchanged={reload} />
    {:else if tab === 'hives'}
      <HivesTab {items} {hives} {cap} onchanged={reload} />
    {:else if tab === 'signals'}
      <SignalsTab {signals} {items} {alerts} onchanged={reload} />
    {:else if tab === 'alerts'}
      <AlertsTab {alerts} {items} {hives} {signals} focus={focusAlert} onchanged={reload} ongo={(t) => (tab = t)} />
    {:else}
      <HitsTab {alerts} {hives} alertId={focusAlert} />
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
    margin-top: var(--space-3);
  }
  @media (max-width: 767px) {
    .page {
      padding: var(--space-4);
    }
  }
</style>
