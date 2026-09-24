<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Pending FX tasks. One task is a (date, currency) pair: a date is never missing as a
  // whole, a currency on it is. Each row offers what can actually resolve it, in the same
  // order the background job tries: a stored histdata series, a download that would create
  // one, then a rate typed by hand. A currency no market prices only ever gets the last.
  //
  // A currency the conventional tickers cannot reach (USDT, USDC, EURC) gets its market
  // named instead: a series already in store, or a connector plus the pair as that venue
  // writes it. The mapping is per currency, so it is asked once and serves every date.
  import { onDestroy, onMount } from 'svelte';
  import { journalApi } from './api.js';
  import { histdataApi, IMPORT_PROVIDER } from '$lib/modules/histdata/api.js';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { onchanged = () => {} } = $props();

  let pending = $state([]);
  let loading = $state(true);

  // key -> typed rate for the expanded row.
  let drafts = $state({});
  let openKey = $state(null);
  let saving = $state(false);
  let busy = $state(''); // key of the row running a series/download action
  let error = $state('');
  let notice = $state('');

  // The open market picker: { quote, loading, connectors, datasets, form, saving }.
  let picker = $state(null);

  // The queued download, followed from here: nothing on this row can resolve until it
  // lands, and the job list lives on another page. { jobId, ticker, status, done }
  let watch = $state(null);
  let poll = null;
  const LIVE = new Set(['queued', 'running', 'waiting', 'cancelling']);

  const keyOf = (p) => `${p.pending_date}|${p.quote}`;
  // A derived rate is a raw float off a close; trim it to something readable without
  // pretending to a precision the candle never had.
  const fmt = (n) => String(Number(Number(n).toFixed(8)));

  onMount(load);

  async function load() {
    loading = true;
    pending = await journalApi.fxPending();
    loading = false;
  }

  function toggle(p) {
    const k = keyOf(p);
    openKey = openKey === k ? null : k;
    error = '';
    notice = '';
    if (drafts[k] === undefined) drafts[k] = '';
  }

  // Resolving removes the row, so reload rather than patch: the same rate can clear
  // several tasks at once through the carry-forward read.
  async function after() {
    openKey = null;
    await load();
    onchanged();
  }

  async function useSeries(p) {
    error = '';
    notice = '';
    busy = keyOf(p);
    try {
      await journalApi.fxFromHistdata(p.pending_date, p.quote);
      await after();
    } catch (e) {
      error = e.message ?? $t('journal.pending.errSave');
    } finally {
      busy = '';
    }
  }

  async function download(p) {
    error = '';
    notice = '';
    busy = keyOf(p);
    try {
      const r = await journalApi.fxDownload(p.pending_date, p.quote);
      startWatch(r.job_id, r.ticker);
    } catch (e) {
      error = e.message ?? $t('journal.pending.errSave');
    } finally {
      busy = '';
    }
  }

  function stopWatch() {
    if (poll) clearInterval(poll);
    poll = null;
  }

  function startWatch(jobId, ticker) {
    stopWatch();
    watch = { jobId, ticker, status: 'queued', done: false };
    poll = setInterval(pollJob, 3000);
    pollJob();
  }

  // Polled, not pushed: the download is an ordinary histdata job and the queue has no
  // stream. A failed read is skipped, not reported — the next tick is 3s away.
  async function pollJob() {
    if (!watch) return stopWatch();
    let job;
    try {
      job = (await histdataApi.jobs()).find((j) => j.id === watch.jobId);
    } catch {
      return;
    }
    if (!job) return;
    watch.status = job.status;
    if (!LIVE.has(job.status)) {
      watch.done = true;
      stopWatch();
    }
  }

  // The reload is the user's, not automatic: the rate is read off the series only when
  // they ask for it, and a list that reshuffles under the cursor loses the open row.
  async function reloadTasks() {
    watch = null;
    error = '';
    notice = '';
    await load();
    onchanged();
  }

  onDestroy(stopWatch);

  // Which way the pair is quoted, guessed from where the currency sits in the ticker.
  // Only a default: the select stays the user's, and a ticker that names neither side
  // leaves it alone.
  function guessInvert(ticker, quote) {
    const t = String(ticker).toUpperCase().replace(/[^A-Z0-9]/g, '');
    if (t.startsWith(quote)) return true;
    if (t.endsWith(quote)) return false;
    return null;
  }

  const kinds = $derived.by(() => {
    const c = picker?.connectors?.find((x) => x.id === picker.form?.connector_id);
    return (c?.asset_types ?? ['fx', 'crypto']).map((k) => ({
      value: k,
      label: $t(`journal.market.assetType.${k}`)
    }));
  });

  async function openPicker(p) {
    error = '';
    notice = '';
    picker = { quote: p.quote, loading: true, connectors: [], datasets: [], form: null };
    try {
      const o = await journalApi.fxSourceOptions(p.quote);
      const first = o.connectors[0];
      picker = {
        quote: p.quote,
        loading: false,
        saving: false,
        connectors: o.connectors,
        datasets: o.datasets,
        form: {
          connector_id: o.source?.connector_id ?? first?.id ?? '',
          provider: o.source?.provider ?? first?.provider ?? '',
          asset_type: o.source?.asset_type ?? first?.asset_types?.[0] ?? 'crypto',
          ticker: o.source?.ticker ?? '',
          invert: o.source?.invert ?? false,
          touched: !!o.source
        }
      };
    } catch (e) {
      picker = null;
      error = e.message ?? $t('journal.pending.errSave');
    }
  }

  // Picking a stored series answers every field at once. Its provider keeps its connector
  // when one is granted, so a later download extends the same dataset instead of a twin.
  function pickDataset(d) {
    const c = picker.connectors.find((x) => x.provider === d.provider);
    const inv = guessInvert(d.ticker, picker.quote);
    picker.form = {
      connector_id: c?.id ?? '',
      provider: d.provider,
      asset_type: d.asset_type,
      ticker: d.ticker,
      invert: inv ?? picker.form.invert,
      touched: picker.form.touched
    };
  }

  function onConnector(id) {
    const c = picker.connectors.find((x) => x.id === id);
    if (!c) return;
    picker.form.provider = c.provider;
    if (!c.asset_types.includes(picker.form.asset_type)) picker.form.asset_type = c.asset_types[0];
  }

  function onTicker() {
    if (picker.form.touched) return;
    const inv = guessInvert(picker.form.ticker, picker.quote);
    if (inv !== null) picker.form.invert = inv;
  }

  async function saveSource(p) {
    const f = picker.form;
    if (!f.ticker.trim()) {
      error = $t('journal.pending.errNoPair');
      return;
    }
    error = '';
    picker.saving = true;
    try {
      const body = { asset_type: f.asset_type, ticker: f.ticker.trim(), invert: f.invert };
      if (f.connector_id) body.connector_id = f.connector_id;
      else body.provider = f.provider;
      await journalApi.fxSetSource(p.quote, body);
      picker = null;
      // Reload rather than patch: the mapping may already price this date, and then the
      // row comes back with the rate read off it.
      await load();
      notice = $t('journal.pending.marketSaved', { currency: p.quote });
    } catch (e) {
      error = e.message ?? $t('journal.pending.errSave');
      if (picker) picker.saving = false;
    }
  }

  async function clearSource(p) {
    error = '';
    notice = '';
    busy = keyOf(p);
    try {
      await journalApi.fxClearSource(p.quote);
      await load();
    } catch (e) {
      error = e.message ?? $t('journal.pending.errSave');
    } finally {
      busy = '';
    }
  }

  async function saveManual(p) {
    error = '';
    notice = '';
    const n = Number(drafts[keyOf(p)]);
    if (!Number.isFinite(n) || n <= 0) {
      error = $t('journal.pending.errNoRate');
      return;
    }
    saving = true;
    try {
      await journalApi.fxResolve(p.pending_date, { [p.quote]: n });
      delete drafts[keyOf(p)];
      await after();
    } catch (e) {
      error = e.message ?? $t('journal.pending.errSave');
    } finally {
      saving = false;
    }
  }
</script>

<div class="pending">
  <div class="head">
    <p class="intro">
      {@html $t('journal.pending.intro')}
    </p>
    {#if watch}
      <div class="watch">
        {#if watch.done}
          {#if watch.status !== 'done' && watch.status !== 'partial'}
            <span class="warn">{$t('journal.pending.downloadFailed', { ticker: watch.ticker })}</span>
          {/if}
          <Button size="sm" icon="refresh-cw" onclick={reloadTasks}>
            {$t('journal.pending.reload')}
          </Button>
        {:else}
          <span class="spinner" aria-hidden="true"></span>
          <span class="watch-label">{$t('journal.pending.downloading', { ticker: watch.ticker })}</span>
        {/if}
      </div>
    {/if}
  </div>

  {#if loading}
    <Skeleton rows={3} height="52px" />
  {:else if pending.length === 0}
    <!-- Nothing pending is the good outcome here, not a void to fill. -->
    <EmptyState icon="check-circle" description={$t('journal.pending.empty')} compact />
  {:else}
    <ul class="list">
      {#each pending as p (keyOf(p))}
        {@const k = keyOf(p)}
        <li class:open={openKey === k}>
          <button class="row" onclick={() => toggle(p)} aria-expanded={openKey === k}>
            <span class="ccy">{p.quote}</span>
            <span class="date num">{p.pending_date}</span>
            <span class="reason">{p.reason}</span>
            <span class="chev">
              <Icon name={openKey === k ? 'chevron-down' : 'chevron-right'} size={13} />
            </span>
          </button>
          {#if openKey === k}
            <div class="editor">
              <div class="market">
                {#if p.source}
                  <p class="hint">
                    {$t('journal.pending.marketSet', {
                      ticker: p.source.ticker,
                      provider: p.source.provider
                    })}
                    {#if !p.source.connector_id && p.source.provider !== IMPORT_PROVIDER}
                      <span class="warn">{$t('journal.pending.marketStale')}</span>
                    {/if}
                  </p>
                  {#if picker?.quote !== p.quote}
                    <div class="market-actions">
                      <Button size="sm" variant="ghost" onclick={() => openPicker(p)}>
                        {$t('journal.pending.marketChange')}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        loading={busy === k}
                        onclick={() => clearSource(p)}
                      >
                        {$t('journal.pending.marketClear')}
                      </Button>
                    </div>
                  {/if}
                {:else if picker?.quote !== p.quote}
                  <Button size="sm" icon="link" onclick={() => openPicker(p)}>
                    {$t('journal.pending.marketName')}
                  </Button>
                {/if}

                {#if picker?.quote === p.quote}
                  {#if picker.loading}
                    <Skeleton rows={2} height="34px" />
                  {:else}
                    <p class="hint">{$t('journal.pending.marketHint', { currency: p.quote })}</p>
                    {#if picker.datasets.length}
                      <div class="stored">
                        {#each picker.datasets as d (d.provider + d.ticker)}
                          <button
                            type="button"
                            class="chip"
                            class:on={picker.form.ticker === d.ticker &&
                              picker.form.provider === d.provider}
                            onclick={() => pickDataset(d)}
                          >
                            <span class="chip-ticker">{d.ticker}</span>
                            <span class="chip-sub">
                              {d.provider} · {d.range_from ?? '?'} → {d.range_to ?? '?'}
                            </span>
                          </button>
                        {/each}
                      </div>
                    {/if}
                    <div class="fields">
                      {#if picker.connectors.length}
                        <Select
                          label={$t('journal.pending.marketConnector')}
                          bind:value={picker.form.connector_id}
                          onpick={(v) => onConnector(v)}
                          options={picker.connectors.map((c) => ({
                            value: c.id,
                            label: `${c.name} · ${c.label}`
                          }))}
                        />
                      {/if}
                      <Select
                        label={$t('journal.pending.marketKind')}
                        bind:value={picker.form.asset_type}
                        options={kinds}
                      />
                      <Input
                        label={$t('journal.pending.marketPair')}
                        bind:value={picker.form.ticker}
                        oninput={onTicker}
                        placeholder="USDTUSD"
                      />
                      <Select
                        label={$t('journal.pending.marketDirection')}
                        bind:value={picker.form.invert}
                        onpick={() => (picker.form.touched = true)}
                        hint={$t('journal.pending.marketPeg')}
                        options={[
                          {
                            value: false,
                            label: $t('journal.pending.marketDirPerUsd', { currency: p.quote })
                          },
                          {
                            value: true,
                            label: $t('journal.pending.marketDirUsdPer', { currency: p.quote })
                          }
                        ]}
                      />
                    </div>
                    {#if !picker.connectors.length && !picker.datasets.length}
                      <p class="hint">{$t('journal.pending.marketNone', { currency: p.quote })}</p>
                    {/if}
                    <div class="market-actions">
                      <Button size="sm" variant="ghost" onclick={() => (picker = null)}>
                        {$t('common.cancel')}
                      </Button>
                      <Button
                        size="sm"
                        variant="primary"
                        loading={picker.saving}
                        onclick={() => saveSource(p)}
                      >
                        {$t('journal.pending.marketSave')}
                      </Button>
                    </div>
                  {/if}
                {/if}
              </div>

              {#if p.histdata_ticker}
                <!-- A real market already stored: the rate is read, not typed. -->
                <div class="route">
                  <div class="route-head">
                    <span class="label">{$t('journal.pending.fromSeries', { ticker: p.histdata_ticker })}</span>
                    <span class="num quoted">1 USD = {fmt(p.histdata_rate)} {p.quote}</span>
                  </div>
                  {#if p.histdata_proxy}
                    <p class="hint">{$t('journal.pending.proxyNote')}</p>
                  {/if}
                  <Button
                    variant="primary"
                    size="sm"
                    loading={busy === k}
                    onclick={() => useSeries(p)}
                  >
                    {$t('journal.pending.useSeries')}
                  </Button>
                </div>
              {:else if p.can_download}
                <div class="route">
                  <p class="hint">{$t('journal.pending.downloadHint', { currency: p.quote })}</p>
                  <Button size="sm" icon="download" loading={busy === k} onclick={() => download(p)}>
                    {$t('journal.pending.download')}
                  </Button>
                </div>
              {:else}
                <p class="hint">{$t('journal.pending.noMarket', { currency: p.quote })}</p>
              {/if}

              <label class="rate">
                <span>{$t('journal.pending.usdTo', { currency: p.quote })}</span>
                <input type="number" step="any" placeholder="0.0" bind:value={drafts[k]} />
              </label>

              {#if notice}<p class="notice">{notice}</p>{/if}
              <ErrorText error={error} copyable />
              <div class="actions">
                <Button variant="ghost" onclick={() => (openKey = null)} disabled={saving}>
                  {$t('common.cancel')}
                </Button>
                <Button variant="primary" loading={saving} onclick={() => saveManual(p)}>
                  {$t('journal.pending.saveRates')}
                </Button>
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .pending {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 720px;
  }
  /* The download watcher sits top right, opposite the intro. */
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .intro {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .watch {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    white-space: nowrap;
  }
  .watch-label {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  /* Same spinner as Button's, at label size: it is the page saying "still running". */
  .spinner {
    width: 13px;
    height: 13px;
    flex: none;
    border: 2px solid var(--muted);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 600ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .list li {
    border: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
    border-radius: 0;
    overflow: hidden;
  }
  .list li.open {
    border-left-color: var(--accent);
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--surface);
    border: none;
    color: var(--text);
    padding: var(--space-3) var(--space-4);
    cursor: pointer;
    text-align: left;
  }
  .row:hover {
    background: var(--surface-2);
  }
  /* The currency is what the task is about, so it leads the row. */
  .ccy {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    min-width: 4ch;
  }
  /* .num (theme/default.css) already gives it tabular figures. */
  .date {
    font-family: var(--mono);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .reason {
    color: var(--muted);
    font-size: var(--text-sm);
    flex: 1;
  }
  .chev {
    color: var(--muted);
    display: inline-flex;
  }
  .editor {
    padding: var(--space-4);
    background: var(--surface-2);
    border-top: 0.5px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .route {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
  }
  /* Naming the market comes before the three routes: it is what makes them work. */
  .market {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
    border-bottom: 0.5px solid var(--border);
  }
  .market-actions {
    display: flex;
    gap: var(--space-2);
  }
  .warn {
    color: var(--amber);
  }
  .stored {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  /* One stored series = one control, so the chip owns the only frame. */
  .chip {
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
  }
  .chip:hover {
    border-color: var(--accent);
  }
  .chip.on {
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .chip-ticker {
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  .chip-sub {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
    width: 100%;
  }
  .route-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .label {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .quoted {
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: var(--lh-base);
  }
  .notice {
    color: var(--green);
    font-size: var(--text-xs);
  }
  .rate {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 220px;
  }
  .rate > span {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
