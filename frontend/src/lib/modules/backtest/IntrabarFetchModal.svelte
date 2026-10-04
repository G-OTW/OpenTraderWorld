<script>
  // Download the lower-timeframe candles a run needs, then hand back the run they settle.
  // Opens on the estimate the plain run returned: what is missing, the requests and the time it
  // costs at the provider's pace, its published limit and the quota left. A short download
  // starts on its own (`autostart`); a long one waits for the user. The run itself happens on
  // the server, which downloads each bar's candles when the simulation reaches it.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { t } from '$lib/i18n';
  import { backtestApi, INTRABAR_LONG_SECS } from './api.js';

  let {
    open = $bindable(false),
    estimate = null,
    autostart = false,
    /** () => Promise<string>: starts the job, returns its id. */
    start,
    /** (result) => void: the finished run. */
    onresult
  } = $props();

  let jobId = $state(null);
  let status = $state(null);
  let error = $state('');
  let timer = null;

  const running = $derived(!!jobId && !status?.done);
  const total = $derived((estimate?.missing_bars ?? 0) + (estimate?.missing_quotes ?? 0));
  const pct = $derived(total ? Math.min(100, Math.round(((status?.fetched ?? 0) / total) * 100)) : 0);
  const long = $derived((estimate?.seconds ?? 0) > INTRABAR_LONG_SECS);

  function human(secs) {
    if (secs < 90) return `${Math.max(1, Math.round(secs))} s`;
    if (secs < 5400) return `${Math.round(secs / 60)} min`;
    return `${(secs / 3600).toFixed(1)} h`;
  }

  async function go() {
    error = '';
    status = null;
    try {
      jobId = await start();
      poll();
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }

  function poll() {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      if (!jobId) return;
      try {
        status = await backtestApi.intrabarStatus(jobId);
      } catch (e) {
        error = e?.message ?? String(e);
        jobId = null;
        return;
      }
      if (!status.done) return poll();
      const id = jobId;
      jobId = null;
      if (status.error) {
        error = status.error;
        return;
      }
      if (status.result) {
        open = false;
        onresult?.(status.result, id);
      }
    }, 600);
  }

  async function cancel() {
    if (running) {
      await backtestApi.cancelIntrabar(jobId).catch(() => {});
    } else {
      open = false;
    }
  }

  // Opening on a short estimate starts at once; reopening resets what the last job left.
  $effect(() => {
    if (!open) {
      clearTimeout(timer);
      return;
    }
    if (autostart && !jobId && !status) go();
  });
  $effect(() => () => clearTimeout(timer));
  $effect(() => {
    if (!open) {
      status = null;
      error = '';
    }
  });

  const quotaLine = (q) =>
    q?.max != null
      ? $t('backtest.intrabar.quota', { used: q.used, max: q.max, period: q.period, reset: new Date(q.resets_at * 1000).toLocaleString() })
      : $t('backtest.intrabar.noQuota');
</script>

<Modal bind:open title={$t('backtest.intrabar.title')} size="md" dismissible={!running}>
  {#if estimate}
    <p>
      {$t('backtest.intrabar.lead2', { requests: estimate.requests, time: human(estimate.seconds) })}
      {#if estimate.missing_bars}{$t('backtest.intrabar.leadBars', { n: estimate.missing_bars })}{/if}
      {#if estimate.missing_quotes}{$t('backtest.intrabar.leadQuotes', { n: estimate.missing_quotes })}{/if}
    </p>
    <table class="rows">
      <thead>
        <tr>
          <th>{$t('backtest.intrabar.ticker')}</th>
          <th>{$t('backtest.intrabar.provider')}</th>
          <th>{$t('backtest.intrabar.kind')}</th>
          <th>{$t('backtest.intrabar.timeframe')}</th>
          <th class="num">{$t('backtest.intrabar.bars')}</th>
          <th class="num">{$t('backtest.intrabar.requests')}</th>
        </tr>
      </thead>
      <tbody>
        {#each estimate.providers ?? [] as p (p.kind + p.ticker + p.provider)}
          <tr>
            <td>{p.ticker}</td>
            <td>{p.provider}</td>
            <td>{p.kind === 'quotes' ? $t('backtest.intrabar.kindQuotes') : $t('backtest.intrabar.kindLower')}</td>
            <td>{p.timeframe}</td>
            <td class="num">{p.bars}</td>
            <td class="num">{p.requests}</td>
          </tr>
          <tr class="sub">
            <td colspan="6">
              {#if p.rate_limit}<span>{$t('backtest.intrabar.rateLimit', { text: p.rate_limit })}</span>{/if}
              <span>{quotaLine(p.quota)}</span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if long}
      <p class="warn">{$t('backtest.intrabar.long', { time: human(estimate.seconds) })}</p>
    {/if}
    <p class="note">{$t('backtest.intrabar.mayFail')}</p>
    <p class="note">{$t('backtest.intrabar.cached')}</p>
  {/if}

  {#if jobId || status}
    <div class="bar" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100">
      <span style="width: {pct}%"></span>
    </div>
    <p class="note">
      {$t('backtest.intrabar.progress', { done: status?.fetched ?? 0, total, requests: status?.requests ?? 0 })}
      {#if status?.failed}· {$t('backtest.intrabar.failed', { n: status.failed })}{/if}
    </p>
    {#if status?.last_error}<p class="warn">{status.last_error}</p>{/if}
  {/if}
  {#if error}<p class="warn">{error}</p>{/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={cancel}>
      {running ? $t('backtest.intrabar.stop') : $t('common.cancel')}
    </Button>
    {#if !running}
      <Button variant="primary" onclick={go}>{$t('backtest.intrabar.start')}</Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .rows {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    margin: var(--space-2) 0;
  }
  .rows th {
    text-align: left;
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: var(--fw-medium);
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
  }
  .rows td {
    padding: var(--space-1) var(--space-2);
  }
  .rows .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .rows tr.sub td {
    font-size: var(--text-xs);
    color: var(--muted);
    border-bottom: var(--hairline) solid var(--border);
  }
  .rows tr.sub span + span::before {
    content: ' · ';
  }
  .note {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .warn {
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .bar {
    height: 6px;
    background: var(--surface-2);
    border-radius: var(--radius);
    overflow: hidden;
    margin-top: var(--space-2);
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
</style>
