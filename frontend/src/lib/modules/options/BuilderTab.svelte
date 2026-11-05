<script>
  // Builder tab: legs from the handbook, the chain or by hand; their payoff at expiry, at a
  // chosen date and along the way, with an IV shift and a volatility regime; the stats; saved
  // strategies and a comparison overlay. Every change is evaluated on the server, debounced.
  import '$lib/modules/quant/panel.css';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import HandbookPicker from './HandbookPicker.svelte';
  import LegsEditor from './LegsEditor.svelte';
  import PayoffChart from './PayoffChart.svelte';
  import StrategyStats from './StrategyStats.svelte';
  import StrategyList from './StrategyList.svelte';
  import { optionsApi } from './api.js';
  import { opt, legId, legMissing, cashDividends, dividendYield, optionStyle } from './store.svelte.js';
  import { applyTemplate } from './handbook.js';
  import { nearestIndex, chainSmiles } from './chain.js';
  import { price, pct } from './columns.js';
  import { t } from '$lib/i18n';

  const DEBOUNCE = 250;
  const DAY = 86400000;

  let spotInput = $state(opt.chain?.spot ?? null);
  let days = $state(0);
  let ivShift = $state(0);
  let ev = $state(null);
  let compare = $state([]);
  let compareEv = $state({});
  let error = $state('');
  let busy = $state(false);
  // Phone: one pane at a time, the legs or their payoff and stats.
  let pane = $state('legs');

  // A newly loaded chain brings its own spot.
  let chainSpot = opt.chain?.spot ?? null;
  $effect(() => {
    const s = opt.chain?.spot ?? null;
    if (s !== chainSpot) {
      chainSpot = s;
      if (s != null) spotInput = s;
    }
  });

  const spot = $derived(Number.isFinite(Number(spotInput)) && Number(spotInput) > 0 ? Number(spotInput) : null);
  // The rate and dividend set in the Chain tab, followed as soon as they are edited.
  const rate = $derived(opt.rate);
  const dividend = $derived(dividendYield());
  const cash = $derived(cashDividends());

  // Dates run on the New York calendar, the one the server checks the value date against, so
  // a browser in another time zone never sends a date New York already left behind.
  const today = new Intl.DateTimeFormat('en-CA', { timeZone: 'America/New_York' }).format(new Date());
  const utcDay = (iso) => {
    const [y, m, d] = iso.split('-').map(Number);
    return Date.UTC(y, m - 1, d);
  };
  const daysFromToday = (iso) => Math.round((utcDay(iso) - utcDay(today)) / DAY);

  // The date slider runs from today to the nearest expiry among the option legs.
  const optionLegs = $derived(opt.legs.filter((l) => l.kind !== 'stock' && l.expiry));
  const maxDays = $derived(optionLegs.length ? Math.max(0, Math.min(...optionLegs.map((l) => daysFromToday(l.expiry)))) : 0);
  $effect(() => {
    if (days > maxDays) days = maxDays;
  });
  const atDate = $derived(new Date(utcDay(today) + Number(days) * DAY).toISOString().slice(0, 10));

  const bare = (legs) => legs.map(({ kind, strike, expiry, qty, price: p, iv }) => ({ kind, strike, expiry, qty: Number(qty), price: p == null || p === '' ? null : Number(p), iv }));
  const ready = (legs) => legs.length && legs.every((l) => !legMissing(l));

  // ATM IV for the probability of profit: the chain row nearest the chain's spot, on the
  // nearest expiry among the option legs, mean of the call and put IVs it has.
  function atmIv(legs) {
    const rows = opt.chain?.rows;
    const s = opt.chain?.spot ?? spot;
    const dates = legs.filter((l) => l.kind !== 'stock' && l.expiry).map((l) => l.expiry);
    if (!rows?.length || s == null || !dates.length) return null;
    const expiry = dates.sort()[0];
    const withIv = rows.filter((r) => r.expiry === expiry && Number.isFinite(r.iv_mid) && r.iv_mid > 0 && r.iv_mid <= 5);
    if (!withIv.length) return null;
    const strikes = [...new Set(withIv.map((r) => r.strike))].sort((a, b) => a - b);
    const k = strikes[nearestIndex(strikes, s)];
    const ivs = withIv.filter((r) => r.strike === k).map((r) => r.iv_mid);
    return ivs.reduce((a, b) => a + b, 0) / ivs.length;
  }

  function range(legs) {
    const ks = legs.filter((l) => l.strike != null).map((l) => l.strike);
    const lo = Math.min(spot * 0.7, ...ks.map((k) => k * 0.9));
    const hi = Math.max(spot * 1.3, ...ks.map((k) => k * 1.1));
    return { lo, hi, steps: 121 };
  }

  function body(legs) {
    const b = {
      spot,
      rate,
      dividend,
      dividends: cash,
      style: optionStyle(),
      legs,
      at: optionLegs.length ? atDate : 'expiry',
      iv_shift: Number(ivShift) / 100,
      vol_regime: opt.volRegime,
      smile: chainSmiles(opt.chain?.rows, opt.chain?.spot ?? spot, [...new Set(legs.map((l) => l.expiry).filter(Boolean))]),
      range: range(legs)
    };
    const atm = atmIv(legs);
    if (atm != null) b.atm_iv = atm;
    return b;
  }

  let timer;
  let ticket = 0;
  $effect(() => {
    const legs = bare(opt.legs);
    const others = compare.map((c) => ({ id: c.id, legs: bare(c.legs) }));
    const ok = spot != null && ready(legs);
    const main = ok ? body(legs) : null;
    const extra = spot != null ? others.filter((c) => ready(c.legs)).map((c) => ({ id: c.id, body: body(c.legs) })) : [];
    clearTimeout(timer);
    if (!main && !extra.length) {
      ev = null;
      compareEv = {};
      return;
    }
    timer = setTimeout(async () => {
      const mine = ++ticket;
      busy = true;
      try {
        const [m, ...rest] = await Promise.all([main ? optionsApi.evaluate(main) : null, ...extra.map((c) => optionsApi.evaluate(c.body))]);
        if (mine !== ticket) return;
        ev = m;
        compareEv = Object.fromEntries(extra.map((c, i) => [c.id, rest[i]]));
        error = '';
      } catch (e) {
        if (mine === ticket) error = e.message;
      } finally {
        if (mine === ticket) busy = false;
      }
    }, DEBOUNCE);
    return () => clearTimeout(timer);
  });

  function useTemplate(h) {
    try {
      opt.legs = applyTemplate(h, opt.chain);
      opt.strategyId = null;
      opt.strategyName = $t(`options.handbook.${h.id}`);
      loadedUnderlying = null;
      error = '';
    } catch (e) {
      error = $t(e.message);
    }
  }

  function load(s) {
    opt.legs = (s.legs ?? []).map((l) => ({ ...l, id: legId() }));
    opt.strategyId = s.id;
    opt.strategyName = s.name;
    loadedUnderlying = s.underlying || null;
  }

  function toggleCompare(s) {
    if (compare.some((c) => c.id === s.id)) compare = compare.filter((c) => c.id !== s.id);
    else compare = [...compare, { id: s.id, name: s.name, legs: s.legs ?? [] }];
  }

  function snapshot() {
    compare = [...compare, { id: legId(), name: opt.strategyName || $t('options.compare.snapshot', { n: compare.length + 1 }), legs: opt.legs.map((l) => ({ ...l })) }];
  }

  // Legs that do not all expire together are read at the first expiry, the later ones by model.
  const mixed = $derived(new Set(optionLegs.map((l) => l.expiry)).size > 1);

  // A saved strategy on another underlying than the loaded chain prices against the wrong spot.
  let loadedUnderlying = $state(null);
  const mismatch = $derived(loadedUnderlying && opt.chain && opt.chain.picked !== loadedUnderlying);

  const curves = $derived([
    ...(ev ? [{ name: opt.strategyName || $t('options.compare.current'), payoff: ev.payoff, breakevens: ev.breakevens, slot: 0, path: compare.length ? [] : ev.path.filter((c) => c.date !== ev.at) }] : []),
    ...compare
      .map((c, i) => ({ name: c.name, payoff: compareEv[c.id]?.payoff, breakevens: compareEv[c.id]?.breakevens, slot: i + 1 }))
      .filter((c) => c.payoff)
  ]);
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field ctx">
      <span class="qp-label">{$t('options.builder.underlying')}</span>
      <span class="val">{opt.chain?.underlying ?? opt.underlying?.symbol ?? '·'}</span>
    </div>
    <label class="qp-field ctx"><span class="qp-label">{$t('options.builder.spot')}</span><input type="number" step="0.01" min="0" bind:value={spotInput} /></label>
    <div class="qp-field ctx">
      <span class="qp-label">{$t('options.builder.rates')}</span>
      <span class="val">{pct(rate)} · {cash.length ? $t('options.builder.cash', { n: cash.length }) : pct(dividend)}</span>
    </div>
    <label class="qp-field slider">
      <span class="qp-label">{$t('options.builder.date', { d: optionLegs.length ? atDate : $t('options.payoff.atExpiry') })}</span>
      <input type="range" min="0" max={maxDays} step="1" bind:value={days} disabled={!maxDays} />
    </label>
    <label class="qp-field slider">
      <span class="qp-label">{$t('options.builder.ivShift', { v: `${ivShift > 0 ? '+' : ''}${ivShift}` })}</span>
      <input type="range" min="-20" max="20" step="1" bind:value={ivShift} />
    </label>
    <div class="qp-field regime">
      <span class="qp-label">{$t('options.builder.regime')}</span>
      <div class="qp-seg">
        {#each ['sticky_strike', 'sticky_delta'] as r (r)}
          <button class:active={opt.volRegime === r} title={$t(`options.builder.${r}Hint`)} onclick={() => (opt.volRegime = r)}>{$t(`options.builder.${r}`)}</button>
        {/each}
      </div>
    </div>
  </div>

  {#if !opt.chain}<p class="qp-hint">{$t('options.builder.noChain')}</p>{/if}
  {#if mismatch}<p class="qp-hint qp-warn">{$t('options.builder.mismatch')}</p>{/if}

  <nav class="mobile-sections" aria-label={$t('options.tab.builder')}>
    <button class:current={pane === 'legs'} aria-pressed={pane === 'legs'} onclick={() => (pane = 'legs')}>{$t('options.legs.title')}</button>
    <button class:current={pane === 'payoff'} aria-pressed={pane === 'payoff'} onclick={() => (pane = 'payoff')}>{$t('options.payoff.title')}</button>
  </nav>

  <div class="pane" class:mobile-hidden={pane !== 'legs'}>
    <HandbookPicker onpick={useTemplate} disabled={!opt.chain} />
    <LegsEditor bind:legs={opt.legs} chain={opt.chain} {spot} />
  </div>
  <ErrorText {error} />

  <section class="qp-block" class:mobile-hidden={pane !== 'payoff'}>
    <div class="head">
      <h3>{$t('options.payoff.title')}{#if busy}<span class="qp-muted"> …</span>{/if}</h3>
      <button class="ghost" onclick={snapshot} disabled={!ev}><Icon name="git-compare" size={14} /> {$t('options.compare.add')}</button>
    </div>
    {#if curves.length}
      <PayoffChart {curves} {spot} height={340} front={mixed} />
    {:else}
      <p class="qp-hint">{spot == null ? $t('options.builder.needSpot') : $t('options.builder.needLegs')}</p>
    {/if}
    {#if compare.length}
      <div class="cmp">
        {#each compare as c, i (c.id)}
          <span class="tag">
            <span class="dot" style:background={`var(--chart-${((i + 1) % 8) + 1})`}></span>
            {c.name}
            <button class="icon" onclick={() => (compare = compare.filter((x) => x.id !== c.id))} aria-label={$t('options.compare.remove')}><Icon name="x" size={12} /></button>
          </span>
        {/each}
      </div>
    {/if}
    <StrategyStats {ev} />
    {#if ev}<p class="qp-hint">{$t('options.stats.units', { s: price(spot) })}</p>{/if}
    {#if ev && mixed}<p class="qp-hint">{$t('options.payoff.mixedHint')}</p>{/if}
  </section>

  <div class="pane" class:mobile-hidden={pane !== 'legs'}>
    <StrategyList onload={load} oncompare={toggleCompare} comparing={compare.map((c) => c.id)} />
  </div>
</div>

<style>
  .ctx {
    flex: 0 1 140px;
  }
  .val {
    font-family: var(--mono);
    min-height: var(--control-h);
    display: flex;
    align-items: center;
  }
  .slider {
    flex: 1 1 200px;
  }
  .qp-field.regime {
    flex: 0 0 auto;
  }
  .slider input {
    accent-color: var(--accent);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .cmp {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    padding: 0 0 0 var(--space-2);
    font-size: var(--fs-desc);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  /* A pane wrapper only exists for the phone switch: it adds no box of its own. */
  .pane {
    display: contents;
  }
  .mobile-sections {
    display: none;
  }
  @media (max-width: 767px) {
    .ctx {
      flex: 1 1 calc(50% - var(--space-4));
    }
    .slider,
    .qp-field.regime {
      flex: 1 1 100%;
    }
    .val {
      overflow-wrap: anywhere;
    }
    .ctx input,
    .slider input {
      min-height: 44px;
    }
    .qp-seg button,
    .head button {
      min-height: 44px;
      height: auto;
    }
    .tag {
      max-width: 100%;
      overflow-wrap: anywhere;
    }
    .tag button.icon {
      flex: none;
      width: 44px;
      height: 44px;
    }
    .mobile-sections {
      display: flex;
      gap: var(--space-2);
    }
    .mobile-sections button {
      flex: 1;
      min-width: 0;
      min-height: 44px;
      padding: var(--space-2);
      border: 1px solid var(--border);
      background: var(--surface);
      color: var(--text);
      white-space: normal;
      overflow-wrap: anywhere;
    }
    .mobile-sections button.current {
      border-color: var(--accent);
      color: var(--accent);
    }
    .mobile-hidden {
      display: none;
    }
  }
</style>
