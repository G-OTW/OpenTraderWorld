<script>
  // What a page that shows an instrument offers to do with it: open it on the chart, add it to
  // a watchlist, download its history. A symbol read on the page is resolved first through the
  // provider symbol search (the chart's connectors), where only an exact match counts; several
  // exact matches are listed for the user to choose, and none is an error naming the fix. A
  // page that already holds provider coordinates (the options underlying) passes `coords`.
  // The watchlist side goes through the watchlist's own resolve, which applies the same rule.
  //
  //   <InstrumentActions symbol="AAPL" assetType="equity" name="Apple Inc." />
  //   <InstrumentActions symbol={u.symbol} coords={{ connector_id, provider, asset_type, ticker }} labels />
  //
  // props: symbol, name, assetType (histdata vocabulary, '' = any), coords, download (show the
  // Download action), labels (text beside the icons, for a header rather than a table row)
  import { goto } from '$app/navigation';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { histdataApi } from '$lib/modules/histdata/api.js';
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';
  import { t } from '$lib/i18n';
  import { exactHits, chartHref, watchKind, defaultTimeframe } from './resolve.js';

  let { symbol = '', name = '', assetType = '', coords = null, download = true, labels = false } = $props();

  let open = $state(false);
  let mode = $state('chart'); // 'chart' | 'download' | 'watch'
  let busy = $state(false);
  let error = $state('');
  // Exact search hits to choose from, then the one chosen (as coordinates).
  let candidates = $state([]);
  let picked = $state(null);
  // The success line of a queued download or an added symbol, with where to see it.
  let done = $state(null); // { text, href, label }

  const yearAgo = () => {
    const d = new Date();
    d.setFullYear(d.getFullYear() - 1);
    return d.toISOString().slice(0, 10);
  };
  let tf = $state('1d');
  let from = $state(yearAgo());

  let lists = $state(null);
  let listId = $state('');
  // Exact matches of an ambiguous ticker, as the watchlist resolve returns them.
  let watchCandidates = $state([]);

  const titles = { chart: 'instrument.chart', watch: 'instrument.watch', download: 'instrument.download' };

  function start(m) {
    mode = m;
    error = '';
    done = null;
    candidates = [];
    picked = null;
    watchCandidates = [];
    // Coordinates in hand: the chart opens without a dialog.
    if (m === 'chart' && coords) {
      goto(chartHref(coords));
      return;
    }
    open = true;
    if (m === 'watch') loadLists();
    else resolve();
  }

  const toCoords = (h) => ({
    connector_id: h.connector_id,
    provider: h.provider,
    asset_type: h.asset_type,
    ticker: h.symbol,
    timeframes: h.timeframes ?? [],
    label: h.label ?? h.connector ?? h.provider,
    exchange: h.exchange ?? '',
    name: h.name ?? ''
  });

  async function resolve() {
    if (coords) {
      choose({ ...coords, timeframes: coords.timeframes ?? ['1d'] });
      return;
    }
    busy = true;
    try {
      const r = await histvizApi.symbols({ q: symbol, asset_type: assetType });
      const hits = exactHits(symbol, r.results);
      if (!hits.length) {
        const why = (r.notes ?? []).map((n) => n.message).filter(Boolean);
        error = $t('instrument.noMatch', { symbol }) + (why.length ? ` (${why.join('; ')})` : '');
      } else if (hits.length === 1) {
        choose(toCoords(hits[0]));
      } else {
        candidates = hits.map(toCoords);
      }
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function choose(c) {
    picked = c;
    if (mode === 'chart') {
      open = false;
      goto(chartHref(c));
      return;
    }
    tf = defaultTimeframe(c.timeframes);
  }

  async function queue() {
    busy = true;
    error = '';
    try {
      await histdataApi.startDownload({
        connector_id: picked.connector_id ?? null,
        provider: picked.provider,
        asset_type: picked.asset_type,
        ticker: picked.ticker,
        timeframe: tf,
        from: `${from}T00:00:00Z`,
        to: new Date().toISOString()
      });
      done = { text: $t('instrument.queued', { symbol: picked.ticker }), href: '/histdata', label: $t('instrument.openHistdata') };
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function loadLists() {
    busy = true;
    try {
      lists = await watchlistsApi.list();
      if (!lists.some((l) => l.id === listId)) listId = lists[0]?.id ?? '';
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function addWatch(hit = null) {
    if (!listId) return;
    busy = true;
    error = '';
    try {
      if (!hit) {
        const r = await watchlistsApi.resolveBatch(listId, symbol, watchKind(assetType));
        const one = r.resolved.length + r.unresolved.length === 1;
        if (one && r.resolved.length) hit = r.resolved[0].hit;
        else {
          const u = r.unresolved[0];
          error = u?.reason ?? $t('instrument.noMatch', { symbol });
          watchCandidates = one ? (u?.candidates ?? []) : [];
          return;
        }
      }
      const { asset_class, provider, provider_id, symbol: sym, name: nm } = hit;
      await watchlistsApi.addItems(listId, [{ asset_class, provider, provider_id, symbol: sym, name: nm }]);
      const list = lists.find((l) => l.id === listId);
      watchCandidates = [];
      done = {
        text: $t('instrument.added', { symbol: sym, list: list?.name ?? '' }),
        href: `/watchlists?list=${listId}`,
        label: $t('instrument.openList')
      };
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<span class="ia" class:labels>
  <button class="btn sm" class:ghost={!labels} title={$t('instrument.chart')} aria-label={$t('instrument.chart')} onclick={() => start('chart')}>
    <Icon name="candlestick" size={14} />{#if labels}<span>{$t('instrument.chart')}</span>{/if}
  </button>
  <button class="btn sm" class:ghost={!labels} title={$t('instrument.watch')} aria-label={$t('instrument.watch')} onclick={() => start('watch')}>
    <Icon name="eye" size={14} />{#if labels}<span>{$t('instrument.watch')}</span>{/if}
  </button>
  {#if download}
    <button class="btn sm" class:ghost={!labels} title={$t('instrument.download')} aria-label={$t('instrument.download')} onclick={() => start('download')}>
      <Icon name="download" size={14} />{#if labels}<span>{$t('instrument.download')}</span>{/if}
    </button>
  {/if}
</span>

<Modal bind:open title={$t(titles[mode])} size="sm">
  <div class="ia-body">
    <p class="ia-sym"><strong>{symbol}</strong>{#if name}<span class="muted">{name}</span>{/if}</p>
    {#if busy && !lists && !candidates.length && !picked}<p class="muted">{$t('common.loading')}</p>{/if}
    <ErrorText {error} />

    {#if done}
      <p>{done.text}</p>
      <div class="ia-acts"><a class="btn" href={done.href}>{done.label}</a></div>
    {:else if mode === 'watch'}
      {#if lists && !lists.length}
        <p class="muted">{$t('instrument.noLists')}</p>
        <div class="ia-acts"><a class="btn" href="/watchlists">{$t('instrument.openWatchlists')}</a></div>
      {:else if lists}
        <div class="ia-field">
          <span>{$t('instrument.list')}</span>
          <Dropdown bind:value={listId} ariaLabel={$t('instrument.list')} options={lists.map((l) => ({ value: l.id, label: l.name }))} />
        </div>
        {#if watchCandidates.length}
          <ul class="ia-cands">
            {#each watchCandidates as c (c.provider + c.provider_id)}
              <li>
                <button class="ia-cand" disabled={busy} onclick={() => addWatch(c)}>
                  <strong>{c.symbol}</strong><span class="muted">{c.name} · {c.provider}</span>
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <div class="ia-acts"><Button variant="primary" loading={busy} disabled={!listId} onclick={() => addWatch()}>{$t('common.add')}</Button></div>
        {/if}
      {/if}
    {:else}
      {#if candidates.length && !picked}
        <p class="muted">{$t('instrument.pickOne', { symbol })}</p>
        <ul class="ia-cands">
          {#each candidates as c, i (i)}
            <li>
              <button class="ia-cand" onclick={() => choose(c)}>
                <strong>{c.ticker}</strong>
                <span class="muted">{[c.exchange, c.asset_type, c.label].filter(Boolean).join(' · ')}</span>
                {#if c.name}<span class="muted">{c.name}</span>{/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if mode === 'download' && picked}
        <p class="muted">{[picked.ticker, picked.exchange, picked.label].filter(Boolean).join(' · ')}</p>
        <div class="ia-row">
          <div class="ia-field">
            <span>{$t('instrument.timeframe')}</span>
            <Dropdown bind:value={tf} ariaLabel={$t('instrument.timeframe')} options={(picked.timeframes.length ? picked.timeframes : ['1d']).map((x) => ({ value: x, label: x }))} />
          </div>
          <label class="ia-field">
            <span>{$t('instrument.from')}</span>
            <input type="date" bind:value={from} />
          </label>
        </div>
        <div class="ia-acts"><Button variant="primary" icon="download" loading={busy} disabled={!from} onclick={queue}>{$t('instrument.queue')}</Button></div>
      {/if}
    {/if}
  </div>
</Modal>

<style>
  .ia {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
  }
  .ia .btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .ia:not(.labels) .btn {
    padding-inline: var(--space-2);
  }
  .ia-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .ia-body p {
    margin: 0;
  }
  .ia-sym {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    flex-wrap: wrap;
  }
  .muted {
    color: var(--muted);
  }
  .ia-row {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .ia-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 140px;
  }
  .ia-field > span {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .ia-acts {
    display: flex;
    justify-content: flex-end;
  }
  .ia-cands {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .ia-cand {
    width: 100%;
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    flex-wrap: wrap;
    text-align: left;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
  }
  .ia-cand:hover:not(:disabled) {
    border-color: var(--accent);
  }
</style>
