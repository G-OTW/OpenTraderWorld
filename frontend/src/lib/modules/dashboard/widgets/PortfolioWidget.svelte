<script>
  // Portfolio widget: a summary thumbnail for a chosen portfolio (like the list card on the
  // portfolios landing page) — value, cost basis, unrealized P&L, asset count. Config:
  // { portfolio_id } — falls back to the first portfolio when unset.
  import { portfoliosApi, fmtMoney, fmtPct, gainPct } from '$lib/modules/portfolios/api.js';
  import { t } from '$lib/i18n';

  let { item, editing } = $props();
  const wanted = $derived(item.config?.portfolio_id || null);

  let list = $state(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      list = await portfoliosApi.list();
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const p = $derived(
    wanted ? (list ?? []).find((x) => x.id === wanted) ?? null : (list ?? [])[0] ?? null
  );
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.portfolio.preview')}</p>
{:else if err}
  <p class="err">{err}</p>
{:else if list === null}
  <p class="hint">{$t('common.loading')}</p>
{:else if !p}
  <p class="hint">{$t('dashboard.widgets.portfolio.empty')}</p>
{:else}
  {@const g = gainPct(p.market_value, p.cost_basis)}
  <div class="card">
    <div class="name">{p.name}</div>
    <div class="value">{fmtMoney(p.market_value, p.currency)}</div>
    <div class="pnl" class:pos={(p.unrealized ?? 0) >= 0} class:neg={(p.unrealized ?? 0) < 0}>
      {fmtMoney(p.unrealized, p.currency)}{#if g != null} · {fmtPct(g)}{/if}
    </div>
    <div class="meta">
      {#if (p.asset_count ?? 0) === 1}
        {$t('dashboard.widgets.portfolio.metaOne', { cost: fmtMoney(p.cost_basis, p.currency) })}
      {:else}
        {$t('dashboard.widgets.portfolio.metaMany', { count: p.asset_count ?? 0, cost: fmtMoney(p.cost_basis, p.currency) })}
      {/if}
    </div>
  </div>
{/if}

<style>
  .hint,
  .err {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .name {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .value {
    font-size: 1.6rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .pnl {
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }
  .pnl.pos {
    color: var(--green);
  }
  .pnl.neg {
    color: var(--red);
  }
  .meta {
    font-size: 0.75rem;
    color: var(--muted);
    margin-top: var(--space-1, 4px);
  }
</style>
