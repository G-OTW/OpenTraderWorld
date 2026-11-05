<script>
  // One contract in detail: the raw quote, our model figures beside what the provider
  // published (never merged), the payoff of that single leg, and Buy / Sell into the Builder.
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import PayoffChart from './PayoffChart.svelte';
  import { optionsApi } from './api.js';
  import { legPrice, cashDividends, dividendYield, optionStyle } from './store.svelte.js';
  import { price, greek, pct, count, num, BLANK } from './columns.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), row = null, spot = null, rate = 0, onadd = () => {} } = $props();

  let payoff = $state(null);
  let error = $state('');

  $effect(() => {
    if (!open || !row || spot == null) return;
    const r = row;
    const premium = legPrice(r, 1);
    error = '';
    payoff = null;
    optionsApi
      .evaluate({
        spot,
        rate,
        dividend: dividendYield(),
        dividends: cashDividends(),
        style: optionStyle(),
        legs: [{ kind: r.call ? 'call' : 'put', strike: r.strike, expiry: r.expiry, qty: 1, price: premium ?? 0, iv: r.iv_mid ?? r.provider_iv ?? null }],
        at: 'expiry',
        iv_shift: 0,
        range: { lo: spot * 0.7, hi: spot * 1.3, steps: 121 }
      })
      .then((ev) => {
        if (row === r) payoff = ev;
      })
      .catch((e) => (error = e.message));
  });

  // The root of the OCC symbol: the head names the underlying, not the whole contract.
  const root = $derived(row?.ticker?.replace(/\d{6}[CP]\d{8}$/, '') ?? '');

  const quote = $derived(
    row
      ? [
          ['bid', price(row.bid), 'sell'],
          ['mid', price(row.mid), ''],
          ['ask', price(row.ask), 'buy'],
          ['last', price(row.last), ''],
          ['iv_mid', pct(row.iv_mid), '']
        ]
      : []
  );

  const market = $derived(
    row
      ? [
          ['spread_pct', pct(row.spread_pct)],
          ['volume', count(row.volume)],
          ['open_interest', count(row.open_interest)],
          ['iv_bid', pct(row.iv_bid)],
          ['iv_ask', pct(row.iv_ask)],
          ['rel_distance', pct(row.rel_distance)]
        ]
      : []
  );

  const value = $derived(
    row
      ? [
          ['theo', price(row.theo)],
          ['intrinsic', price(row.intrinsic)],
          ['time_value', price(row.time_value)],
          ['breakeven', price(row.breakeven)],
          ['to_be_pct', pct(row.to_be_pct)],
          ['dte', num(row.dte, 0)]
        ]
      : []
  );

  // Provider greeks are shown in the units the provider publishes them in.
  const compare = $derived(
    row
      ? [
          ['iv_mid', pct(row.iv_mid), pct(row.provider_iv)],
          ['delta', greek(row.delta), greek(row.provider_delta)],
          ['gamma', greek(row.gamma), greek(row.provider_gamma)],
          ['vega', greek(row.vega), greek(row.provider_vega)],
          ['theta', greek(row.theta), greek(row.provider_theta)],
          ['rho', greek(row.rho), BLANK]
        ]
      : []
  );

  function add(qty) {
    onadd(row, qty);
    open = false;
  }
</script>

<Modal bind:open title={row?.ticker ?? $t('options.drawer.title')} size="lg">
  {#snippet head()}
    {#if row}
      <div class="id">
        <div class="name">
          <span class="root">{root}</span>
          <span class="strike">{price(row.strike)}</span>
          <span class="side" class:put={!row.call}>{$t(row.call ? 'options.drawer.tagCall' : 'options.drawer.tagPut')}</span>
        </div>
        <div class="sub">
          <span>{row.expiry}</span>
          <span>{$t('options.drawer.left', { n: num(row.dte, 0) })}</span>
          <span class="occ">{row.ticker}</span>
        </div>
      </div>
    {/if}
  {/snippet}
  {#if row}
    <div class="quote">
      {#each quote as [k, v, tone] (k)}
        <div class="cell">
          <span class="label">{$t(`options.col.${k}`)}</span>
          <span class="big {tone}">{v}</span>
        </div>
      {/each}
    </div>
    <div class="cols">
      <section>
        <h4>{$t('options.drawer.market')}</h4>
        <dl>
          {#each market as [k, v] (k)}
            <div><dt>{$t(`options.col.${k}`)}</dt><dd>{v}</dd></div>
          {/each}
        </dl>
      </section>
      <section>
        <h4>{$t('options.drawer.value')}</h4>
        <dl>
          {#each value as [k, v] (k)}
            <div><dt>{$t(`options.col.${k}`)}</dt><dd>{v}</dd></div>
          {/each}
        </dl>
      </section>
      <section>
        <div class="gh">
          <h4>{$t('options.drawer.greeks')}</h4>
          <h4 class="n">{$t('options.drawer.ours')}</h4>
          <h4 class="n">{$t('options.drawer.provider')}</h4>
        </div>
        <dl class="three">
          {#each compare as [k, ours, theirs] (k)}
            <div><dt>{$t(`options.col.${k}`)}</dt><dd>{ours}</dd><dd class="them">{theirs}</dd></div>
          {/each}
        </dl>
      </section>
    </div>
    <section class="pay">
      <h4>{$t('options.drawer.payoff')}</h4>
      <ErrorText {error} />
      {#if payoff}
        <PayoffChart curves={[{ name: row.ticker, payoff: payoff.payoff, breakevens: payoff.breakevens }]} {spot} height={190} showDate={false} compact />
      {:else if spot == null}
        <p class="qp-hint">{$t('options.drawer.noSpot')}</p>
      {/if}
    </section>
    <p class="note">{$t('options.drawer.unitsHint', { style: $t(`options.drawer.${optionStyle()}`) })}</p>
  {/if}
  {#snippet footer()}
    <button class="ghost close" onclick={() => (open = false)}>{$t('options.close')}</button>
    <button class="ghost danger" onclick={() => add(-1)} disabled={!row}>{$t('options.drawer.sell', { p: price(row?.bid) })}</button>
    <button class="primary" onclick={() => add(1)} disabled={!row}>{$t('options.drawer.buy', { p: price(row?.ask) })}</button>
  {/snippet}
</Modal>

<style>
  .id {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .name {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .root,
  .strike {
    font-size: var(--fs-page-title);
    font-weight: var(--fw-medium);
    line-height: 1.15;
    color: var(--text);
  }
  .strike {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .side {
    align-self: center;
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--green);
    background: color-mix(in srgb, var(--green) 14%, transparent);
  }
  .side.put {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 14%, transparent);
  }
  .sub {
    display: flex;
    flex-wrap: wrap;
    gap: 0 var(--space-3);
    font-size: var(--fs-desc);
    color: var(--muted);
  }
  .occ {
    font-family: var(--mono);
  }

  .quote {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .cell {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    padding: var(--space-3) var(--space-4);
  }
  .cell + .cell {
    border-left: var(--hairline) solid var(--border);
  }
  .label,
  h4 {
    margin: 0;
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .big {
    font-family: var(--mono);
    font-size: calc(var(--fs-metric-value) + 3px);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .big.sell {
    color: var(--red);
  }
  .big.buy {
    color: var(--green);
  }

  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) minmax(0, 1.25fr);
    margin-top: var(--space-4);
  }
  .cols section {
    min-width: 0;
    padding: 0 var(--space-4);
  }
  .cols section:first-child {
    padding-left: 0;
  }
  .cols section:last-child {
    padding-right: 0;
  }
  .cols section + section {
    border-left: var(--hairline) solid var(--border);
  }
  .cols h4 {
    padding-bottom: var(--space-2);
  }
  .gh,
  dl.three > div {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 72px 72px;
    gap: var(--space-2);
  }
  .n {
    text-align: right;
  }
  dl {
    margin: 0;
  }
  dl > div {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 5px 0;
    border-top: var(--hairline) solid var(--border);
  }
  dt {
    color: var(--muted);
    white-space: nowrap;
  }
  dd {
    margin: 0;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
    color: var(--text);
    white-space: nowrap;
  }
  dd.them {
    color: var(--muted);
  }

  .pay {
    margin-top: var(--space-4);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .note {
    margin: var(--space-2) 0 0;
    font-size: var(--fs-desc);
    color: var(--dim, var(--muted));
  }
  .close {
    margin-right: auto;
  }

  @media (max-width: 767px) {
    .quote {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
    .cell {
      padding: var(--space-2) var(--space-3);
    }
    .cell + .cell {
      border-left: none;
    }
    .cols {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-4);
    }
    .cols section,
    .cols section:first-child,
    .cols section:last-child {
      padding: 0;
    }
    .cols section + section {
      border-left: none;
    }
  }
</style>
