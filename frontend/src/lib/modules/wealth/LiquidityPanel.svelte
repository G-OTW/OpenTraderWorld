<script>
  // Liquidity and runway: what can be spent within a month against what a month costs. The
  // cost is the monthly expenses set in Goals plus the Subscriptions forecast's monthly
  // average; a category counts as liquid only when flagged here.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { wealthApi } from './api.js';
  import { money } from './view.js';
  import { runway, liquidTotal } from './engine.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    currency = 'USD',
    monthlyExpenses = null,
    /** (amount, from) => amount in `currency`, or null without a rate. */
    toDisplay = (v) => v,
    reloadKey = 0
  } = $props();

  let cats = $state([]);
  let byCategory = $state({});
  let subs = $state(null); // { monthly, currency } | null
  let subsError = $state('');
  let error = $state('');

  $effect(() => {
    void reloadKey;
    error = '';
    Promise.all([wealthApi.categories(), wealthApi.snapshot()])
      .then(([c, snap]) => {
        cats = c;
        byCategory = snap.by_category ?? {};
      })
      .catch((e) => (error = e.message));
    subsError = '';
    wealthApi
      .subscriptionsForecast()
      .then((f) => (subs = { monthly: f.monthly_avg ?? 0, currency: f.currency }))
      .catch(() => {
        subs = null;
        subsError = $t('wealth.liquidity.noSubs');
      });
  });

  const liquidNames = $derived(cats.filter((c) => c.liquid).map((c) => c.name));
  const liquid = $derived(liquidTotal(byCategory, liquidNames));
  const total = $derived(Object.values(byCategory).reduce((s, v) => s + v, 0));
  const subsMonthly = $derived(subs ? toDisplay(subs.monthly, subs.currency) : null);
  const subsUnconverted = $derived(subs != null && subsMonthly == null);
  const r = $derived(runway(liquid, monthlyExpenses, subsMonthly));

  async function toggle(c) {
    try {
      cats = await wealthApi.setLiquid(c.name, !c.liquid);
    } catch (e) {
      error = e.message;
    }
  }
  const label = (c) => c || $t('wealth.page.uncategorized');
</script>

<div class="panel">
  <dl class="figures">
    <div class="fig">
      <dt>{$t('wealth.liquidity.liquid')}</dt>
      <dd class:word={liquid == null}>{liquid == null ? $t('wealth.liquidity.noneFlagged') : money(liquid, currency)}</dd>
    </div>
    <div class="fig">
      <dt>{$t('wealth.liquidity.illiquid')}</dt>
      <dd class:word={liquid == null}>{liquid == null ? $t('wealth.liquidity.noneFlagged') : money(total - liquid, currency)}</dd>
    </div>
    <div class="fig">
      <dt>{$t('wealth.liquidity.monthlyCost')}</dt>
      <dd class:word={!r.cost}>{r.cost ? money(r.cost, currency) : $t('wealth.liquidity.noCost')}</dd>
    </div>
    <div class="fig">
      <dt>{$t('wealth.liquidity.runway')}</dt>
      <dd class:word={r.months == null}>
        {#if r.months != null}{$t('wealth.liquidity.months', { n: fmtNum(r.months, 1) })}
        {:else if r.reason === 'no_cost'}{$t('wealth.liquidity.setExpenses')}
        {:else}{$t('wealth.liquidity.flagOne')}{/if}
      </dd>
    </div>
  </dl>
  <p class="note">
    {$t('wealth.liquidity.expenses', { amount: monthlyExpenses ? money(monthlyExpenses, currency) : $t('wealth.liquidity.notSet') })}
    ·
    {#if subsError}{subsError}
    {:else if subsUnconverted}{$t('wealth.liquidity.subsNoRate', { currency: subs.currency })}
    {:else if subs}{$t('wealth.liquidity.subs', { amount: money(subsMonthly, currency) })}{/if}
  </p>
  <ErrorText {error} />
  {#if cats.length}
    <div class="chips" role="group" aria-label={$t('wealth.liquidity.flagTitle')}>
      <span class="note">{$t('wealth.liquidity.flagTitle')}</span>
      {#each cats as c (c.name)}
        <button class="chip" class:active={c.liquid} aria-pressed={c.liquid} onclick={() => toggle(c)}>
          {#if c.liquid}<Icon name="check" size={12} />{/if}{label(c.name)}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* Four figures on one hairline grid: no box per figure inside the panel's card. */
  .figures {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: 0;
    border-top: var(--hairline) solid var(--border);
    border-bottom: var(--hairline) solid var(--border);
  }
  .fig {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3);
    border-left: var(--hairline) solid var(--border);
    min-width: 0;
  }
  .fig:first-child {
    border-left: none;
    padding-left: 0;
  }
  dt {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  dd {
    margin: 0;
    font-family: var(--mono);
    font-size: 15px;
    font-variant-numeric: tabular-nums;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  dd.word {
    font-family: var(--font);
    font-size: 12px;
    color: var(--muted);
  }
  .note {
    margin: 0;
    color: var(--dim);
    font-size: 12px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
  }
  @media (max-width: 767px) {
    .figures {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .fig:nth-child(3) {
      border-left: none;
      padding-left: 0;
    }
    .chip {
      min-height: 44px;
    }
  }
</style>
