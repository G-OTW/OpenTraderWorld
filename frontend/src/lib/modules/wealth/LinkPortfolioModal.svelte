<script>
  // Link a Portfolio Tracker portfolio into MyWealth as one asset, valued live.
  //
  // This replaced a one-shot import that copied each position's market value into a wealth
  // revision. That copy was correct for one morning: the tracker kept moving and the balance
  // sheet did not. A link holds a pointer instead, so the net worth reads the portfolio's own
  // number every time it is drawn, and the portfolio stays the one place that owns it.
  //
  // One asset per portfolio, not one per position. The positions already have a home; what
  // the balance sheet needs is a line saying what the whole account is worth.
  //
  // The second tab links a broker account the same way: its equity, as the broker states it,
  // read with the account's own credentials. An account not granted to MyWealth says so and
  // names the fix rather than failing on the first read.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { portfoliosApi } from '$lib/modules/portfolios/api.js';
  import { brokersApi, allows } from '$lib/brokers/api.js';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { wealthApi, CURRENCIES } from './api.js';
  import { money } from './view.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    /** Existing wealth assets, to flag portfolios that are already linked. */
    existing = [],
    /** Currency a new broker asset is kept in by default. */
    defaultCurrency = 'USD',
    onimported = () => {}
  } = $props();

  let tab = $state('portfolio'); // portfolio | broker
  let accounts = $state([]);
  let accountId = $state('');
  let brokerCurrency = $state('USD');

  let portfolios = $state([]);
  let picked = $state(new Set());
  let category = $state('');
  let loading = $state(false);
  let saving = $state(false);
  let error = $state('');

  $effect(() => {
    if (open) init();
  });

  async function init() {
    picked = new Set();
    category = '';
    error = '';
    accountId = '';
    brokerCurrency = defaultCurrency;
    loading = true;
    const [pf, acc] = await Promise.allSettled([portfoliosApi.list(), brokersApi.list()]);
    portfolios = pf.status === 'fulfilled' ? pf.value : [];
    accounts = acc.status === 'fulfilled' ? acc.value : [];
    if (pf.status === 'rejected') error = pf.reason?.message ?? '';
    loading = false;
  }

  const linkedAccounts = $derived(new Set(existing.map((a) => a.broker_account_id).filter(Boolean)));
  const account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  const granted = $derived(account ? allows(account, 'wealth') : false);

  async function linkBroker() {
    if (!account) return;
    saving = true;
    error = '';
    try {
      await wealthApi.addAsset({
        template_id: null,
        name: account.name,
        asset_type: 'broker_account',
        currency: brokerCurrency,
        category: category.trim() || null,
        sign: 1,
        portfolio_id: null,
        broker_account_id: account.id,
        review_days: 0
      });
      open = false;
      onimported();
    } catch (e) {
      error = e.message;
    }
    saving = false;
  }

  const linkedIds = $derived(new Set(existing.map((a) => a.portfolio_id).filter(Boolean)));

  function toggle(id) {
    if (linkedIds.has(id)) return;
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    picked = next;
  }

  async function link() {
    saving = true;
    error = '';
    try {
      for (const pf of portfolios.filter((p) => picked.has(p.id))) {
        await wealthApi.addAsset({
          template_id: null,
          name: pf.name,
          asset_type: 'portfolio',
          currency: pf.currency,
          category: category.trim() || null,
          sign: 1,
          portfolio_id: pf.id,
          // A linked value cannot go stale: it is read, not remembered.
          review_days: 0
        });
      }
      open = false;
      onimported();
    } catch (e) {
      error = e.message;
    }
    saving = false;
  }
</script>

<Modal bind:open size="md" title={$t('wealth.link.title')}>
  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={tab === 'portfolio'} onclick={() => (tab = 'portfolio')}>{$t('wealth.link.tabPortfolio')}</button>
    <button role="tab" aria-selected={tab === 'broker'} onclick={() => (tab = 'broker')}>{$t('wealth.broker.tab')}</button>
  </div>

  {#if tab === 'broker'}
    <p class="intro">{$t('wealth.broker.intro')}</p>
    {#if loading}
      <p class="muted">{$t('common.loading')}</p>
    {:else if accounts.length === 0}
      <p class="muted">{$t('wealth.broker.none')}</p>
    {:else}
      <div class="grid">
        <div class="field">
          <span>{$t('wealth.broker.account')}</span>
          <Dropdown
            value={accountId}
            onpick={(v) => (accountId = v)}
            ariaLabel={$t('wealth.broker.account')}
            options={[
              { value: '', label: $t('wealth.broker.pick') },
              ...accounts.map((a) => ({ value: a.id, label: `${a.name} (${a.capability?.label ?? a.broker})`, disabled: linkedAccounts.has(a.id) }))
            ]}
          />
        </div>
        <div class="field">
          <span>{$t('wealth.assetForm.currency')}</span>
          <Dropdown bind:value={brokerCurrency} ariaLabel={$t('wealth.assetForm.currency')} options={CURRENCIES.map((c) => ({ value: c, label: c }))} />
        </div>
        <label class="field span2">
          <span>{$t('wealth.link.category')}</span>
          <input type="text" bind:value={category} placeholder={$t('wealth.link.categoryHint')} />
        </label>
      </div>
      {#if account && linkedAccounts.has(account.id)}
        <p class="muted">{$t('wealth.link.already')}</p>
      {:else if account && !granted}
        <!-- The fix, named: the grant lives with the account, not here. -->
        <p class="warn">{$t('wealth.broker.notGranted', { name: account.name })} <a href="/settings#brokers">{$t('wealth.broker.openSettings')}</a></p>
      {/if}
    {/if}
  {:else}
    <p class="intro">{$t('wealth.link.intro')}</p>

    {#if loading}
      <p class="muted">{$t('common.loading')}</p>
    {:else if portfolios.length === 0}
      <p class="muted">{$t('wealth.link.noPortfolios')}</p>
    {:else}
      <ul class="list">
        {#each portfolios as pf (pf.id)}
          {@const already = linkedIds.has(pf.id)}
          <li class:already>
            <label>
              <input
                type="checkbox"
                checked={picked.has(pf.id)}
                disabled={already || saving}
                onchange={() => toggle(pf.id)}
              />
              <span class="name">{pf.name}</span>
              {#if already}
                <span class="badge"><Icon name="link" size={11} /> {$t('wealth.link.already')}</span>
              {/if}
              <span class="val">{money(pf.net_worth ?? pf.market_value, pf.currency, $t('wealth.page.na'))}</span>
            </label>
          </li>
        {/each}
      </ul>

      <label class="field cat">
        <span>{$t('wealth.link.category')}</span>
        <input type="text" bind:value={category} placeholder={$t('wealth.link.categoryHint')} />
      </label>
    {/if}
  {/if}

  <ErrorText {error} />

  {#snippet footer()}
    <button class="btn ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    {#if tab === 'broker'}
      <button class="btn primary" onclick={linkBroker} disabled={!account || !granted || linkedAccounts.has(account?.id) || saving}>
        {saving ? $t('common.saving') : $t('wealth.broker.action')}
      </button>
    {:else}
      <button class="btn primary" onclick={link} disabled={picked.size === 0 || saving}>
        {saving ? $t('common.saving') : $t('wealth.link.action', { n: picked.size })}
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .tabs {
    margin-bottom: var(--space-3);
  }
  .intro,
  .muted {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: 12px;
    line-height: var(--lh-base);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .span2 {
    grid-column: span 2;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .list li {
    border-top: var(--hairline) solid var(--border);
  }
  .list label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 36px;
    cursor: pointer;
    font-size: 13px;
  }
  .list li.already label {
    cursor: default;
    color: var(--muted);
  }
  .name {
    flex: 1;
    min-width: 0;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .val {
    color: var(--muted);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .cat {
    margin-top: var(--space-4);
  }
  .warn {
    margin: var(--space-3) 0 0;
    font-size: 12px;
    color: var(--amber-ink);
  }
  /* Phone: a row takes a finger, a long name or value wraps. */
  @media (max-width: 767px) {
    .grid { grid-template-columns: minmax(0, 1fr); }
    .span2 { grid-column: auto; }
    .list label { flex-wrap: wrap; min-height: 44px; }
    .name { white-space: normal; overflow-wrap: anywhere; }
  }
</style>
