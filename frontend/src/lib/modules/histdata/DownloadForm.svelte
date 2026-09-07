<script>
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import UsagePie from './UsagePie.svelte';
  // Download form: pick a connector → asset types → timeframes (greyed by capability matrix),
  // enter ticker(s) + date range, queue the jobs. No client-side ticker validation — a bad
  // ticker surfaces as a job error (the server's validation policy).
  // Batch: several timeframes and several comma-separated tickers queue one job per pair,
  // all on the same connector and the same window (the server fans them out).
  import { histdataApi } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // Yahoo's free chart API caps intraday history per interval (older data isn't served
  // at all — chunking can't bypass it). Days of lookback from today; daily/weekly = ∞.
  const YAHOO_LIMITS = { '1m': 7, '5m': 60, '15m': 60, '30m': 60, '1h': 730 };

  let { connectors = [], onqueued, onconfigure } = $props();

  let connectorId = $state('');
  let assetType = $state('');
  let selTfs = $state([]); // one job per selected timeframe
  let ticker = $state('');
  // Option contract builder (shown when assetType === 'option'). We collect the parts every
  // broker asks for and compose the bare OCC symbol (UND + YYMMDD + C/P + strike*1000); each
  // connector adds its own vendor prefix (Massive → O:, Alpaca → none).
  let optUnderlying = $state('');
  let optExpiry = $state('');
  let optType = $state('C');
  const optTypeOpts = $derived([
    { value: 'C', label: $t('histdata.download.optCall') },
    { value: 'P', label: $t('histdata.download.optPut') }
  ]);
  let optStrike = $state('');
  let fromDate = $state('');
  let fromTime = $state('');
  let toDate = $state('');
  let toTime = $state('');
  let error = $state('');
  let busy = $state(false);
  let showLimits = $state(false); // Yahoo range-limit warning modal
  let showBatchInfo = $state(false); // (i) popover: how multi-timeframe / multi-ticker works
  let infoRoot = $state(null);

  const conn = $derived(connectors.find((c) => c.id === connectorId) ?? null);
  const assetTypes = $derived(conn?.asset_types ?? []);
  const timeframes = $derived(conn?.timeframes ?? []);
  const assetTypeOpts = $derived(assetTypes.map((a) => ({ value: a, label: a })));
  const timeframeOpts = $derived(timeframes.map((tf) => ({ value: tf, label: tf })));
  // A connector is ready to use if keyless or all required credentials are set.
  const isReady = (c) => c.required_secrets.every((s) => c.set_secrets.includes(s));
  // Missing required credentials block the download.
  const missingSecrets = $derived(
    conn ? conn.required_secrets.filter((s) => !conn.set_secrets.includes(s)) : []
  );

  // ── Connector picker (searchable dropdown, one row per connector) ──
  let pickerOpen = $state(false);
  let pickerQuery = $state('');
  let pickerRoot = $state(null);
  let pickerInput = $state(null);
  const pickerFiltered = $derived.by(() => {
    const q = pickerQuery.trim().toLowerCase();
    if (!q) return connectors;
    return connectors.filter(
      (c) => c.name.toLowerCase().includes(q) || c.label.toLowerCase().includes(q)
    );
  });
  function openPicker() {
    pickerOpen = true;
    pickerQuery = '';
    queueMicrotask(() => pickerInput?.focus());
  }
  function pickConnector(c) {
    connectorId = c.id;
    pickerOpen = false;
  }
  function onPickerDocClick(e) {
    if (pickerRoot && !pickerRoot.contains(e.target)) pickerOpen = false;
    if (infoRoot && !infoRoot.contains(e.target)) showBatchInfo = false;
  }
  function quotaText(c) {
    if (!c.quota) return '';
    const period = $t(`common.period.${c.quota.period}`);
    return c.quota.max_requests == null
      ? `${c.quota.used} / ∞ · ${period}`
      : `${c.quota.used} / ${c.quota.max_requests} · ${period}`;
  }

  const isOption = $derived(assetType === 'option');
  const isFuture = $derived(assetType === 'future');

  // Compose the bare OCC option symbol from the builder fields, or '' if incomplete/invalid.
  // e.g. SPY + 2025-12-19 + Call + 650 → SPY251219C00650000 (strike ×1000, 8 digits).
  const occSymbol = $derived.by(() => {
    if (!isOption) return '';
    const und = optUnderlying.trim().toUpperCase();
    const strike = Number(optStrike);
    if (!/^[A-Z0-9]{1,6}$/.test(und) || !optExpiry || !(strike > 0)) return '';
    const [y, m, d] = optExpiry.split('-');
    if (!y || !m || !d) return '';
    const yymmdd = `${y.slice(2)}${m}${d}`;
    const strike8 = String(Math.round(strike * 1000)).padStart(8, '0');
    if (strike8.length !== 8) return ''; // strike too large to encode
    return `${und}${yymmdd}${optType}${strike8}`;
  });

  // The symbols actually submitted: the composed OCC contract for options (one at a time,
  // the builder describes a single contract), else the comma-separated list.
  const effectiveTickers = $derived.by(() => {
    if (isOption) return occSymbol ? [occSymbol] : [];
    const seen = new Set();
    return ticker
      .split(',')
      .map((s) => s.trim())
      .filter((s) => s && !seen.has(s) && seen.add(s));
  });

  // Reset dependent fields when the connector changes / selection becomes invalid.
  $effect(() => {
    if (conn && !assetTypes.includes(assetType)) assetType = assetTypes[0] ?? '';
  });
  // Drop timeframes the new connector doesn't serve; never leave the selection empty.
  $effect(() => {
    if (!conn) return;
    const kept = selTfs.filter((tf) => timeframes.includes(tf));
    if (kept.length !== selTfs.length) selTfs = kept;
    if (kept.length === 0 && timeframes.length) selTfs = [timeframes[0]];
  });

  // Date is required; time is optional and defaults to midnight.
  function localToRfc(date, time) {
    if (!date) return '';
    return new Date(`${date}T${time || '00:00'}`).toISOString();
  }
  // The end of the window. An empty "To" means now: asking for everything up to today is
  // the common case, and typing today's date is not what makes it explicit.
  function endRfc() {
    return toDate ? localToRfc(toDate, toTime) : new Date().toISOString();
  }

  // ── What the batch will cost ──
  // The server prices the request (chunks per pair, the connector's spacing, the quota
  // left) without queueing anything, so the count is the worker's own arithmetic rather
  // than a guess made here. Debounced: the form is edited character by character.
  let estimate = $state(null);

  const estimateKey = $derived(
    [connectorId, assetType, selTfs.join('|'), effectiveTickers.join('|'), fromDate, fromTime, toDate, toTime].join('~')
  );
  $effect(() => {
    void estimateKey;
    if (!connectorId || !assetType || !selTfs.length || !effectiveTickers.length || !fromDate) {
      estimate = null;
      return;
    }
    const payload = {
      connector_id: connectorId,
      asset_type: assetType,
      ticker: effectiveTickers[0],
      tickers: effectiveTickers.slice(1),
      timeframe: selTfs[0],
      timeframes: selTfs.slice(1),
      from: localToRfc(fromDate, fromTime),
      to: endRfc()
    };
    let cancelled = false;
    const id = setTimeout(() => {
      histdataApi
        .estimateDownload(payload)
        .then((e) => {
          if (!cancelled) estimate = e ?? null;
        })
        .catch(() => {
          if (!cancelled) estimate = null; // a bad combination surfaces on submit, not here
        });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(id);
    };
  });

  /** "3 min" / "2 h 10" — the floor imposed by the provider's pacing. */
  function human(secs) {
    if (secs < 60) return `${Math.round(secs)} s`;
    if (secs < 5400) return `${Math.round(secs / 60)} min`;
    return `${Math.floor(secs / 3600)} h ${Math.round((secs % 3600) / 60)}`;
  }

  // Each selected timeframe carries its own intraday limit, so a batch is checked per
  // timeframe: [{ tf, days }] for the ones whose window starts before what Yahoo serves.
  const outside = $derived.by(() => {
    if (conn?.provider !== 'yahoo' || !fromDate) return [];
    const earliest = new Date(localToRfc(fromDate, fromTime));
    return selTfs
      .map((tf) => ({ tf, days: YAHOO_LIMITS[tf] ?? null }))
      .filter((o) => o.days && earliest < new Date(Date.now() - o.days * 86400000));
  });

  async function submit() {
    error = '';
    if (!connectorId || !assetType || !selTfs.length || !effectiveTickers.length || !fromDate) {
      error = isOption ? $t('histdata.download.errOptionFields') : $t('histdata.download.errFillAll');
      return;
    }
    if (outside.length) {
      showLimits = true; // inform, don't queue jobs that will fail
      return;
    }
    busy = true;
    try {
      // Singular field + the rest as arrays: the server unions them, and a lone pair keeps
      // the exact payload the endpoint has always accepted.
      await histdataApi.startDownload({
        connector_id: connectorId,
        asset_type: assetType,
        ticker: effectiveTickers[0],
        tickers: effectiveTickers.slice(1),
        timeframe: selTfs[0],
        timeframes: selTfs.slice(1),
        from: localToRfc(fromDate, fromTime),
        to: endRfc()
      });
      ticker = '';
      optUnderlying = '';
      optStrike = '';
      onqueued?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<svelte:document onclick={onPickerDocClick} />

<form class="dl" onsubmit={(e) => (e.preventDefault(), submit())}>
  <div class="row">
    <div class="field">
      <span class="lbl">{$t('histdata.download.connector')}</span>
      <div class="picker" bind:this={pickerRoot}>
        <button
          type="button"
          class="trigger"
          onclick={() => (pickerOpen ? (pickerOpen = false) : openPicker())}
        >
          {#if conn}
            {#if conn.quota && conn.quota.max_requests != null}
              <UsagePie used={conn.quota.used} max={conn.quota.max_requests} title={quotaText(conn)} />
            {:else}
              <span class="dot" class:ok={isReady(conn)}></span>
            {/if}
            <span class="cname">{conn.name}</span>
            <span class="cprov">{conn.label}</span>
          {:else}
            <span class="cname placeholder">{$t('histdata.download.selectConnector')}</span>
          {/if}
          <span class="caret"><Icon name="chevron-down" size={13} /></span>
        </button>
        {#if pickerOpen}
          <div class="menu">
            <input
              class="menu-search"
              bind:this={pickerInput}
              bind:value={pickerQuery}
              placeholder={$t('histdata.download.searchConnector')}
              aria-label={$t('histdata.download.searchConnector')}
            />
            <ul class="opts">
              {#each pickerFiltered as c (c.id)}
                <li>
                  <button
                    type="button"
                    class="opt"
                    class:sel={c.id === connectorId}
                    onclick={() => pickConnector(c)}
                  >
                    {#if c.quota && c.quota.max_requests != null}
                      <UsagePie used={c.quota.used} max={c.quota.max_requests} title={quotaText(c)} />
                    {:else}
                      <span class="dot" class:ok={isReady(c)}></span>
                    {/if}
                    <span class="oname">{c.name}</span>
                    <span class="ometa">
                      {c.label}
                      {#if c.quota}
                        · {quotaText(c)}
                      {/if}
                    </span>
                  </button>
                </li>
              {/each}
              {#if pickerFiltered.length === 0}
                <li class="none">{$t('common.noMatches')}</li>
              {/if}
            </ul>
          </div>
        {/if}
      </div>
      {#if conn}
        <span class="ready" class:ok={isReady(conn)}>
          {isReady(conn) ? `🟢 ${$t('histdata.download.readyToUse')}` : `🔴 ${$t('histdata.download.needsCredentials')}`}
        </span>
      {/if}
    </div>
    <!-- Dropdown is a button, not a labellable control, so these are .field/span like the
         connector picker above rather than a wrapping <label>. -->
    <div class="field sel">
      <span class="lbl">{$t('histdata.download.assetType')}</span>
      <Dropdown
        bind:value={assetType}
        options={assetTypeOpts}
        disabled={!conn}
        ariaLabel={$t('histdata.download.assetType')}
      />
    </div>
    <div class="field sel">
      <span class="lbl">
        {$t('histdata.download.timeframe')}
        <!-- One (i) for the whole batch behaviour: several timeframes here, several tickers
             below, one job per pair. -->
        <span class="infowrap" bind:this={infoRoot}>
          <button
            type="button"
            class="infobtn"
            class:on={showBatchInfo}
            onclick={() => (showBatchInfo = !showBatchInfo)}
            aria-label={$t('histdata.download.batchInfo.aria')}
            aria-expanded={showBatchInfo}
          >
            <Icon name="info" size={13} />
          </button>
          {#if showBatchInfo}
            <div class="infopop" role="dialog" aria-label={$t('histdata.download.batchInfo.aria')}>
              <p class="ititle">{$t('histdata.download.batchInfo.title')}</p>
              <p>{$t('histdata.download.batchInfo.timeframes')}</p>
              <p>{$t('histdata.download.batchInfo.tickers')}</p>
              <p>{$t('histdata.download.batchInfo.pairs')}</p>
              <p>{$t('histdata.download.batchInfo.queue')}</p>
            </div>
          {/if}
        </span>
      </span>
      <MultiSelect
        bind:value={selTfs}
        options={timeframeOpts}
        allLabel={$t('histdata.download.selectPlaceholder')}
        disabled={!conn}
        width="100%"
      />
    </div>
  </div>

  <div class="row">
    {#if isOption}
      <div class="opt-block grow">
        <span class="optlbl">{$t('histdata.download.optContract')}</span>
        <div class="optfields">
          <label class="oa">
            {$t('histdata.download.optUnderlying')}
            <input bind:value={optUnderlying} placeholder="AAPL" maxlength="6" />
          </label>
          <label class="oa">
            {$t('histdata.download.optExpiry')}
            <input type="date" bind:value={optExpiry} />
          </label>
          <div class="oa field">
            <span class="lbl">{$t('histdata.download.optType')}</span>
            <Dropdown
              bind:value={optType}
              options={optTypeOpts}
              ariaLabel={$t('histdata.download.optType')}
            />
          </div>
          <label class="oa">
            {$t('histdata.download.optStrike')}
            <input type="number" min="0" step="any" bind:value={optStrike} placeholder="650" />
          </label>
        </div>
        <span class="occ" class:ok={occSymbol}>
          {occSymbol ? occSymbol : $t('histdata.download.optIncomplete')}
        </span>
      </div>
    {:else}
      <label class="grow">
        {$t('histdata.download.ticker')}
        <input
          bind:value={ticker}
          placeholder={isFuture ? $t('histdata.download.futurePlaceholder') : $t('histdata.download.tickerPlaceholder')}
        />
        <span class="hint">{$t('histdata.download.tickerMultiHint')}</span>
        {#if isFuture}<span class="hint">{$t('histdata.download.futureHint')}</span>{/if}
      </label>
    {/if}
    <label>
      {$t('histdata.download.from')}
      <span class="dt">
        <input type="date" bind:value={fromDate} />
        <input type="time" bind:value={fromTime} title={$t('histdata.download.timeOptionalTitle')} />
      </span>
    </label>
    <label>
      {$t('histdata.download.to')}
      <span class="dt">
        <input type="date" bind:value={toDate} />
        <input type="time" bind:value={toTime} title={$t('histdata.download.timeOptionalTitle')} />
      </span>
    </label>
  </div>

  {#if missingSecrets.length}
    <p class="warn">
      {$t('histdata.download.missingSecrets', { label: conn.label, secrets: missingSecrets.join(', ') })}
      <!-- Sends the user to the Settings tab, where the vault hint explains the rest. -->
      <button type="button" class="linkbtn" onclick={() => onconfigure?.()}>
        {$t('histdata.download.openProviders')}
      </button>
    </p>
  {/if}
  <!-- One line per offending timeframe: the limit is not the same for 1m and 1h. -->
  {#each outside as o (o.tf)}
    <p class="warn">
      {$t('histdata.download.outsideLimit', { timeframe: o.tf, days: o.days })}
      <button type="button" class="linkbtn" onclick={() => (showLimits = true)}>
        {$t('histdata.download.seeLimits')}
      </button>
    </p>
  {/each}
  <ErrorText error={error} copyable />

  <!-- What this queues, before it is queued: jobs, provider requests, the floor the
       pacing imposes, and whether the connector's quota can absorb it. -->
  {#if estimate}
    <p class="cost" class:over={estimate.over_quota}>
      <Icon name={estimate.over_quota ? 'alert-triangle' : 'info'} size={13} />
      <span>
        {$t('histdata.download.costLine', {
          jobs: estimate.jobs,
          requests: estimate.requests,
          time: human(estimate.seconds)
        })}
        {#if estimate.quota_max != null}
          {$t('histdata.download.costQuota', {
            used: estimate.quota_used,
            max: estimate.quota_max,
            period: $t(`common.period.${estimate.quota_period}`)
          })}
        {/if}
        {#if estimate.over_quota}
          {$t('histdata.download.costOverQuota')}
        {/if}
      </span>
    </p>
  {/if}

  <div class="actions">
    <button type="submit" disabled={busy || missingSecrets.length}>
      {busy ? $t('histdata.download.queuing') : $t('histdata.download.downloadBtn')}
    </button>
  </div>
</form>

<Modal bind:open={showLimits} title={$t('histdata.download.limitsModalTitle')} size="sm">
  <p class="modal-intro">
    {@html $t('histdata.download.limitsModalIntro')}
  </p>
  <table class="limits">
    <thead><tr><th>{$t('histdata.download.timeframe')}</th><th>{$t('histdata.download.maxLookback')}</th></tr></thead>
    <tbody>
      <tr class:hi={selTfs.includes('1m')}><td>1m</td><td>{$t('histdata.download.lookback7d')}</td></tr>
      <tr class:hi={selTfs.includes('5m')}><td>5m</td><td>{$t('histdata.download.lookback60d')}</td></tr>
      <tr class:hi={selTfs.includes('15m')}><td>15m</td><td>{$t('histdata.download.lookback60d')}</td></tr>
      <tr class:hi={selTfs.includes('30m')}><td>30m</td><td>{$t('histdata.download.lookback60d')}</td></tr>
      <tr class:hi={selTfs.includes('1h')}><td>1h</td><td>{$t('histdata.download.lookback730d')}</td></tr>
      <tr class:hi={selTfs.includes('1d') || selTfs.includes('1w')}><td>1d / 1w</td><td>{$t('histdata.download.noLimit')}</td></tr>
    </tbody>
  </table>
</Modal>

<style>
  .dl {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  label,
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-base);
    color: var(--muted);
  }
  .grow {
    flex: 1;
    min-width: 180px;
  }
  /* Asset type / timeframe: a fixed width, so the row keeps its shape as the
     connector swaps one option set for another. */
  .field.sel {
    width: 140px;
  }
  /* The label carries the (i), so it is a flex row rather than plain text. */
  .lbl {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  /* ── Batch (i) popover ── */
  .infowrap {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  .infobtn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    padding: 0;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .infobtn:hover,
  .infobtn.on {
    color: var(--text);
  }
  .infopop {
    position: absolute;
    top: calc(100% + var(--space-1));
    left: 0;
    z-index: var(--z-dropdown);
    width: 320px;
    max-width: min(320px, calc(100vw - var(--space-4)));
    padding: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    font-size: var(--text-sm);
    color: var(--muted);
    text-align: left;
    white-space: normal;
    line-height: 1.5;
    cursor: default;
  }
  .infopop p {
    margin: 0 0 var(--space-2);
  }
  .infopop p:last-child {
    margin-bottom: 0;
  }
  .infopop .ititle {
    color: var(--text);
    font-weight: var(--fw-medium);
    margin-bottom: var(--space-1);
  }

  /* ── Connector picker ── */
  .picker {
    position: relative;
  }
  .trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    min-width: 240px;
    max-width: 320px;
    padding: var(--space-2) var(--space-3);
    font: inherit;
    cursor: pointer;
    transition: border-color 0.12s ease;
  }
  .trigger:hover {
    border-color: var(--border-control);
  }
  .cname {
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cname.placeholder {
    color: var(--muted);
    font-weight: var(--fw-normal);
  }
  .cprov {
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .caret {
    margin-left: auto;
    color: var(--muted);
    display: inline-flex;
  }
  .menu {
    position: absolute;
    z-index: var(--z-dropdown);
    top: calc(100% + 4px);
    left: 0;
    min-width: 300px;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-2);
  }
  .menu-search {
    width: 100%;
    margin-bottom: var(--space-2);
  }
  .opts {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 260px;
    overflow-y: auto;
  }
  .opt {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-base);
  }
  .opt:hover {
    background: var(--surface-2);
  }
  .opt.sel .oname {
    color: var(--text);
  }
  .oname {
    font-weight: var(--fw-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ometa {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .none {
    color: var(--muted);
    font-size: var(--text-base);
    padding: var(--space-2) var(--space-3);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--red);
    flex: none;
  }
  .dot.ok {
    background: var(--green);
  }

  .opt-block {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .optlbl {
    font-size: var(--text-base);
    color: var(--muted);
  }
  .optfields {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .oa {
    flex: 1;
    min-width: 90px;
  }
  .oa input {
    width: 100%;
  }
  .occ {
    font-family: ui-monospace, monospace;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .occ.ok {
    color: var(--green);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .dt {
    display: flex;
    gap: var(--space-1);
  }
  .dt input[type='time'] {
    width: 7em;
  }
  .ready {
    font-size: var(--text-xs);
    color: var(--red);
  }
  .ready.ok {
    color: var(--green);
  }
  .warn {
    color: var(--amber);
    font-size: var(--text-base);
  }
  /* Cost preview: quiet by default, amber once the batch outruns the quota. */
  .cost {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }
  .cost.over {
    color: var(--amber);
  }
  .cost :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .linkbtn {
    background: none;
    border: none;
    padding: 0;
    color: var(--muted);
    text-decoration: underline;
    cursor: pointer;
    font-size: inherit;
  }
  .modal-intro {
    font-size: var(--text-base);
    color: var(--muted);
    margin: 0 0 var(--space-3);
    line-height: 1.5;
  }
  .limits {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-base);
  }
  .limits th,
  .limits td {
    text-align: left;
    padding: var(--space-2);
    border-bottom: 1px solid var(--border);
  }
  .limits th {
    color: var(--muted);
    font-weight: var(--fw-medium);
  }
  .limits tr.hi td {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .actions button {
    background: transparent;
    color: var(--text);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .actions button:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .actions button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
