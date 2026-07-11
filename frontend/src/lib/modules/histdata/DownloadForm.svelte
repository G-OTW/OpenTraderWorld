<script>
  import Modal from '$lib/ui/Modal.svelte';
  // Download form: pick provider → asset type → timeframe (greyed by capability matrix),
  // enter ticker + date range, queue the job. No client-side ticker validation — a bad
  // ticker surfaces as a job error (the server's validation policy).
  import { histdataApi } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // Yahoo's free chart API caps intraday history per interval (older data isn't served
  // at all — chunking can't bypass it). Days of lookback from today; daily/weekly = ∞.
  const YAHOO_LIMITS = { '1m': 7, '5m': 60, '15m': 60, '30m': 60, '1h': 730 };

  let { providers = [], onqueued } = $props();

  let provider = $state('');
  let assetType = $state('');
  let timeframe = $state('');
  let ticker = $state('');
  // Option contract builder (shown when assetType === 'option'). We collect the parts every
  // broker asks for and compose the bare OCC symbol (UND + YYMMDD + C/P + strike*1000); each
  // connector adds its own vendor prefix (Massive → O:, Alpaca → none).
  let optUnderlying = $state('');
  let optExpiry = $state('');
  let optType = $state('C');
  let optStrike = $state('');
  let fromDate = $state('');
  let fromTime = $state('');
  let toDate = $state('');
  let toTime = $state('');
  let error = $state('');
  let busy = $state(false);
  let showLimits = $state(false); // Yahoo range-limit warning modal

  const cap = $derived(providers.find((p) => p.provider === provider) ?? null);
  const assetTypes = $derived(cap?.asset_types ?? []);
  const timeframes = $derived(cap?.timeframes ?? []);
  // A provider is ready to use if keyless or all required credentials are set.
  const isReady = (p) => p.required_secrets.every((s) => p.set_secrets.includes(s));
  // Missing required credentials block the download.
  const missingSecrets = $derived(
    cap ? cap.required_secrets.filter((s) => !cap.set_secrets.includes(s)) : []
  );

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

  // The symbol actually submitted: composed OCC for options, else the free-text ticker.
  const effectiveTicker = $derived(isOption ? occSymbol : ticker.trim());

  // Reset dependent fields when provider changes / selection becomes invalid.
  $effect(() => {
    if (cap && !assetTypes.includes(assetType)) assetType = assetTypes[0] ?? '';
  });
  $effect(() => {
    if (cap && !timeframes.includes(timeframe)) timeframe = timeframes[0] ?? '';
  });

  // Date is required; time is optional and defaults to midnight.
  function localToRfc(date, time) {
    if (!date) return '';
    return new Date(`${date}T${time || '00:00'}`).toISOString();
  }

  // Yahoo intraday limit for the current provider/timeframe, in days (null = no limit).
  const yahooLimitDays = $derived(
    provider === 'yahoo' ? (YAHOO_LIMITS[timeframe] ?? null) : null
  );
  // `from` older than the limit → the older portion can't be fetched.
  const outsideLimit = $derived.by(() => {
    if (!yahooLimitDays || !fromDate) return false;
    const earliest = new Date(localToRfc(fromDate, fromTime));
    const cutoff = new Date(Date.now() - yahooLimitDays * 86400000);
    return earliest < cutoff;
  });

  async function submit() {
    error = '';
    if (!provider || !assetType || !timeframe || !effectiveTicker || !fromDate || !toDate) {
      error = isOption ? $t('histdata.download.errOptionFields') : $t('histdata.download.errFillAll');
      return;
    }
    if (outsideLimit) {
      showLimits = true; // inform, don't queue a job that will fail
      return;
    }
    busy = true;
    try {
      await histdataApi.startDownload({
        provider,
        asset_type: assetType,
        ticker: effectiveTicker,
        timeframe,
        from: localToRfc(fromDate, fromTime),
        to: localToRfc(toDate, toTime)
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

<form class="dl" onsubmit={(e) => (e.preventDefault(), submit())}>
  <div class="row">
    <label>
      {$t('histdata.download.provider')}
      <select bind:value={provider}>
        <option value="" disabled>{$t('histdata.download.selectPlaceholder')}</option>
        {#each providers as p (p.provider)}
          <option value={p.provider}>
            {isReady(p) ? '🟢' : '🔴'} {p.label}{p.paid ? ` (${$t('histdata.providers.paid')})` : ''}
          </option>
        {/each}
      </select>
      {#if cap}
        <span class="ready" class:ok={isReady(cap)}>
          {isReady(cap) ? `🟢 ${$t('histdata.download.readyToUse')}` : `🔴 ${$t('histdata.download.needsCredentials')}`}
        </span>
      {/if}
    </label>
    <label>
      {$t('histdata.download.assetType')}
      <select bind:value={assetType} disabled={!cap}>
        {#each assetTypes as a (a)}
          <option value={a}>{a}</option>
        {/each}
      </select>
    </label>
    <label>
      {$t('histdata.download.timeframe')}
      <select bind:value={timeframe} disabled={!cap}>
        {#each timeframes as tf (tf)}
          <option value={tf}>{tf}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="row">
    {#if isOption}
      <div class="opt grow">
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
          <label class="oa">
            {$t('histdata.download.optType')}
            <select bind:value={optType}>
              <option value="C">{$t('histdata.download.optCall')}</option>
              <option value="P">{$t('histdata.download.optPut')}</option>
            </select>
          </label>
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
      {$t('histdata.download.missingSecrets', { label: cap.label, secrets: missingSecrets.join(', ') })}
    </p>
  {/if}
  {#if outsideLimit}
    <p class="warn">
      {$t('histdata.download.outsideLimit', { timeframe, days: yahooLimitDays })}
      <button type="button" class="linkbtn" onclick={() => (showLimits = true)}>
        {$t('histdata.download.seeLimits')}
      </button>
    </p>
  {/if}
  <ErrorText error={error} copyable />

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
      <tr class:hi={timeframe === '1m'}><td>1m</td><td>{$t('histdata.download.lookback7d')}</td></tr>
      <tr class:hi={timeframe === '5m'}><td>5m</td><td>{$t('histdata.download.lookback60d')}</td></tr>
      <tr class:hi={timeframe === '15m'}><td>15m</td><td>{$t('histdata.download.lookback60d')}</td></tr>
      <tr class:hi={timeframe === '30m'}><td>30m</td><td>{$t('histdata.download.lookback60d')}</td></tr>
      <tr class:hi={timeframe === '1h'}><td>1h</td><td>{$t('histdata.download.lookback730d')}</td></tr>
      <tr class:hi={timeframe === '1d' || timeframe === '1w'}><td>1d / 1w</td><td>{$t('histdata.download.noLimit')}</td></tr>
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
  label {
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
  .opt {
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
  .oa input,
  .oa select {
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
  .linkbtn {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
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
    font-weight: var(--fw-semibold);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  button {
    background: var(--accent);
    color: var(--accent-contrast);
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
