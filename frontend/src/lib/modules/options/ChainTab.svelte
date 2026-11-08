<script>
  // Chain tab: pick a source and an underlying, choose expiries and a strike window, load the
  // snapshot, then filter and read it without another provider request. A contract opens its
  // details; Buy / Sell sends it to the Builder.
  import '$lib/modules/quant/panel.css';
  import MarketSource from '$lib/modules/quant/MarketSource.svelte';
  import TaskProgress from '$lib/modules/quant/TaskProgress.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import UnderlyingPicker from './UnderlyingPicker.svelte';
  import ChainFilters from './ChainFilters.svelte';
  import ChainTable from './ChainTable.svelte';
  import ColumnPicker from './ColumnPicker.svelte';
  import ContractDrawer from './ContractDrawer.svelte';
  import InfoNote from './InfoNote.svelte';
  import { optionsApi } from './api.js';
  import { opt, addContract, cashDividends, dividendYield } from './store.svelte.js';
  import { MAX_EXPIRIES, pickExpiries, fetchWindow, covers, sameRates, strikeKey, groupChain } from './chain.js';
  import { loadLayout, onLayout, price } from './columns.js';
  import { t } from '$lib/i18n';

  let { ongo = () => {} } = $props();

  let layout = $state(loadLayout());
  // The saved layout can arrive from the server after the first paint.
  $effect(() => onLayout((l) => (layout = l)));
  let pickerOpen = $state(false);
  let drawerOpen = $state(false);
  let drawerRow = $state(null);
  let busy = $state(false);
  let expBusy = $state(false);
  let progress = $state(null);
  let error = $state('');
  let ratePct = $state(+(opt.rate * 100).toFixed(4));
  let divPct = $state(+(opt.dividend * 100).toFixed(4));

  $effect(() => {
    opt.rate = Number(ratePct) / 100 || 0;
    opt.dividend = Number(divPct) / 100 || 0;
  });

  // A different connector lists its own symbols: the pick and what came from it are reset.
  let lastSource = opt.source;
  $effect(() => {
    const s = opt.source;
    if (lastSource && s !== lastSource) {
      opt.underlying = null;
      opt.expiries = null;
      opt.chain = null;
    }
    lastSource = s;
  });

  async function loadExpiries() {
    if (!opt.source || !opt.underlying) return;
    expBusy = true;
    error = '';
    opt.expiries = null;
    opt.chain = null;
    opt.dividends = null;
    // Nothing to read is an empty list with a notice, and a failed read leaves the yield field.
    optionsApi
      .dividends({ underlying: opt.underlying.symbol, asset_type: opt.assetType })
      .then((d) => (opt.dividends = d))
      .catch(() => (opt.dividends = null));
    try {
      const r = await optionsApi.expiries({ connector_id: opt.source, underlying: opt.underlying.symbol, asset_type: opt.assetType });
      opt.expiries = r;
      // An empty preset on a fresh underlying falls back to the two nearest expiries.
      if (!pickExpiries(r.expiries, opt.expiryFilter).length) {
        opt.expiryFilter.mode = 'specific';
        opt.expiryFilter.dates = r.expiries.slice(0, 2).map((e) => e.date);
      }
    } catch (e) {
      error = e.message;
    } finally {
      expBusy = false;
    }
  }

  const list = $derived(opt.expiries?.expiries ?? []);
  const selected = $derived(pickExpiries(list, opt.expiryFilter));
  const picked = $derived(selected.slice(0, MAX_EXPIRIES));
  const spot = $derived(opt.chain?.spot ?? opt.expiries?.spot ?? null);
  const win = $derived(fetchWindow(opt.strikeFilter, spot, list, picked));
  const sameUnderlying = $derived(opt.chain && opt.underlying && opt.chain.picked === opt.underlying.symbol && opt.chain.source === opt.source);
  const cash = $derived(cashDividends());
  const want = $derived({ rate: opt.rate, dividend: dividendYield(), cash: JSON.stringify(cash), strike: strikeKey(opt.strikeFilter) });
  // What the dividend schedule holds, for its line above the filters.
  const ahead = $derived(opt.dividends?.dividends ?? []);
  const estimated = $derived(ahead.filter((d) => d.projected).length);
  const fresh = $derived(sameUnderlying && covers(opt.chain, picked, win, want));
  const ratesStale = $derived(sameUnderlying && opt.chain.asked && !sameRates(opt.chain.asked, want));

  // Notices of the expiries answer and of the loaded chain, once per code. A spot found by the
  // chain retires the expiries' missing spot notice.
  const notices = $derived.by(() => {
    const chainNotices = sameUnderlying ? (opt.chain.notices ?? []) : [];
    const spotFound = sameUnderlying && opt.chain.spot != null;
    const seen = new Set();
    const out = [];
    for (const n of [...chainNotices, ...(opt.expiries?.notices ?? [])]) {
      if (seen.has(n.code) || (spotFound && n.code === 'missing_spot')) continue;
      seen.add(n.code);
      out.push(n);
    }
    return out;
  });

  async function load(force = false) {
    if (!opt.source || !opt.underlying || !picked.length) return;
    busy = true;
    error = '';
    progress = null;
    const asked = { expiries: [...picked], lo: win[0], hi: win[1], rate: opt.rate, dividend: want.dividend, cash: want.cash, strike: want.strike };
    try {
      const r = await optionsApi.chain(
        {
          connector_id: opt.source,
          underlying: opt.underlying.symbol,
          asset_type: opt.assetType,
          expiries: asked.expiries,
          strike_lo: asked.lo,
          strike_hi: asked.hi,
          rate: asked.rate,
          dividend: asked.dividend,
          dividends: cash,
          force
        },
        (p) => (progress = p)
      );
      opt.chain = { ...r, source: opt.source, picked: opt.underlying.symbol, underlying: r.underlying ?? opt.underlying.symbol, asked };
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  const groups = $derived(
    sameUnderlying ? groupChain(opt.chain.rows, opt.chain.spot, { expiries: picked, strike: opt.strikeFilter, spread: opt.spreadFilter }) : []
  );
  const columns = $derived(layout.order.filter((k) => layout.visible.includes(k)));
  const asOf = $derived(opt.chain?.as_of ? new Date(opt.chain.as_of).toLocaleString([], { dateStyle: 'medium', timeStyle: 'medium' }) : '');

  function openRow(r) {
    drawerRow = r;
    drawerOpen = true;
  }

  function add(r, qty) {
    addContract(r, qty);
    ongo('builder');
  }
</script>

<div class="qp">
  <div class="qp-params">
    <MarketSource module="options" providers={['ibkr', 'massive', 'alpaca']} bind:value={opt.source} bind:provider={opt.provider} />
    <UnderlyingPicker connector={opt.source} kinds={opt.provider === 'alpaca' ? ['equity'] : undefined} bind:assetType={opt.assetType} bind:value={opt.underlying} onpick={loadExpiries} />
    <label class="qp-field num"><span class="qp-label">{$t('options.rate')}</span><input type="number" step="0.1" bind:value={ratePct} /></label>
    <label class="qp-field num"><span class="qp-label">{$t('options.dividend')}</span><input type="number" step="0.1" bind:value={divPct} disabled={cash.length > 0} /></label>
  </div>

  {#if expBusy}
    <p class="qp-hint">{$t('options.chain.loadingExpiries')}</p>
  {/if}

  {#if opt.expiries}
    <section class="qp-block">
      <div class="head">
        <div class="meta">
          <strong>{opt.expiries.underlying ?? opt.underlying?.symbol}</strong>
          <span class="qp-muted">{$t('options.chain.spot', { v: price(spot) })}</span>
          {#if opt.expiries.trading_class}<span class="qp-muted">{opt.expiries.trading_class}</span>{/if}
          {#if asOf && sameUnderlying}<span class="qp-muted">{$t('options.chain.asOf', { t: asOf })}</span>{/if}
        </div>
        <div class="actions">
          <button class="icon" onclick={() => (pickerOpen = true)} aria-label={$t('options.columns.title')} title={$t('options.columns.title')}><Icon name="settings" /></button>
          {#if sameUnderlying}
            <button class="ghost" onclick={() => load(true)} disabled={busy} title={$t('options.chain.refreshHint')}><Icon name="refresh-cw" size={14} /> {$t('options.chain.refresh')}</button>
          {/if}
          <button class="primary" onclick={() => load(false)} disabled={busy || !picked.length || fresh}>
            {busy ? $t('options.chain.loading') : $t('options.chain.load')}
          </button>
        </div>
      </div>
      {#if ahead.length}
        <div class="divs">
          <label class="use"><input type="checkbox" bind:checked={opt.useDividends} /> {$t('options.dividends.use')}</label>
          <span class="qp-muted">
            {$t('options.dividends.summary', { n: ahead.length, d: ahead[0].ex_date, a: price(ahead[0].amount) })}{#if estimated}, {$t('options.dividends.projected', { n: estimated })}{/if}{#if opt.dividends.provider}. {$t('options.dividends.source', { p: opt.dividends.provider })}{/if}
          </span>
        </div>
      {:else if opt.dividends?.notice}
        <p class="qp-hint">{opt.dividends.notice}</p>
      {/if}
      <ChainFilters expiries={list} {picked} bind:expiry={opt.expiryFilter} bind:strike={opt.strikeFilter} bind:spread={opt.spreadFilter} bind:view={opt.view} />
      <p class="qp-hint">
        {$t('options.chain.picked', { n: picked.length, lo: price(win[0]), hi: price(win[1]) })}
        <InfoNote note={$t('options.chain.ivNote')} />
        {#if opt.assetType === 'index'}<InfoNote note={$t('options.chain.indexNote')} />{/if}
        {#if selected.length > MAX_EXPIRIES}<span class="qp-warn">{$t('options.chain.capped', { n: selected.length, max: MAX_EXPIRIES })}</span>{/if}
        {#if ratesStale}<span class="qp-warn">{$t('options.chain.staleRates')}</span>
        {:else if sameUnderlying && !fresh}<span class="qp-warn">{$t('options.chain.stale')}</span>{/if}
      </p>
    </section>
  {:else if !expBusy}
    <p class="qp-hint">{$t('options.chain.pickHint')}</p>
  {/if}

  {#if busy}<TaskProgress {progress} />{/if}
  <ErrorText {error} />

  {#each notices as n (n.code)}
    <div class="notice" role="status"><Icon name="alert-triangle" size={14} /> <span>{n.message}</span></div>
  {/each}

  {#if sameUnderlying}
    {#if groups.length && groups.some((g) => g.strikes.length)}
      <section class="qp-block flush">
        <ChainTable {groups} view={opt.view} {columns} spot={opt.chain.spot} onrow={openRow} />
      </section>
    {:else}
      <p class="qp-hint">{$t('options.chain.empty')}</p>
    {/if}
  {/if}
</div>

<ColumnPicker bind:open={pickerOpen} bind:layout />
<ContractDrawer
  bind:open={drawerOpen}
  row={drawerRow}
  spot={opt.chain?.spot ?? null}
  rate={opt.chain?.rate ?? opt.rate}
  onadd={add}
/>

<style>
  .num {
    flex: 0 1 110px;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-3);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .qp-hint .qp-warn {
    margin-left: var(--space-2);
  }
  .notice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-left: 2px solid var(--amber);
    background: color-mix(in srgb, var(--amber) 8%, var(--surface));
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    font-size: var(--fs-desc);
  }
  .flush {
    padding: 0;
    overflow: hidden;
  }
  .divs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
    font-size: var(--fs-desc);
  }
  .use {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }
  /* Phone: rate and dividend share a line, the actions wrap and take touch-sized targets. */
  @media (max-width: 767px) {
    .num {
      flex: 1 1 calc(50% - var(--space-4));
    }
    .num input {
      min-height: 44px;
    }
    .meta {
      min-width: 0;
      overflow-wrap: anywhere;
    }
    .actions {
      flex-wrap: wrap;
    }
    .actions button {
      min-height: 44px;
      height: auto;
    }
    .actions button.icon {
      width: 44px;
      height: 44px;
    }
    .use {
      min-height: 44px;
    }
  }
</style>
