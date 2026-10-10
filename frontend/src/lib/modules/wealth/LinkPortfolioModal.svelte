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
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { portfoliosApi } from '$lib/modules/portfolios/api.js';
  import { wealthApi, fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    /** Existing wealth assets, to flag portfolios that are already linked. */
    existing = [],
    onimported = () => {}
  } = $props();

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
    loading = true;
    try {
      portfolios = await portfoliosApi.list();
    } catch (e) {
      error = e.message;
    }
    loading = false;
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
            <span class="val num">{fmtMoney(pf.net_worth ?? pf.market_value, pf.currency)}</span>
            {#if already}
              <span class="chip"><Icon name="link" size={11} /> {$t('wealth.link.already')}</span>
            {/if}
          </label>
        </li>
      {/each}
    </ul>

    <label class="cat">
      <span>{$t('wealth.link.category')}</span>
      <input type="text" bind:value={category} placeholder={$t('wealth.link.categoryHint')} />
    </label>
  {/if}

  <ErrorText {error} />

  {#snippet footer()}
    <button class="btn ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={link} disabled={picked.size === 0 || saving}>
      {saving ? $t('common.saving') : $t('wealth.link.action', { n: picked.size })}
    </button>
  {/snippet}
</Modal>

<style>
  .intro,
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  .list {
    list-style: none;
    margin: var(--space-3) 0 0;
    padding: 0;
  }
  .list li {
    border-top: 1px solid var(--border);
  }
  .list label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) 0;
    cursor: pointer;
  }
  .list li.already label {
    cursor: default;
    opacity: 0.6;
  }
  .name {
    flex: 1;
    font-weight: 600;
  }
  .val {
    color: var(--muted);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 7px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 11px;
    color: var(--muted);
  }
  .cat {
    display: block;
    margin-top: var(--space-4);
  }
  .cat > span {
    display: block;
    font-size: 12px;
    color: var(--muted);
    margin-bottom: var(--space-1);
  }
  .cat input {
    width: 100%;
  }
</style>
