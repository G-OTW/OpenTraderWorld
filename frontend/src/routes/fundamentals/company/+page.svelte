<script>
  // Fundamentals: Company. Ticker search, an identity header with the few numbers that
  // matter at a glance, then one tab per data family. Tabs, overview tiles and statement
  // display are the user's choice (Customize). `?t=AAPL&tab=financials` deep-links.
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import OrderList from '$lib/modules/fundamentals/OrderList.svelte';
  import SymbolPicker from '$lib/modules/fundamentals/SymbolPicker.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Overview from '$lib/modules/fundamentals/company/Overview.svelte';
  import Financials from '$lib/modules/fundamentals/company/Financials.svelte';
  import Estimates from '$lib/modules/fundamentals/company/Estimates.svelte';
  import Earnings from '$lib/modules/fundamentals/company/Earnings.svelte';
  import Segments from '$lib/modules/fundamentals/company/Segments.svelte';
  import Capital from '$lib/modules/fundamentals/company/Capital.svelte';
  import Ownership from '$lib/modules/fundamentals/company/Ownership.svelte';
  import Peers from '$lib/modules/fundamentals/company/Peers.svelte';
  import Ratings from '$lib/modules/fundamentals/company/Ratings.svelte';
  import Filings from '$lib/modules/fundamentals/company/Filings.svelte';
  import Transcripts from '$lib/modules/fundamentals/company/Transcripts.svelte';
  import { fundamentalsApi, fmtBig, fmtNum, fmtPct } from '$lib/modules/fundamentals/api.js';
  import { prefs, METRIC_GROUPS } from '$lib/modules/fundamentals/prefs.svelte.js';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { errorText } from '$lib/modules/fundamentals/deferred.js';
  import { t } from '$lib/i18n';

  const VIEWS = { overview: Overview, financials: Financials, estimates: Estimates, earnings: Earnings, segments: Segments, capital: Capital, ownership: Ownership, peers: Peers, ratings: Ratings, filings: Filings, transcripts: Transcripts };

  const tabs = $derived(prefs.company.tabs.filter((id) => !prefs.company.hiddenTabs.includes(id)));

  const urlTab = $page.url.searchParams.get('tab');
  let tab = $state(urlTab && VIEWS[urlTab] ? urlTab : 'overview');
  if ($page.url.searchParams.get('t')) prefs.company.ticker = $page.url.searchParams.get('t').toUpperCase();
  const ticker = $derived(prefs.company.ticker);

  let company = $state(null);
  let error = $state('');
  let stored = $state([]);
  // The latest close drives the header and the market ratios: fetched once the company
  // is stored, then the profile is re-read so its ratios include it.
  let price = $state(null);
  let priced = '';

  async function loadPrice(tk) {
    try {
      const snapshot = await fundamentalsApi.ensure('price', tk);
      if (prefs.company.ticker !== tk) return;
      price = { snapshot, error: snapshot.error ?? '' };
      company = await fundamentalsApi.company(tk);
    } catch (e) {
      if (prefs.company.ticker === tk) price = { snapshot: null, error: errorText($t, e) };
    }
  }

  const loadStored = () => fundamentalsApi.companies().then((c) => (stored = c)).catch(() => {});

  // Opening a ticker stores it and fetches it from EDGAR in the background; the profile
  // polls until that run settles.
  $effect(() => {
    const tk = ticker;
    company = null;
    error = '';
    price = null;
    let timer;
    let stop = false;
    let touched = false;
    const load = () =>
      fundamentalsApi
        .company(tk)
        .then((c) => {
          if (stop) return;
          company = c;
          if (c.status === 'pending') timer = setTimeout(load, 2500);
          else {
            // Opened: it moves to the front of the recent ones.
            if (!touched) {
              touched = true;
              fundamentalsApi.openedCompany(tk).catch(() => {}).finally(loadStored);
            } else loadStored();
            if (c.status === 'ok' && priced !== tk) {
              priced = tk;
              loadPrice(tk);
            }
          }
        })
        .catch((e) => !stop && (error = e.message));
    load();
    return () => {
      stop = true;
      clearTimeout(timer);
    };
  });

  async function follow() {
    await setFollowed(company.ticker, !company.followed);
  }

  async function setFollowed(tk, followed) {
    await fundamentalsApi.followCompany(tk, followed);
    if (company?.ticker === tk) company = { ...company, followed };
    loadStored();
  }

  const lookup = async (q) =>
    (await fundamentalsApi.searchCompanies(q)).map((s) => ({
      ticker: s.ticker,
      name: s.name,
      detail: [s.exchange, s.sector].filter(Boolean).join(' · ')
    }));

  async function refresh() {
    try {
      await fundamentalsApi.refreshCompany(company.ticker);
      const tk = company.ticker;
      company = { ...company, status: 'pending', error: null };
      const poll = async () => {
        const c = await fundamentalsApi.company(tk);
        if (prefs.company.ticker !== tk) return;
        company = c;
        if (c.status === 'pending') setTimeout(poll, 2500);
      };
      setTimeout(poll, 2500);
    } catch (e) {
      error = e.message;
    }
  }

  // A hidden tab cannot stay selected.
  $effect(() => {
    if (!tabs.includes(tab)) tab = tabs[0] ?? 'overview';
  });

  // Keep the URL in step so a tab is bookmarkable.
  $effect(() => {
    const url = `/fundamentals/company?t=${encodeURIComponent(ticker)}&tab=${tab}`;
    if ($page.url.pathname + $page.url.search !== url) goto(url, { replaceState: true, keepFocus: true, noScroll: true });
  });

  function pick(tk) {
    prefs.company.ticker = tk.toUpperCase();
  }

  function toggleTile(k) {
    const cur = prefs.company.tiles;
    prefs.company.tiles = cur.includes(k) ? cur.filter((x) => x !== k) : [...cur, k];
  }

  const View = $derived(VIEWS[tab]);
</script>

<Shell title={$t('fundamentals.company.title')} subtitle={$t('fundamentals.company.subtitle')} page="company">
  {#snippet options()}
    <div>
      <h3>{$t('fundamentals.cust.tabs')}</h3>
      <p class="hint">{$t('fundamentals.cust.tabsHint')}</p>
      <OrderList
        items={prefs.company.tabs.map((id) => ({ id, label: $t(`fundamentals.tab.${id}`) }))}
        hidden={prefs.company.hiddenTabs}
        onchange={(order, hidden) => {
          prefs.company.tabs = order;
          prefs.company.hiddenTabs = hidden.length === order.length ? hidden.slice(1) : hidden;
        }}
      />
    </div>
    <div>
      <h3>{$t('fundamentals.cust.tiles')}</h3>
      <p class="hint">{$t('fundamentals.cust.tilesHint')}</p>
      {#each Object.entries(METRIC_GROUPS) as [g, keys] (g)}
        <div class="tilegrp">
          <span class="fd-muted">{$t(`fundamentals.metricGroup.${g}`)}</span>
          <div class="fd-row">
            {#each keys as k (k)}
              <button class="chip" class:active={prefs.company.tiles.includes(k)} onclick={() => toggleTile(k)}>{$t(`fundamentals.metric.${k}`)}</button>
            {/each}
          </div>
        </div>
      {/each}
    </div>
    <div>
      <h3>{$t('fundamentals.cust.statements')}</h3>
      <div class="fd-row">
        <span class="fd-muted">{$t('fundamentals.cust.units')}</span>
        {#each ['auto', 'M', 'B'] as u (u)}
          <button class="chip" class:active={prefs.financials.units === u} onclick={() => (prefs.financials.units = u)}>{u === 'auto' ? $t('fundamentals.cust.unitsAuto') : u}</button>
        {/each}
      </div>
      <label class="fd-muted check"><input type="checkbox" bind:checked={prefs.financials.growth} /> {$t('fundamentals.cust.growth')}</label>
    </div>
  {/snippet}

  <div class="search">
    <SymbolPicker items={stored} value={ticker} placeholder={$t('fundamentals.company.search')} {lookup} onpick={pick} onfollow={setFollowed} />
  </div>

  {#if error}
    <ErrorText {error} />
  {:else if !company}
    <Skeleton rows={6} />
  {:else}
    <section class="head">
      <div class="id">
        <div class="fd-row">
          <h2 class="tk">{company.ticker}</h2>
          <span class="nm">{company.name}</span>
        </div>
        <div class="fd-row badges">
          <Badge>{company.exchange}</Badge>
          <Badge>{company.sector}</Badge>
          <Badge>{company.industry}</Badge>
          <Badge>{company.country}</Badge>
        </div>
      </div>
      <div class="glance">
        <div><span class="lbl">{$t('fundamentals.price')}</span><span class="v">{fmtNum(company.price)} <small>{company.currency}</small></span><span class="num" class:fd-up={company.change_pct > 0} class:fd-down={company.change_pct < 0}>{fmtPct(company.change_pct, 2, true)}</span></div>
        <div><span class="lbl">{$t('fundamentals.metric.marketCap')}</span><span class="v">{fmtBig(company.metrics.market_cap)}</span></div>
        <div><span class="lbl">{$t('fundamentals.metric.pe')}</span><span class="v">{fmtNum(company.metrics.pe, 1)}</span></div>
        <div><span class="lbl">{$t('fundamentals.metric.divYield')}</span><span class="v">{fmtPct(company.metrics.dividend_yield, 2)}</span></div>
      </div>
      <div class="fd-row">
        <Button size="sm" icon={company.followed ? 'check' : 'star'} onclick={follow}>
          {company.followed ? $t('fundamentals.following') : $t('fundamentals.follow')}
        </Button>
        <Button size="sm" icon="refresh-cw" disabled={company.status === 'pending'} onclick={refresh}>{$t('fundamentals.refresh')}</Button>
      </div>
    </section>

    {#if company.status === 'pending'}
      <p class="fd-muted status">{$t('fundamentals.company.fetching')}</p>
    {:else if company.status === 'error'}
      <ErrorText error={company.error} />
    {/if}

    <div class="fd-scroll"><Tabs tabs={tabs.map((id) => ({ id, label: $t(`fundamentals.tab.${id}`) }))} bind:value={tab} ariaLabel={$t('fundamentals.company.title')} /></div>

    {#key `${company.ticker}:${tab}:${company.status}:${company.updated}`}
      <div class="fd-stack"><View {company} {price} onpick={pick} /></div>
    {/key}
  {/if}
</Shell>

<style>
  .search {
    position: relative;
  }
  .status {
    margin: 0;
  }
  .head {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) auto auto;
    align-items: center;
    gap: var(--space-6);
    padding: var(--fd-pad);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--fd-radius);
    border-top: 2px solid var(--accent);
    box-shadow: var(--shadow-1);
  }
  .tk {
    margin: 0;
    font-size: 28px;
    letter-spacing: -0.04em;
    font-weight: var(--fw-medium);
  }
  .nm {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .badges {
    margin-top: var(--space-2);
  }
  .glance {
    display: flex;
    gap: var(--space-4) var(--space-6);
    flex-wrap: wrap;
  }
  .glance > div {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-left: var(--space-4);
    border-left: var(--hairline) solid var(--border);
  }
  .lbl {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .v {
    font-family: var(--mono);
    font-size: 22px;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.03em;
  }
  .v small {
    font-size: 11px;
    color: var(--dim);
  }
  .glance .num {
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  .tilegrp {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  @media (max-width: 1000px) {
    .head {
      grid-template-columns: 1fr;
    }
  }
</style>
