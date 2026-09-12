<script>
  // `costs` — income collected, cost paid, and what the cost was worth.
  //
  // Net-after-fees is a counterfactual, not a subtraction: fees already sit inside cost
  // basis, proceeds and cash, so the curve is net by construction. The gap shown here is
  // between the real curve and the one the book would have had without the fee legs.
  import { t } from '$lib/i18n';
  import { fmtMoney, fmtPct, fmtSignedPct, fmtFixed, EM_DASH } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const money = (v) => (v == null ? EM_DASH : fmtMoney(v, currency));
  // The four tiles must speak of one period. Income and cost are the window's own whenever
  // the curve covers it; the lifetime figure moves to the hint rather than to the headline,
  // where it silently contradicted a drag measured over the window.
  const windowed = $derived(d?.window_fees != null || d?.window_income != null);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="stats">
    <StatCard
      label={$t('portfolios.analytics.costs.income')}
      value={money(windowed ? d.window_income : d.income.total)}
      hint={windowed
        ? $t('portfolios.analytics.costs.lifetime', { v: fmtMoney(d.income.total, currency) })
        : ''}
    />
    <StatCard
      label={$t('portfolios.analytics.costs.total')}
      value={money(windowed ? d.window_fees : d.costs.total)}
      hint={windowed
        ? $t('portfolios.analytics.costs.lifetime', { v: fmtMoney(d.costs.total, currency) })
        : ''}
    />
    <StatCard
      label={$t('portfolios.analytics.costs.drag')}
      value={d.drag_bps == null ? EM_DASH : `${fmtFixed(d.drag_bps, 0)} bp`}
      hint={$t('portfolios.analytics.costs.dragHint')}
    />
    <StatCard
      label={$t('portfolios.analytics.costs.feeCost')}
      value={d.fee_cost_pct == null ? EM_DASH : fmtPct(d.fee_cost_pct)}
      hint={$t('portfolios.analytics.costs.feeCostHint')}
    />
  </div>

  {#if d.gross_return_pct != null && d.net_return_pct != null}
    <div class="compare">
      <div>
        <span class="lbl">{$t('portfolios.analytics.costs.gross')}</span>
        <strong class="num">{fmtSignedPct(d.gross_return_pct)}</strong>
      </div>
      <span class="arrow">→</span>
      <div>
        <span class="lbl">{$t('portfolios.analytics.costs.net')}</span>
        <strong class="num">{fmtSignedPct(d.net_return_pct)}</strong>
      </div>
      <p>{$t('portfolios.analytics.costs.explain')}</p>
    </div>
  {/if}

  <div class="cols">
    <table class="tbl">
      <thead>
        <tr><th colspan="2">{$t('portfolios.analytics.costs.incomeTitleAll')}</th></tr>
      </thead>
      <tbody>
        <tr><td>{$t('portfolios.kind.dividend')}</td><td class="r num">{money(d.income.dividends)}</td></tr>
        <tr><td>{$t('portfolios.kind.interest')}</td><td class="r num">{money(d.income.interest)}</td></tr>
        <tr><td>{$t('portfolios.kind.coupon')}</td><td class="r num">{money(d.income.coupons)}</td></tr>
      </tbody>
    </table>
    <table class="tbl">
      <thead>
        <tr><th colspan="2">{$t('portfolios.analytics.costs.costTitleAll')}</th></tr>
      </thead>
      <tbody>
        <tr><td>{$t('portfolios.analytics.costs.transaction')}</td><td class="r num">{money(d.costs.transaction_fees)}</td></tr>
        <tr><td>{$t('portfolios.analytics.costs.standalone')}</td><td class="r num">{money(d.costs.standalone_fees)}</td></tr>
        <tr><td>{$t('portfolios.kind.tax')}</td><td class="r num">{money(d.costs.taxes)}</td></tr>
      </tbody>
    </table>
  </div>

  <Sample sample={block.sample} />
{/if}

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .compare {
    display: grid;
    grid-template-columns: auto auto auto;
    gap: var(--space-1) var(--space-4);
    justify-content: start;
    align-items: baseline;
    margin-top: var(--space-4);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .compare .lbl {
    display: block;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .compare strong {
    font-size: 22px;
  }
  .arrow {
    color: var(--muted);
    font-size: 20px;
  }
  .compare p {
    grid-column: 1 / -1;
    margin: var(--space-2) 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--space-3);
    margin-top: var(--space-4);
  }
  .r {
    text-align: right;
  }
</style>
