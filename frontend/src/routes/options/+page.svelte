<script>
  // Options: the chain of an underlying with its derived columns, a multi-leg strategy
  // builder with a payoff chart and saved strategies, and the volatility panels. Chains are
  // read from the connectors granted to Options (Interactive Brokers, Massive, Alpaca). The tab comes
  // from `?tab=`, else the last one used.
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import ChainTab from '$lib/modules/options/ChainTab.svelte';
  import BuilderTab from '$lib/modules/options/BuilderTab.svelte';
  import VolatilityTab from '$lib/modules/options/VolatilityTab.svelte';
  import { opt } from '$lib/modules/options/store.svelte.js';
  import { t } from '$lib/i18n';
  import { pref } from '$lib/prefs.svelte.js';

  const TABS = ['chain', 'builder', 'volatility'];
  const TAB_KEY = 'options.tab';

  // The last tab used, a user preference; `?tab=` opens that tab and makes it the last one.
  const tabPref = pref(TAB_KEY, 'chain');
  const query = typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('tab');
  if (TABS.includes(query)) tabPref.value = query;
  const tab = $derived(TABS.includes(tabPref.value) ? tabPref.value : 'chain');
  const setTab = (v) => (tabPref.value = v);

  const tabs = $derived(TABS.map((id) => ({ id, label: $t(`options.tab.${id}`) })));
  const subtitle = $derived(
    opt.chain ? $t('options.subtitle', { u: opt.chain.underlying, n: opt.chain.rows?.length ?? 0 }) : $t('options.subtitleEmpty')
  );
  // Remounting the chain tab reloads the connector list after a grant changes.
  let gen = $state(0);
</script>

<RequireModule module="options">
  <div class="page">
    <PageHeader title={$t('options.title')} {subtitle}>
      {#snippet actions()}
        <ConnectorButton module="options" onchanged={() => gen++} />
      {/snippet}
      <div class="tabsrow">
        <Tabs {tabs} bind:value={() => tab, setTab} ariaLabel={$t('options.title')} />
      </div>
    </PageHeader>

    {#key gen}
      {#if tab === 'chain'}
        <ChainTab ongo={setTab} />
      {:else if tab === 'builder'}
        <BuilderTab />
      {:else}
        <VolatilityTab />
      {/if}
    {/key}
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
    /* Room under the last block, so it scrolls clear of the floating shortcut buttons. */
    .page {
      min-width: 0;
      overflow-x: clip;
      padding: var(--space-4) var(--space-4) calc(72px + env(safe-area-inset-bottom));
    }
  }
</style>
