<script>
  // Above the list: every portfolio summed in one currency, the same symbol held in several
  // books, currency exposure, the largest lines, and the theses due for review. Folded by
  // default once seen; the fold is remembered for the session only.
  //
  // One calm block (the Kubera overview shape): the net worth as the headline with the
  // total result beside it, three supporting figures, then three short tables on one row
  // so no column leaves a hole under a shorter neighbour.
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import Segmented from '$lib/ui/charts/Segmented.svelte';
  import { fmtDate, fmtPct } from '$lib/format.js';
  import { portfoliosApi, fmtMoney, unrealizedPct } from './api.js';

  let { reloadKey = 0, onopen = null } = $props();
  let data = $state(null);
  let reviews = $state([]);
  let open = $state(true);
  let error = $state(null);

  $effect(() => {
    void reloadKey;
    portfoliosApi
      .overview()
      .then((r) => (data = r))
      .catch((e) => (error = e.message));
    portfoliosApi
      .reviewsDue(7)
      .then((r) => (reviews = r))
      .catch(() => (reviews = []));
  });

  const pl = $derived(data ? data.unrealized + data.realized : null);
  const plPct = $derived(data ? unrealizedPct(pl, data.cost_basis) : null);
  const money = (v) => fmtMoney(v, data?.currency ?? 'USD');
  const share = (v) => (v == null ? '' : fmtPct(v, 1));
</script>

{#if reviews.length}
  <div class="reviews">
    <Icon name="calendar" size={13} />
    <span class="eyebrow">{$t('portfolios.reviews.due')}</span>
    {#each reviews as r (r.asset_id)}
      <button class="chip" onclick={() => onopen?.(r.portfolio_id)} title={r.thesis}>
        <b>{r.symbol}</b> <span class="dim">{r.portfolio} · {fmtDate(r.review_at)}</span>
      </button>
    {/each}
  </div>
{/if}

{#if data && data.portfolios.length > 1}
  <section class="ov">
    <button class="head" onclick={() => (open = !open)} aria-expanded={open}>
      <Icon name={open ? 'chevron-down' : 'chevron-right'} size={13} />
      <span class="eyebrow">{$t('portfolios.overview.title')}</span>
      {#if !open}
        <span class="folded">{money(data.net_worth)}</span>
        <Delta value={pl} format={money} sensitive empty={$t('portfolios.na')} />
      {/if}
    </button>
    {#if open}
      <div class="headline">
        <span class="worth">{money(data.net_worth)}</span>
        <Delta value={pl} format={money} size="md" sensitive empty={$t('portfolios.na')} />
        <Delta value={plPct} size="md" arrow={false} empty="" />
        <span class="dim">{$t('portfolios.detail.totalPl')}</span>
      </div>
      <dl class="figs">
        <div><dt>{$t('portfolios.detail.value')}</dt><dd>{money(data.market_value)}</dd></div>
        <div><dt>{$t('portfolios.detail.costBasis')}</dt><dd>{money(data.cost_basis)}</dd></div>
        <div><dt>{$t('portfolios.overview.cash')}</dt><dd>{money(data.cash_total)}</dd></div>
      </dl>
      <div class="cols">
        <div class="col wide">
          <h4>{$t('portfolios.overview.overlap')}</h4>
          {#if data.overlaps.length === 0}
            <p class="dim">{$t('portfolios.overview.noOverlap')}</p>
          {:else}
            <table class="rows">
              <tbody>
                {#each data.overlaps as o (o.symbol)}
                  <tr>
                    <td class="sym" title={o.name}>{o.symbol}</td>
                    <td class="books" title={o.portfolios.join(', ')}>{o.portfolios.join(', ')}</td>
                    <td class="num">{money(o.value)}</td>
                    <td class="num share">{share(o.weight_pct)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
        <div class="col">
          <h4>{$t('portfolios.overview.top')}</h4>
          <table class="rows">
            <tbody>
              {#each data.top as o (o.symbol)}
                <tr>
                  <td class="sym" title={o.name}>{o.symbol}</td>
                  <td class="num">{money(o.value)}</td>
                  <td class="num share">{share(o.weight_pct)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <div class="col">
          <h4>{$t('portfolios.overview.currencies')}</h4>
          <Segmented
            items={data.currencies.map((c) => ({ key: c.key, label: c.key, value: c.value }))}
            otherLabel={$t('portfolios.alloc.other')}
            format={money}
            ariaLabel={$t('portfolios.overview.currencies')}
          />
        </div>
      </div>
      <p class="hint">{$t('portfolios.overview.hint', { ccy: data.currency })}</p>
    {/if}
  </section>
{/if}
{#if error}<p class="dim">{error}</p>{/if}

<style>
  .reviews {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
    color: var(--dim);
  }
  .reviews .chip {
    padding: 2px var(--space-2);
    font-size: 12px;
  }
  .reviews b {
    color: var(--text);
    font-weight: 600;
  }
  .ov {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: none;
    border: 0;
    padding: 0;
    color: var(--dim);
    font: inherit;
    cursor: pointer;
    text-align: left;
  }
  .head:hover {
    color: var(--text);
  }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .folded {
    margin-left: var(--space-2);
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .headline {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
  }
  .worth {
    font-size: 28px;
    font-weight: 600;
    line-height: var(--lh-tight);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .figs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-8);
    margin: 0;
  }
  .figs div {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  dt {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  dd {
    margin: 0;
    font-family: var(--mono);
    font-size: 15px;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 1fr) minmax(0, 1fr);
    gap: var(--space-6);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
    align-items: start;
  }
  .col {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  h4 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  table.rows {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: 12px;
  }
  .rows td {
    height: 32px;
    padding: 0 var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows td:first-child {
    padding-left: 0;
  }
  .rows td:last-child {
    padding-right: 0;
  }
  .rows tr:hover td {
    background: var(--surface-hover);
  }
  .sym {
    width: 72px;
    font-weight: 600;
    color: var(--text);
  }
  .books {
    color: var(--dim);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .share {
    width: 64px;
    color: var(--muted);
  }
  .dim {
    color: var(--dim);
    font-size: 12px;
  }
  .hint {
    margin: 0;
    color: var(--dim);
    font-size: 12px;
  }
  @media (max-width: 1099px) {
    .cols {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    }
    .col.wide {
      grid-column: 1 / -1;
    }
  }
  @media (max-width: 767px) {
    .cols {
      grid-template-columns: minmax(0, 1fr);
    }
    .books {
      display: none;
    }
  }
</style>
