<script>
  // Watchlist widgets — the cards of the Watchlists mockup, each a `variant` of one
  // component. All of them read the one `detail(id)` call the module already serves
  // (items with their cached quote, plus the list's alerts); nothing new server-side.
  //
  // Config: { variant, watchlist_id, limit, symbol, window }.
  import { watchlistsApi, fmtQuote, fmtSignedPct, agoLabel } from '$lib/modules/watchlists/api.js';
  import { t, locale } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';
  import Spark from './parts/Spark.svelte';
  import TrendChart from './parts/TrendChart.svelte';
  import Donut from './parts/Donut.svelte';
  import Stat from './parts/Stat.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? (item.config?.order === 'movers' ? 'movers' : 'quotes'));
  const order = $derived(variant === 'movers' ? 'movers' : 'list');
  const limit = $derived(Math.max(1, Math.min(50, item.config?.limit ?? 10)));
  // Which change column the movers / best-worst cards rank on.
  const win = $derived(item.config?.window ?? '24h');
  const changeOf = (it) =>
    win === '7d' ? it.quote?.change_7d : win === '30d' ? it.quote?.change_30d : it.quote?.change_24h;

  let items = $state(null);
  let alerts = $state([]);
  let list = $state(null);
  let noList = $state(false);
  let err = $state('');

  // Refreshes run every few seconds, so a slow response can land after a newer one:
  // only the newest request in flight is allowed to write.
  let seq = 0;
  async function load(wantedId) {
    const mine = ++seq;
    err = '';
    noList = false;
    try {
      const lists = await watchlistsApi.list();
      const id = lists.some((w) => w.id === wantedId) ? wantedId : lists[0]?.id;
      if (mine !== seq) return;
      if (!id) {
        noList = true;
        items = [];
        return;
      }
      const d = await watchlistsApi.detail(id);
      if (mine !== seq) return;
      items = d.items;
      alerts = d.alerts ?? [];
      list = d.watchlist;
    } catch (e) {
      if (mine === seq) err = e.message;
    }
  }
  const live = livePulse(LIVE.quotes);
  $effect(() => {
    const wanted = item.config?.watchlist_id;
    live.n;
    if (!editing) load(wanted);
  });

  // Colour alone must not carry the direction, so the class rides alongside the
  // signed figure fmtSignedPct already produces.
  // Neutral below half a basis point of a percent: that is what fmtSignedPct rounds to
  // "0.00%", and a figure that prints as zero must not be coloured as a move.
  function cls(n) {
    if (n == null || !isFinite(n) || Math.abs(n) < 0.005) return '';
    return n < 0 ? 'w-neg' : 'w-pos';
  }

  const shown = $derived.by(() => {
    const rows = [...(items ?? [])];
    if (order === 'movers') rows.sort((a, b) => Math.abs(changeOf(b) ?? 0) - Math.abs(changeOf(a) ?? 0));
    return rows.slice(0, limit);
  });

  const ranked = $derived(
    [...(items ?? [])].filter((it) => changeOf(it) != null).sort((a, b) => changeOf(b) - changeOf(a))
  );
  const best = $derived(ranked[0] ?? null);
  const worst = $derived(ranked.at(-1) ?? null);

  // The single-symbol card: the configured item, else the first of the list.
  const focus = $derived(
    (items ?? []).find((it) => it.symbol === item.config?.symbol) ?? (items ?? [])[0] ?? null
  );

  const byClass = $derived.by(() => {
    const counts = new Map();
    for (const it of items ?? []) counts.set(it.asset_class, (counts.get(it.asset_class) ?? 0) + 1);
    return [...counts]
      .map(([key, value]) => ({ label: $t(`watchlists.class.${key}`), value }))
      .sort((a, b) => b.value - a.value);
  });

  // Freshness is the newest stamp on the list: one stale row is a row problem, all of
  // them stale is a sync problem, and that is what this card is for.
  const lastQuoted = $derived(
    list?.refreshed_at ?? (items ?? []).map((it) => it.quoted_at).filter(Boolean).sort().at(-1) ?? null
  );

  // Alerts ordered by how close the live price is to the threshold, nearest first. Only
  // price alerts have a comparable distance; a percentage-move alert does not.
  const nearTrigger = $derived.by(() => {
    const byItem = new Map((items ?? []).map((it) => [it.id, it]));
    return alerts
      .filter((a) => a.enabled && a.metric === 'price' && a.threshold)
      .map((a) => {
        const it = byItem.get(a.item_id);
        const price = it?.quote?.price_usd;
        return { a, it, price, gap: price == null ? null : Math.abs(price - a.threshold) / a.threshold };
      })
      .filter((r) => r.it && r.gap != null)
      .sort((x, y) => x.gap - y.gap)
      .slice(0, limit);
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={items === null}
  empty={noList || items?.length === 0}
  preview={$t('dashboard.widgets.watchlist.preview')}
  emptyText={noList ? $t('dashboard.widgets.watchlist.noList') : $t('dashboard.widgets.watchlist.empty')}
  rows={4}
>
  {#if variant === 'movers'}
    <ul class="w-list">
      {#each shown as it (it.id)}
        <li class="w-row">
          <span class="ticker">{it.symbol}</span>
          <span class="w-num grow {cls(changeOf(it))}">{fmtSignedPct(changeOf(it))}</span>
          {#if it.quote?.spark?.length > 1}
            <span class="minispark">
              <Spark values={it.quote.spark} height={22} area={false} valueFormat={fmtQuote}
                label={it.symbol} />
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if variant === 'bestworst'}
    <div class="w-body w-split">
      {#each [{ k: 'best', it: best }, { k: 'worst', it: worst }] as row (row.k)}
        {#if row.it}
          <div class="perf">
            <span class="w-eyebrow">
              {row.k === 'best' ? $t('dashboard.widgets.watchlist.best') : $t('dashboard.widgets.watchlist.worst')}
            </span>
            <div class="perfline">
              <span class="ident">
                <span class="ticker">{row.it.symbol}</span>
                <span class="w-sub w-name">{row.it.name}</span>
              </span>
              <span class="w-num {cls(changeOf(row.it))}">{fmtSignedPct(changeOf(row.it))}</span>
            </div>
            {#if row.it.quote?.spark?.length > 1}
              <Spark values={row.it.quote.spark} height={30} valueFormat={fmtQuote}
                label={row.it.symbol} />
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {:else if variant === 'spark'}
    {#if focus}
      <div class="w-body tight">
        <!-- Symbol, instrument and the list it was taken from on one line: a fourth row
             pushes the chart out of a single-height card. -->
        <span class="ident row">
          <span class="ticker">{focus.symbol}</span>
          <span class="w-sub w-name grow">{focus.name}</span>
          {#if list?.name}<span class="w-eyebrow w-name src">{list.name}</span>{/if}
        </span>
        <Stat
          value={fmtQuote(focus.quote?.price_usd)}
          size="sm"
          delta={fmtSignedPct(changeOf(focus))}
          deltaTone={cls(changeOf(focus)) === 'w-neg' ? 'neg' : 'pos'}
        />
        {#if focus.quote?.spark?.length > 1}
          <!-- The card prints its own headline (the live quote and the configured window's
               change), so the chart carries the grid and the axis only. -->
          <TrendChart
            points={focus.quote.spark.map((v) => ({ value: v }))}
            tone={cls(changeOf(focus)) === 'w-neg' ? 'neg' : 'pos'}
            format={fmtQuote}
            showValue={false}
            showPill={false}
            label={focus.symbol}
          />
        {/if}
      </div>
    {/if}
  {:else if variant === 'sync'}
    <div class="w-body">
      <div class="syncline">
        <span class="w-dot" class:stale={!lastQuoted}></span>
        <span class="synctitle">
          {lastQuoted ? $t('dashboard.widgets.watchlist.upToDate') : $t('dashboard.widgets.watchlist.neverSynced')}
        </span>
      </div>
      <ul class="w-list">
        <li class="w-row">
          <span class="w-name grow">{$t('dashboard.widgets.watchlist.lastSync')}</span>
          <span class="w-num">{lastQuoted ? agoLabel(lastQuoted, $locale) : '—'}</span>
        </li>
        <li class="w-row">
          <span class="w-name grow">{$t('dashboard.widgets.watchlist.interval')}</span>
          <span class="w-num">{list?.refresh_secs ? `${list.refresh_secs}s` : '—'}</span>
        </li>
      </ul>
    </div>
  {:else if variant === 'alerts'}
    {#if nearTrigger.length === 0}
      <p class="w-state">{$t('dashboard.widgets.watchlist.noAlerts')}</p>
    {:else}
      <table class="w-tbl">
        <thead>
          <tr>
            <th>{$t('watchlists.table.symbol')}</th>
            <th>{$t('dashboard.widgets.watchlist.condition')}</th>
            <th class="num">{$t('dashboard.widgets.watchlist.current')}</th>
            <th class="num">{$t('dashboard.widgets.watchlist.target')}</th>
          </tr>
        </thead>
        <tbody>
          {#each nearTrigger as r (r.a.id)}
            <tr>
              <td class="ticker">{r.it.symbol}</td>
              <td class="w-sub">{r.a.direction === 'below' ? '≤' : '≥'} {fmtQuote(r.a.threshold)}</td>
              <td class="num">{fmtQuote(r.price)}</td>
              <td class="num">{fmtQuote(r.a.threshold)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if variant === 'classes'}
    <Donut segments={byClass} centerLabel={$t('dashboard.widgets.watchlist.assets')} />
  {:else}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('watchlists.table.symbol')}</th>
          <th class="num">{$t('watchlists.table.price')}</th>
          <th class="num">{$t('watchlists.table.h24')}</th>
          <th class="num">{$t('watchlists.table.d7')}</th>
        </tr>
      </thead>
      <tbody>
        {#each shown as it (it.id)}
          <tr>
            <td>
              <span class="ident">
                <span class="w-name" title={it.name}>{it.name}</span>
                <span class="ticker">{it.symbol}</span>
              </span>
            </td>
            <td class="num">{fmtQuote(it.quote?.price_usd)}</td>
            <td class="num {cls(it.quote?.change_24h)}">{fmtSignedPct(it.quote?.change_24h)}</td>
            <td class="num {cls(it.quote?.change_7d)}">{fmtSignedPct(it.quote?.change_7d)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</WidgetState>

<style>
  /* Ticker under the name: the symbol is what you scan, the name is what you read. */
  .ident {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.2;
    padding: 5px 0;
  }
  .ident.row {
    flex-direction: row;
    align-items: baseline;
    gap: var(--space-2);
    padding: 0;
  }
  /* The chart is the card; the identification lines above it stay as tight as they read. */
  .tight {
    gap: var(--space-2);
  }
  .ident .grow {
    flex: 1;
  }
  .src {
    flex-shrink: 0;
    max-width: 45%;
  }
  .ticker {
    font-family: var(--mono);
    font-size: 10.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .w-row .ticker,
  td.ticker {
    font-size: var(--text-sm);
    color: var(--text);
    flex-shrink: 0;
  }
  /* The name column takes what is left and truncates: a long company name must not
     widen the table past the card and push the figures out of view. */
  .w-tbl td:first-child {
    max-width: 0;
    width: 100%;
  }
  .grow {
    flex: 1;
    text-align: left;
  }
  .minispark {
    width: 68px;
    flex-shrink: 0;
  }
  /* Each half fills its column so the sparkline sits under its own figures rather than
     floating against the divider. */
  .perf {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .perfline {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    min-width: 0;
  }
  .syncline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .syncline .w-dot {
    background: var(--green);
  }
  .syncline .w-dot.stale {
    background: var(--faint);
  }
  .synctitle {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
</style>
