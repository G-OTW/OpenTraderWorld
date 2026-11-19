<script>
  // The Income tab: forecast annual income per holding from the dividend data the user
  // entered, the next ex and pay dates, and the income actually booked over twelve months
  // beside the forecast. A holding without data is listed as "no estimate", never as zero.
  import { t } from '$lib/i18n';
  import StatCard from '$lib/ui/StatCard.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import { portfoliosApi, fmtMoney, fmtNum, fmtDate } from './api.js';

  let { portfolioId, currency = 'USD', reloadKey = 0, onasset = null } = $props();

  let data = $state(null);
  let error = $state(null);

  $effect(() => {
    void reloadKey;
    if (!portfolioId) return;
    portfoliosApi
      .income(portfolioId)
      .then((r) => (data = r))
      .catch((e) => (error = e.message));
  });

  const pct = (v) => (v == null ? '' : `${v.toFixed(2)}%`);
</script>

<ErrorText {error} />
{#if !data}
  <div class="stack">
    <div class="stats">
      {#each Array(3) as _, i (i)}<Skeleton height="64px" />{/each}
    </div>
    <Skeleton height="240px" />
  </div>
{:else}
  <div class="stack">
    <div class="stats">
      <StatCard label={$t('portfolios.income.forecast')} value={fmtMoney(data.forecast_annual, currency)} hint={$t('portfolios.income.forecastHint')} />
      <StatCard label={$t('portfolios.income.trailing')} value={fmtMoney(data.trailing_12m, currency)} hint={$t('portfolios.income.trailingHint')} />
      <StatCard
        label={$t('portfolios.income.yoc')}
        value={data.yield_on_cost_pct == null ? $t('portfolios.income.noEstimate') : pct(data.yield_on_cost_pct)}
        hint={$t('portfolios.income.yocHint')}
      />
    </div>

    {#if data.no_estimate.length}
      <p class="note">{$t('portfolios.income.missing', { list: data.no_estimate.join(', ') })}</p>
    {/if}

    <div class="cols">
      <section class="card">
        <h3>{$t('portfolios.income.upcoming')}</h3>
        {#if data.events.length === 0}
          <p class="note pad">{$t('portfolios.income.noUpcoming')}</p>
        {:else}
          <table>
            <tbody>
              {#each data.events as e, i (i)}
                <tr>
                  <td class="date">{fmtDate(e.date)}</td>
                  <td><Badge tone={e.kind === 'pay' ? 'success' : 'neutral'}>{$t(`portfolios.income.${e.kind}`)}</Badge></td>
                  <td class="sym">{e.symbol}</td>
                  <td class="r">{e.amount == null ? '' : fmtMoney(e.amount, currency)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>

      <section class="card wide">
        <h3>{$t('portfolios.income.perHolding')}</h3>
        <div class="scroll">
          <table>
            <thead>
              <tr>
                <th>{$t('portfolios.detail.colSymbol')}</th>
                <th class="r">{$t('portfolios.detail.colQty')}</th>
                <th>{$t('portfolios.asset.frequency')}</th>
                <th class="r">{$t('portfolios.income.perShare')}</th>
                <th class="r">{$t('portfolios.income.annual')}</th>
                <th class="r">{$t('portfolios.income.yoc')}</th>
                <th class="r">{$t('portfolios.income.trailing')}</th>
              </tr>
            </thead>
            <tbody>
              {#each data.holdings as h (h.asset_id)}
                <tr>
                  <td class="sym">
                    {#if onasset}<button class="symbtn" onclick={() => onasset(h.asset_id)}>{h.symbol}</button>{:else}{h.symbol}{/if}
                  </td>
                  <td class="r">{fmtNum(h.quantity)}</td>
                  <td>{$t(`portfolios.freq.${h.frequency}`)}</td>
                  <td class="r">{h.per_share == null ? '' : fmtMoney(h.per_share, currency)}</td>
                  <td class="r">{#if h.annual == null}<span class="dim">{$t('portfolios.income.noEstimate')}</span>{:else}{fmtMoney(h.annual, currency)}{/if}</td>
                  <td class="r">{pct(h.yield_on_cost_pct)}</td>
                  <td class="r">{fmtMoney(h.trailing_12m, currency)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  </div>
{/if}

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }
  .stats :global(.statcard) {
    background: var(--surface);
    border-radius: var(--radius);
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(280px, 1fr) minmax(0, 2fr);
    gap: var(--space-3);
    align-items: start;
  }
  .card {
    min-width: 0;
    padding: var(--space-4) 0 var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
  }
  h3 {
    margin: 0 var(--space-4) var(--space-3);
    font-size: 13px;
    font-weight: 600;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th,
  td {
    height: 36px;
    padding: 0 var(--space-3);
    text-align: left;
    white-space: nowrap;
    border-bottom: var(--hairline) solid var(--border);
  }
  th {
    height: 32px;
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  tbody tr:last-child td {
    border-bottom: 0;
  }
  tbody tr {
    transition: background-color 140ms ease;
  }
  tbody tr:hover {
    background: var(--surface-hover);
  }
  .r {
    text-align: right;
  }
  td.r,
  .date {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .date {
    color: var(--muted);
  }
  .sym {
    font-weight: 600;
  }
  .symbtn {
    background: none;
    border: 0;
    padding: 0;
    font: inherit;
    color: var(--text);
    cursor: pointer;
  }
  .symbtn:hover {
    color: var(--accent);
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--dim);
  }
  .pad {
    padding: 0 var(--space-4) var(--space-3);
  }
  .dim {
    color: var(--dim);
    font-family: var(--font);
  }
  @media (max-width: 1099px) {
    .cols {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
