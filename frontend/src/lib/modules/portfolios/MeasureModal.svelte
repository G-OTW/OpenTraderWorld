<script>
  // Everything that has to be true before the measures can answer.
  //
  // Three verbs in the order they matter, the same order the journal's enrichment uses:
  // coverage reads what is needed and writes nothing, sync queues ordinary downloads,
  // rebuild walks the ledger and writes the curve. Plus the two settings the risk and
  // benchmark blocks read.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { fmtDate } from '$lib/format.js';
  import { CURRENCIES, portfoliosApi } from './api.js';

  let { portfolio, onclose, onchanged } = $props();

  let coverage = $state(null);
  let loading = $state(true);
  let busy = $state('');
  let error = $state(null);

  let benchmark = $state(portfolio?.benchmark?.symbol ?? '');
  let riskFree = $state(((portfolio?.risk_free ?? 0) * 100).toFixed(2));
  // Per-asset edits, applied on blur so a half-typed ticker is never saved.
  let edits = $state({});

  async function load() {
    loading = true;
    try {
      coverage = await portfoliosApi.coverage(portfolio.id);
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (portfolio?.id) load();
  });

  async function saveAsset(a) {
    const patch = edits[a.asset_id];
    if (!patch) return;
    try {
      await portfoliosApi.updateAsset(a.asset_id, patch);
      edits = { ...edits, [a.asset_id]: undefined };
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function saveSettings() {
    busy = 'settings';
    error = null;
    try {
      await portfoliosApi.update(portfolio.id, {
        benchmark: benchmark.trim() ? { asset_type: 'equity', symbol: benchmark.trim() } : null,
        risk_free: (Number(riskFree) || 0) / 100
      });
      onchanged?.();
      toast.ok($t('portfolios.measure.saved'));
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function sync() {
    busy = 'sync';
    error = null;
    try {
      const r = await portfoliosApi.syncHistory(portfolio.id);
      if (r.queued > 0) toast.ok($t('portfolios.measure.queued', { n: r.queued }));
      else if (r.needs_symbol.length) toast.warn($t('portfolios.measure.needSymbols'));
      else if (!r.no_connector.length) toast.ok($t('portfolios.measure.nothingToQueue'));
      // No connector at all and no connector that carries this instrument are two different
      // problems with two different fixes, so they are two different sentences.
      if (r.no_connector.length) {
        toast.warn(
          r.connectors
            ? $t('portfolios.measure.brokerCannotServe', { names: r.no_connector.join(', ') })
            : $t('portfolios.measure.noConnector')
        );
      }
      await load();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function rebuild() {
    busy = 'rebuild';
    error = null;
    try {
      const r = await portfoliosApi.rebuildHistory(portfolio.id, null);
      toast.ok($t('portfolios.measure.rebuilt', { n: r.days }));
      if (r.incomplete_days > 0) toast.warn($t('portfolios.measure.incomplete', { n: r.incomplete_days }));
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  const needsSymbol = $derived(coverage?.needs_symbol ?? 0);
  const noConnector = $derived(coverage?.no_connector ?? 0);
  const failed = $derived(coverage?.failed ?? 0);
  const unserved = $derived(
    coverage?.assets?.filter((a) => a.status === 'no_connector').map((a) => a.symbol) ?? []
  );
  const stale = $derived(portfolio?.rebuild_from ?? null);
</script>

<Modal open title={$t('portfolios.measure.title')} size="lg" {onclose}>
  <section>
    <h4>{$t('portfolios.measure.settings')}</h4>
    <div class="row">
      <Field label={$t('portfolios.measure.benchmark')} hint={$t('portfolios.measure.benchmarkHint')}>
        <input type="text" bind:value={benchmark} placeholder="SPY" />
      </Field>
      <Field label={$t('portfolios.measure.riskFree')} hint={$t('portfolios.measure.riskFreeHint')}>
        <input type="number" step="any" bind:value={riskFree} />
      </Field>
      <button class="btn" onclick={saveSettings} disabled={busy === 'settings'}>
        {$t('common.save')}
      </button>
    </div>
  </section>

  <section>
    <div class="head">
      <h4>{$t('portfolios.measure.history')}</h4>
      <div class="acts">
        <button class="btn sm" onclick={sync} disabled={!!busy || needsSymbol > 0}>
          <Icon name="download" size={13} /> {$t('portfolios.measure.sync')}
        </button>
        <button class="btn sm primary" onclick={rebuild} disabled={!!busy}>
          {busy === 'rebuild' ? $t('portfolios.measure.rebuilding') : $t('portfolios.measure.rebuild')}
        </button>
      </div>
    </div>

    {#if stale}
      <!-- A backdated edit made every snapshot after it describe a book that never existed.
           The watermark says so; rebuilding stays the user's call, not a side effect. -->
      <p class="stale">{$t('portfolios.measure.stale', { date: fmtDate(stale) })}</p>
    {/if}
    {#if failed}
      <!-- Said on the portfolio screen, not only in the download log the user has no reason
           to open: the jobs were queued, the broker took them, and every one came back
           empty. -->
      <p class="stale">{$t('portfolios.measure.downloadsFailed', { n: failed })}</p>
    {/if}
    {#if noConnector}
      <!-- Said before Sync is pressed, not after the worker fails hours later: no granted
           connector carries these instruments, and pressing the button again cannot fix it. -->
      <p class="stale">{$t('portfolios.measure.brokerCannotServe', { names: unserved.join(', ') })}</p>
    {/if}
    {#if coverage}
      <p class="span">
        {#if coverage.curve_from}
          {$t('portfolios.measure.curve', {
            from: fmtDate(coverage.curve_from),
            to: fmtDate(coverage.curve_to),
            days: coverage.curve_days
          })}
        {:else}
          {$t('portfolios.measure.noCurve')}
        {/if}
      </p>
    {/if}

    <ErrorText {error} copyable />

    {#if loading}
      <p class="muted">{$t('common.loading')}</p>
    {:else if coverage?.assets?.length}
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('portfolios.measure.asset')}</th>
            <th>{$t('portfolios.measure.barSymbol')}</th>
            <th>{$t('portfolios.measure.barCurrency')}</th>
            <th class="r">{$t('portfolios.measure.bars')}</th>
            <th>{$t('portfolios.measure.status')}</th>
          </tr>
        </thead>
        <tbody>
          {#each coverage.assets as a (a.asset_id)}
            <tr>
              <td><b>{a.symbol}</b> <span class="muted">{$t(`portfolios.class.${a.asset_class}`)}</span></td>
              <td>
                <!-- A CoinGecko coin id is a spot coordinate, not a bar ticker. Asked once,
                     never guessed: an asset with no answer is named, not approximated. -->
                <input
                  class="sym"
                  type="text"
                  value={a.hist_symbol}
                  placeholder={a.symbol}
                  oninput={(e) =>
                    (edits = { ...edits, [a.asset_id]: { ...edits[a.asset_id], hist_symbol: e.target.value } })}
                  onblur={() => saveAsset(a)}
                />
              </td>
              <td>
                <Select
                  value={edits[a.asset_id]?.hist_currency ?? a.hist_currency ?? 'USD'}
                  options={CURRENCIES.map((c) => ({ value: c, label: c }))}
                  onpick={(v) => {
                    edits = { ...edits, [a.asset_id]: { ...edits[a.asset_id], hist_currency: v } };
                    saveAsset(a);
                  }}
                />
              </td>
              <td class="r num muted">{a.bars || '—'}</td>
              <td>
                <span class="st {a.status}">{$t(`portfolios.measure.st.${a.status}`)}</span>
                <!-- The worker's own words. A capability matrix knows which asset types a
                     provider serves and never which listings, so the only honest answer to
                     "why is there no candle" is the failure the download actually returned. -->
                {#if a.last_error}
                  <span class="st failed" title={a.last_error}>
                    {$t('portfolios.measure.st.failed', { provider: a.error_provider })}
                  </span>
                {/if}
              </td>
            </tr>
            {#if a.last_error}
              <tr class="why">
                <td colspan="5">{a.last_error}</td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="muted">{$t('portfolios.measure.noAssets')}</p>
    {/if}
  </section>

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose}>{$t('common.close')}</button>
  {/snippet}
</Modal>

<style>
  section {
    margin-bottom: var(--space-6);
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .head h4 {
    margin: 0;
  }
  .acts {
    display: flex;
    gap: var(--space-2);
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: var(--space-3);
    align-items: start;
  }
  /* The hints are two different heights, so the controls line up on their own top
     edge and the button drops by exactly one label line to join them. */
  .row .btn {
    margin-top: calc(var(--text-xs) * var(--lh-tight) + var(--space-1));
  }
  /* The asset column takes the slack; every other column is as wide as its control. */
  table.tbl th:first-child {
    width: 100%;
  }
  table.tbl th:nth-child(2),
  table.tbl td:nth-child(2) {
    width: 160px;
  }
  table.tbl th:nth-child(3),
  table.tbl td:nth-child(3) {
    width: 110px;
  }
  table.tbl th:nth-child(4),
  table.tbl td:nth-child(4) {
    width: 72px;
  }
  table.tbl th:last-child,
  table.tbl td:last-child {
    width: 116px;
    white-space: nowrap;
  }
  .sym {
    width: 100%;
  }
  .r {
    text-align: right;
  }
  .muted,
  .span {
    color: var(--muted);
    font-size: 12px;
  }
  .span {
    margin: var(--space-1) 0 var(--space-3);
  }
  .stale {
    margin: var(--space-2) 0 0;
    color: var(--amber);
    font-size: 12px;
    font-weight: 600;
  }
  .st {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
  }
  .st.ok {
    background: color-mix(in srgb, var(--green) 15%, transparent);
    color: var(--green);
  }
  .st.partial {
    background: color-mix(in srgb, var(--amber) 18%, transparent);
    color: var(--amber);
  }
  .st.missing,
  .st.no_connector,
  .st.needs_symbol,
  .st.failed {
    background: color-mix(in srgb, var(--red) 15%, transparent);
    color: var(--red);
  }
  .why td {
    padding-top: 0;
    color: var(--red);
    font-size: 11px;
    white-space: normal;
  }
</style>
