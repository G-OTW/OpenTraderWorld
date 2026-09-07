<script>
  // Portfolio widget: a summary thumbnail for a chosen portfolio (like the list card on the
  // portfolios landing page) — value, cost basis, unrealized P&L, asset count. Config:
  // { portfolio_id } — falls back to the first portfolio when unset.
  import { portfoliosApi, fmtMoney, fmtPct, gainPct } from '$lib/modules/portfolios/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

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
  <ErrorText error={err} compact />
{:else if list === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
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
  .sk {
    padding: var(--space-1) 0;
  }
  /* Preview, loading and empty text — not an error. This was grouped with a
     now-removed .err rule and inherited its red. */
  .hint {
    color: var(--dim);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .name {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .value {
    font-size: 1.6rem;
    font-weight: var(--fw-normal);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .pnl {
    font-size: var(--text-base);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .pnl.pos {
    color: var(--green);
  }
  .pnl.neg {
    color: var(--red);
  }
  .meta {
    font-size: var(--text-xs);
    color: var(--dim);
    margin-top: var(--space-1, 4px);
  }
</style>
