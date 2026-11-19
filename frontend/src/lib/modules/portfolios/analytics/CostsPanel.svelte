<script>
  // `costs`: income collected, cost paid, and what the cost was worth.
  //
  // Net-after-fees is a counterfactual, not a subtraction: fees already sit inside cost
  // basis, proceeds and cash, so the curve is net by construction. The gap shown here is
  // between the real curve and the one the book would have had without the fee legs.
  import { t } from '$lib/i18n';
  import { fmtPct as fmtPlainPct, fmtFixed } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtMoney, fmtPct, na } from '../api.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import Panel from './Panel.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const money = (v) => fmtMoney(v, currency);
  // The four tiles must speak of one period. Income and cost are the window's own whenever
  // the curve covers it; the lifetime figure moves to the hint rather than to the headline,
  // where it silently contradicted a drag measured over the window.
  const windowed = $derived(d?.window_fees != null || d?.window_income != null);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
    <div class="pa-stats">
      <StatCard
        label={$t('portfolios.analytics.costs.income')}
        value={money(windowed ? d.window_income : d.income.total)}
        hint={windowed ? $t('portfolios.analytics.costs.lifetime', { v: money(d.income.total) }) : ''}
      />
      <StatCard
        label={$t('portfolios.analytics.costs.total')}
        value={money(windowed ? d.window_fees : d.costs.total)}
        hint={windowed ? $t('portfolios.analytics.costs.lifetime', { v: money(d.costs.total) }) : ''}
      />
      <StatCard
        label={$t('portfolios.analytics.costs.drag')}
        value={d.drag_bps == null ? na() : `${fmtFixed(d.drag_bps, 0)} bp`}
        hint={$t('portfolios.analytics.costs.dragHint')}
      />
      <StatCard
        label={$t('portfolios.analytics.costs.feeCost')}
        value={d.fee_cost_pct == null ? na() : fmtPlainPct(d.fee_cost_pct)}
        hint={$t('portfolios.analytics.costs.feeCostHint')}
      />
    </div>

    {#if d.gross_return_pct != null && d.net_return_pct != null}
      <section class="compare">
        <div class="fig">
          <span class="eyebrow">{$t('portfolios.analytics.costs.gross')}</span>
          <strong>{fmtPct(d.gross_return_pct)}</strong>
        </div>
        <Icon name="arrow-right" size={16} />
        <div class="fig">
          <span class="eyebrow">{$t('portfolios.analytics.costs.net')}</span>
          <strong>{fmtPct(d.net_return_pct)}</strong>
        </div>
        <p class="pa-note">{$t('portfolios.analytics.costs.explain')}</p>
      </section>
    {/if}

    <div class="pa-cols">
      <Panel title={$t('portfolios.analytics.costs.incomeTitleAll')} flush>
        <table class="pa-table">
          <tbody>
            <tr><td>{$t('portfolios.kind.dividend')}</td><td class="r">{money(d.income.dividends)}</td></tr>
            <tr><td>{$t('portfolios.kind.interest')}</td><td class="r">{money(d.income.interest)}</td></tr>
            <tr><td>{$t('portfolios.kind.coupon')}</td><td class="r">{money(d.income.coupons)}</td></tr>
          </tbody>
        </table>
      </Panel>
      <Panel title={$t('portfolios.analytics.costs.costTitleAll')} flush>
        <table class="pa-table">
          <tbody>
            <tr><td>{$t('portfolios.analytics.costs.transaction')}</td><td class="r">{money(d.costs.transaction_fees)}</td></tr>
            <tr><td>{$t('portfolios.analytics.costs.standalone')}</td><td class="r">{money(d.costs.standalone_fees)}</td></tr>
            <tr><td>{$t('portfolios.kind.tax')}</td><td class="r">{money(d.costs.taxes)}</td></tr>
          </tbody>
        </table>
      </Panel>
    </div>

    <Sample sample={block.sample} />
  </div>
{/if}

<style>
  .compare {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-3) var(--space-4);
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    color: var(--dim);
  }
  .fig {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .fig strong {
    font-family: var(--mono);
    font-size: 15px;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .compare p {
    flex: 1 1 280px;
  }
</style>
