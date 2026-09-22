<script>
  // The market-data strip that sits above the analytics tabs.
  //
  // Three verbs, in the order they are used, and none of them happens behind the user's
  // back in manual mode:
  //
  //   Discover  reads what the filtered trades need against what the catalog holds
  //   Download  queues the missing windows as ordinary histdata jobs
  //   Measure   walks the trades against the stored bars and writes their excursions
  //
  // The download follows on the histdata job queue, so the progress line here is the same
  // one the historical-data page reads. Nothing on this bar talks to a provider.
  import {
    journalApi,
    ASSET_CLASSES,
    MARKET_TIMEFRAMES,
    MARKET_SYNC_MODES,
    MARKET_ASSET_TYPES
  } from './api.js';
  import { connectorsApi } from '$lib/connectors/api.js';
  import { histdataApi } from '$lib/modules/histdata/api.js';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SymbolHelp from '$lib/ui/SymbolHelp.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { fmtDateTime } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { filter = {}, onmeasured } = $props();

  let coverage = $state.raw(null);
  let settings = $state(null);
  let connectors = $state.raw([]);
  let busy = $state(''); // '' | 'discover' | 'sync' | 'compute'
  let error = $state('');
  let notice = $state('');
  let showSettings = $state(false);
  let showDetail = $state(false);
  // The batch the last download queued, and its live jobs.
  let batchId = $state('');
  let jobs = $state.raw([]);
  let poll = null;

  const STATUS_TONE = {
    ready: 'success',
    partial: 'warning',
    missing: 'neutral',
    no_connector: 'danger',
    needs_symbol: 'warning',
    unsupported: 'neutral',
    undated: 'neutral'
  };

  // Instruments whose journal ticker is not a contract: the symbol modal, and the draft
  // the user is filling in. `MNQ` is a root, `MNQU6` is what a provider can fetch.
  let showSymbols = $state(false);
  let draft = $state({});

  async function load() {
    const [s, c] = await Promise.all([
      journalApi.marketSettings(),
      connectorsApi.list('journal').catch(() => [])
    ]);
    settings = s.market;
    connectors = c;
  }

  async function discover() {
    busy = 'discover';
    error = '';
    notice = '';
    try {
      coverage = await journalApi.marketCoverage(filter);
      discovered = true;
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function download() {
    busy = 'sync';
    error = '';
    notice = '';
    try {
      const r = await journalApi.marketSync(filter);
      batchId = r.batch_id ?? '';
      if (r.queued === 0) {
        notice =
          r.skipped.length > 0
            ? $t('journal.market.syncSkipped', {
                reason: r.skipped.map((s) => `${s.ticker}: ${s.reason}`).join(', ')
              })
            : $t('journal.market.syncNothing');
      } else {
        notice = $t('journal.market.syncQueued', { count: r.queued });
        startPoll();
      }
      await discover();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function measure(full = false) {
    busy = full ? 'remeasure' : 'compute';
    error = '';
    notice = '';
    try {
      const r = await journalApi.marketCompute(filter, full);
      // The incremental run is the normal one, so it says what it skipped: a pass that
      // reports "3 measured" out of 900 trades has to explain the other 897.
      notice = r.reused
        ? $t('journal.market.computedIncremental', {
            measured: r.measured,
            reused: r.reused,
            considered: r.considered,
            timeframe: r.timeframe
          })
        : $t('journal.market.computed', {
            measured: r.measured,
            considered: r.considered,
            timeframe: r.timeframe
          });
      await discover();
      onmeasured?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  // Follow the queued batch on the shared job endpoint. Stops on its own once nothing is
  // live, and measures automatically: a download the user asked for is only useful once
  // the bars it fetched have been read.
  function startPoll() {
    stopPoll();
    poll = setInterval(async () => {
      try {
        const all = await histdataApi.jobs();
        jobs = batchId ? all.filter((j) => j.batch_id === batchId) : [];
        const live = jobs.some((j) =>
          ['queued', 'running', 'waiting', 'cancelling'].includes(j.status)
        );
        if (!live) {
          stopPoll();
          if (jobs.length > 0) {
            await measure();
          }
        }
      } catch {
        stopPoll();
      }
    }, 3000);
  }
  function stopPoll() {
    if (poll) clearInterval(poll);
    poll = null;
  }

  $effect(() => {
    load();
    return stopPoll;
  });

  // Re-read the coverage whenever the analytics filter moves: the scope is the scope.
  // The flag is deliberately not reactive: reading `coverage` here would re-run the
  // effect on the very assignment `discover()` makes, which is a loop, not a refresh.
  let discovered = false;
  $effect(() => {
    void filter;
    if (discovered) discover();
  });

  async function patch(p) {
    try {
      settings = await journalApi.saveMarketSettings(p);
      error = '';
      if (coverage) await discover();
    } catch (e) {
      error = e.message;
    }
  }

  // Asset classes carry their label in the module's own vocabulary, not in the packs.
  const assetLabel = (id) => ASSET_CLASSES.find((a) => a.id === id)?.label ?? id;

  const chosen = $derived(settings?.connectors ?? {});
  function pickConnector(assetType, id) {
    const next = { ...chosen };
    if (id) next[assetType] = id;
    else delete next[assetType];
    patch({ connectors: next });
  }
  // Connectors granted to the journal that actually serve an asset type.
  const servers = (assetType) =>
    connectors.filter((c) => (c.asset_types ?? []).includes(assetType));

  // ── Contract symbols ───────────────────────────────────────────────────────
  // A stock is its own symbol everywhere; a future is not. The journal never guesses the
  // contract, so these rows are the trader saying it once, the way their source spells it.
  const symbols = $derived(settings?.symbols ?? {});
  const symKey = (i) => `${i.asset_class}:${i.ticker}`;
  const mappable = $derived(
    (coverage?.instruments ?? []).filter((i) => i.status === 'needs_symbol' || symbols[symKey(i)])
  );

  function openSymbols(instrument) {
    draft = Object.fromEntries(
      mappable.map((i) => [symKey(i), symbols[symKey(i)] ?? i.ticker])
    );
    if (instrument) draft[symKey(instrument)] ??= symbols[symKey(instrument)] ?? instrument.ticker;
    showSymbols = true;
  }

  async function saveSymbols() {
    // The patch replaces the whole map, so what is already stored elsewhere rides along:
    // an empty box clears its entry, which the server treats as "not stated".
    await patch({ symbols: { ...symbols, ...draft } });
    if (!error) showSymbols = false;
  }

  // A download that failed says why, and for a contract that is the answer the user
  // needs: IB names the listings it found when a symbol matches several.
  const failed = $derived(jobs.filter((j) => j.status === 'error' && j.error));

  const progress = $derived.by(() => {
    if (jobs.length === 0) return null;
    const done = jobs.filter((j) => !['queued', 'running', 'waiting', 'cancelling'].includes(j.status));
    return { done: done.length, total: jobs.length };
  });
</script>

<section class="bar">
  <div class="head">
    <span class="title">
      <Icon name="candlestick" size={13} />
      {$t('journal.market.title')}
    </span>

    {#if coverage}
      <span class="summary">
        {$t('journal.market.summary', {
          measured: coverage.measured,
          measurable: coverage.measurable,
          timeframe: coverage.timeframe
        })}
        {#if coverage.timeframe_auto}
          <span class="cur">{$t('journal.market.auto')}</span>
        {/if}
      </span>
      {#if coverage.missing_windows > 0}
        <Badge tone="warning">
          {$t('journal.market.missingBadge', {
            instruments: coverage.missing_instruments,
            windows: coverage.missing_windows
          })}
        </Badge>
      {:else if coverage.measurable > 0}
        <Badge tone="success">{$t('journal.market.coveredBadge')}</Badge>
      {/if}
      {#if coverage.needs_symbol > 0}
        <button class="link warn" onclick={() => openSymbols()}>
          <Icon name="alert-triangle" size={12} />
          {$t('journal.market.needsSymbolBadge', { count: coverage.needs_symbol })}
        </button>
      {/if}
      {#if coverage.unsupported > 0}
        <Badge tone="neutral">
          {$t('journal.market.unsupportedBadge', { count: coverage.unsupported })}
        </Badge>
      {/if}
    {:else}
      <span class="summary cur">{$t('journal.market.intro')}</span>
    {/if}

    <span class="spacer"></span>

    <Button size="sm" icon="search" loading={busy === 'discover'} onclick={discover}>
      {$t('journal.market.discover')}
    </Button>
    {#if coverage && coverage.missing_windows > 0}
      <Button
        size="sm"
        variant="primary"
        icon="download"
        loading={busy === 'sync'}
        disabled={settings?.sync_mode === 'off'}
        onclick={download}
      >
        {$t('journal.market.download')}
      </Button>
    {/if}
    {#if coverage && coverage.ready > 0}
      <Button size="sm" icon="bar-chart" loading={busy === 'compute'} onclick={() => measure()}>
        {$t('journal.market.measure')}
      </Button>
      {#if coverage.measured > 0}
        <Button
          size="sm"
          variant="ghost"
          icon="refresh-cw"
          loading={busy === 'remeasure'}
          onclick={() => measure(true)}
        >
          {$t('journal.market.remeasure')}
        </Button>
      {/if}
    {/if}
    {#if coverage}
      <button class="link" onclick={() => (showDetail = !showDetail)}>
        {showDetail ? $t('journal.market.hideDetail') : $t('journal.market.showDetail')}
      </button>
    {/if}
    <button class="link" onclick={() => (showSettings = true)}>
      <Icon name="settings" size={12} />
      {$t('journal.market.settings')}
    </button>
  </div>

  {#if progress}
    <div class="progress">
      <Icon name="download" size={12} />
      {$t('journal.market.jobProgress', { done: progress.done, total: progress.total })}
      <span class="track"><span class="fill" style="width:{(progress.done / progress.total) * 100}%"></span></span>
    </div>
  {/if}

  {#each failed as j (j.id)}
    <p class="msg err">
      <Icon name="alert-triangle" size={12} />
      <span><span class="sym">{j.ticker}</span>{j.error}</span>
      <button class="link" onclick={() => openSymbols()}>{$t('journal.market.mapSymbol')}</button>
    </p>
  {/each}

  {#if error}
    <p class="msg err"><Icon name="alert-triangle" size={12} /> {error}</p>
  {:else if notice}
    <p class="msg"><Icon name="check-circle" size={12} /> {notice}</p>
  {/if}

  {#if showDetail && coverage}
    <table class="tbl">
      <thead>
        <tr>
          <th>{$t('journal.market.col.instrument')}</th>
          <th class="num">{$t('journal.market.col.trades')}</th>
          <th class="num">{$t('journal.market.col.measured')}</th>
          <th>{$t('journal.market.col.stored')}</th>
          <th>{$t('journal.market.col.missing')}</th>
          <th>{$t('journal.market.col.source')}</th>
          <th>{$t('journal.market.col.status')}</th>
        </tr>
      </thead>
      <tbody>
        {#each coverage.instruments as i (i.ticker + i.asset_class)}
          <tr>
            <td>
              <span class="sym">{i.ticker}</span>
              <span class="cur">{assetLabel(i.asset_class)}</span>
              {#if i.symbol && i.symbol !== i.ticker}
                <button class="link" onclick={() => openSymbols(i)}>→ {i.symbol}</button>
              {/if}
            </td>
            <td class="num">{i.trades}</td>
            <td class="num">{i.measured}</td>
            <td class="range">
              {#if i.bar_count > 0}
                {fmtDateTime(i.stored_from)} → {fmtDateTime(i.stored_to)}
                <span class="cur">{$t('journal.market.bars', { count: i.bar_count })}</span>
              {:else}
                <span class="cur">{$t('journal.market.noBars')}</span>
              {/if}
            </td>
            <td class="range">
              {#if i.missing.length === 0}
                <span class="cur">—</span>
              {:else}
                {#each i.missing as w (w.from)}
                  <span class="win">{fmtDateTime(w.from)} → {fmtDateTime(w.to)}</span>
                {/each}
              {/if}
            </td>
            <td>
              {#if i.connector_name}
                {i.connector_name}
              {:else if i.status === 'needs_symbol'}
                <button class="link" onclick={() => openSymbols(i)}>
                  {$t('journal.market.mapSymbol')}
                </button>
              {:else}
                <span class="cur">{i.reason || '—'}</span>
              {/if}
            </td>
            <td>
              <Badge tone={STATUS_TONE[i.status] ?? 'neutral'}>
                {$t(`journal.market.status.${i.status}`)}
              </Badge>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<Modal bind:open={showSettings} title={$t('journal.market.settingsTitle')} size="md">
  {#if settings}
    <div class="form">
      <p class="hint">{$t('journal.market.settingsHint')}</p>

      <label class="field">
        <span class="lbl">{$t('journal.market.timeframe')}</span>
        <select
          value={settings.timeframe}
          onchange={(e) => patch({ timeframe: e.currentTarget.value })}
        >
          {#each MARKET_TIMEFRAMES as tf (tf)}
            <option value={tf}>
              {tf === 'auto' ? $t('journal.market.timeframeAuto') : tf}
            </option>
          {/each}
        </select>
        <span class="hint">{$t('journal.market.timeframeHint')}</span>
      </label>

      <label class="field">
        <span class="lbl">{$t('journal.market.syncMode')}</span>
        <select
          value={settings.sync_mode}
          onchange={(e) => patch({ sync_mode: e.currentTarget.value })}
        >
          {#each MARKET_SYNC_MODES as m (m)}
            <option value={m}>{$t(`journal.market.syncMode.${m}`)}</option>
          {/each}
        </select>
        <span class="hint">{$t(`journal.market.syncModeHint.${settings.sync_mode}`)}</span>
      </label>

      <div class="field">
        <span class="lbl">{$t('journal.market.sources')}</span>
        <span class="hint">{$t('journal.market.sourcesHint')}</span>
        <div class="sources">
          {#each MARKET_ASSET_TYPES as a (a.id)}
            {@const list = servers(a.id)}
            <div class="src">
              <span class="k">{$t(`journal.market.assetType.${a.id}`)}</span>
              {#if list.length === 0}
                <span class="cur">{$t('journal.market.noConnector')}</span>
              {:else}
                <select
                  value={chosen[a.id] ?? ''}
                  onchange={(e) => pickConnector(a.id, e.currentTarget.value)}
                >
                  <option value="">{$t('journal.market.connectorAuto')}</option>
                  {#each list as c (c.id)}
                    <option value={c.id}>{c.name}</option>
                  {/each}
                </select>
              {/if}
            </div>
          {/each}
        </div>
        <div class="grant">
          <ConnectorButton module="journal" size="sm" onchanged={load} />
        </div>
      </div>

      {#if error}<p class="msg err">{error}</p>{/if}
    </div>
  {/if}
</Modal>

<Modal bind:open={showSymbols} title={$t('journal.market.symbolsTitle')} size="md">
  <div class="form">
    <p class="hint">
      {$t('journal.market.symbolsHint')}
      <!-- The symbol typed here is the data provider's, so the shapes it accepts sit
           one click away. -->
      <SymbolHelp />
    </p>
    {#if mappable.length === 0}
      <p class="hint">{$t('journal.market.symbolsNone')}</p>
    {:else}
      <div class="syms">
        {#each mappable as i (symKey(i))}
          <label class="field">
            <span class="lbl">
              {i.ticker}
              <span class="cur">{assetLabel(i.asset_class)} · {$t('journal.market.symbolsTrades', { count: i.trades })}</span>
            </span>
            <input
              value={draft[symKey(i)] ?? ''}
              placeholder={i.ticker}
              oninput={(e) => (draft = { ...draft, [symKey(i)]: e.currentTarget.value })}
            />
          </label>
        {/each}
      </div>
      <p class="hint">{$t('journal.market.symbolsExamples')}</p>
    {/if}
    {#if error}<p class="msg err">{error}</p>{/if}
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (showSymbols = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" disabled={mappable.length === 0} onclick={saveSymbols}>
      {$t('common.save')}
    </Button>
  {/snippet}
</Modal>

<style>
  .bar {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border: 0.5px solid var(--border);
    background: var(--surface);
    padding: var(--space-3) var(--space-4);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .title {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.03em;
  }
  .summary {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .spacer {
    flex: 1;
  }
  .cur {
    color: var(--dim);
    font-size: 11px;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--accent);
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .track {
    flex: 1;
    height: 3px;
    background: var(--surface-2);
    max-width: 240px;
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .msg {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .msg.err {
    color: var(--red);
  }
  .sym {
    font-family: var(--mono);
    margin-right: var(--space-2);
  }
  .range {
    font-size: 11px;
    color: var(--muted);
  }
  .win {
    display: block;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lbl {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .hint {
    font-size: 11px;
    color: var(--faint);
    line-height: var(--lh-base);
  }
  .sources {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--space-3);
    margin-top: var(--space-2);
  }
  .src {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .src .k {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .grant {
    margin-top: var(--space-3);
  }
  .link.warn {
    color: var(--amber);
  }
  .syms {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .syms .lbl {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    text-transform: none;
    letter-spacing: 0;
    font-family: var(--mono);
    color: var(--text);
  }
  .syms input {
    font-family: var(--mono);
  }
</style>
