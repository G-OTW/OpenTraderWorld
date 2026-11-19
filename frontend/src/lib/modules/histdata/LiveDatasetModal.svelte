<script>
  // A live symbol turned into a stored dataset, for a page that can only run on one
  // (Backtest, Quant). The search is the chart's own picker; the window is then kept through
  // the chart's "save" path (one download job: closed bars only, upserted on the dataset key,
  // gaps recorded) and the page selects the dataset id it produced, so a run reads stored
  // bars and can be run again on the same ones. The chart itself stays preview-only.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import DataTab from '../histviz/DataTab.svelte';
  import GapCheckModal from './GapCheckModal.svelte';
  import { histdataApi } from './api.js';
  import { storeWindow, defaultFrom } from './storeWindow.js';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';

  let {
    open = $bindable(false),
    /** The timeframe the page is locked to (a basket shares one), or '' for any. */
    timeframe = '',
    /** Called with the stored dataset and the fresh catalog. */
    onstored
  } = $props();

  let hit = $state(null); // the symbol-search pick
  let tf = $state('');
  let fromDate = $state('');
  let toDate = $state('');
  let busy = $state(false);
  let job = $state(null); // the download being waited on
  let error = $state('');
  let gaps = $state(null); // { dataset, holes } once stored with holes
  let gapOpen = $state(false);
  let abort = null;

  const tfOpts = $derived((hit?.timeframes ?? []).map((v) => ({ value: v, label: v })));
  const unserved = $derived(!!hit && !!timeframe && !(hit.timeframes ?? []).includes(timeframe));

  // Each opening starts over; closing stops the wait (the job itself runs on).
  $effect(() => {
    if (open) {
      hit = null;
      job = null;
      error = '';
      gaps = null;
      busy = false;
    } else {
      abort?.abort();
      abort = null;
    }
  });

  function pick(c) {
    hit = c;
    error = '';
    const tfs = c.timeframes ?? [];
    tf = timeframe || (tfs.includes('1d') ? '1d' : (tfs[0] ?? ''));
    fromDate = defaultFrom(tf);
    toDate = '';
  }

  async function store() {
    if (!hit || !tf || !fromDate || unserved) return;
    busy = true;
    error = '';
    job = null;
    const ctl = new AbortController();
    abort = ctl;
    try {
      const from = new Date(`${fromDate}T00:00:00Z`).toISOString();
      // An empty "To" is up to now, as in the download form.
      const to = toDate ? new Date(`${toDate}T23:59:59Z`).toISOString() : new Date().toISOString();
      const r = await storeWindow({ ...hit, timeframe: tf }, { from, to }, { onjob: (j) => (job = j), signal: ctl.signal });
      onstored?.(r.dataset, r.datasets);
      // The integrity check is what the catalog says about holes: shown before closing when
      // it finds any, so a run on this window is not taken for a complete one.
      const report = await histdataApi.checkDataset(r.dataset.id).catch(() => null);
      if (report?.total_holes) gaps = { dataset: r.dataset, holes: report.total_holes, partial: r.job?.status === 'partial' ? r.job.error : '' };
      else open = false;
    } catch (e) {
      if (!ctl.signal.aborted) error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={$t('histdata.live.title')} size="md">
  <div class="live">
    <p class="hint">{$t('histdata.live.storeHint')}</p>
    {#if gaps}
      <p class="warn">
        {$t('histdata.live.gaps', { ticker: gaps.dataset.ticker, timeframe: gaps.dataset.timeframe, n: gaps.holes })}
      </p>
      {#if gaps.partial}<p class="warn">{gaps.partial}</p>{/if}
    {:else if !hit}
      <div class="search">
        <DataTab module="histviz" shelves={false} onselect={pick} />
      </div>
    {:else}
      <div class="picked">
        <span class="sym">{hit.ticker}</span>
        <span class="prov">{hit.provider} · {hit.asset_type}</span>
        <button type="button" class="linkbtn" disabled={busy} onclick={() => (hit = null)}>{$t('histdata.live.change')}</button>
      </div>
      <div class="row">
        <div class="field">
          <span class="lbl">{$t('histdata.download.timeframe')}</span>
          <Dropdown
            bind:value={tf}
            options={tfOpts}
            disabled={busy || !!timeframe}
            ariaLabel={$t('histdata.download.timeframe')}
          />
        </div>
        <label class="field">
          <span class="lbl">{$t('histdata.download.from')}</span>
          <input type="date" bind:value={fromDate} disabled={busy} />
        </label>
        <label class="field">
          <span class="lbl">{$t('histdata.download.to')}</span>
          <input type="date" bind:value={toDate} disabled={busy} title={$t('histdata.live.toNow')} />
        </label>
      </div>
      {#if unserved}
        <p class="warn">{$t('histdata.live.tfUnserved', { provider: hit.provider, timeframe })}</p>
      {/if}
      {#if busy}
        <p class="hint">
          {job
            ? $t('histdata.live.progress', { status: $t(`histdata.jobs.status.${job.status}`), bars: fmtNum(job.bars_written ?? 0, 0) })
            : $t('histdata.download.queuing')}
          {$t('histdata.live.runsOn')}
        </p>
      {/if}
    {/if}
    <ErrorText {error} copyable />
  </div>

  {#snippet footer()}
    {#if gaps}
      <Button onclick={() => (gapOpen = true)}>{$t('histdata.datasets.checkGaps')}</Button>
      <Button variant="primary" onclick={() => (open = false)}>{$t('common.close')}</Button>
    {:else}
      <Button onclick={() => (open = false)}>{$t('common.cancel')}</Button>
      <Button variant="primary" loading={busy} disabled={!hit || !tf || !fromDate || unserved} onclick={store}>
        {$t('histdata.live.store')}
      </Button>
    {/if}
  {/snippet}
</Modal>

<GapCheckModal bind:open={gapOpen} dataset={gaps?.dataset ?? null} />

<style>
  .live {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
  }
  /* DataTab scrolls its own result list inside a fixed box. */
  .search {
    display: flex;
    flex-direction: column;
    height: 320px;
    min-height: 0;
  }
  .picked {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .sym {
    font-weight: var(--fw-medium);
  }
  .prov {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 140px;
  }
  .lbl {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-xs);
    margin: 0;
    line-height: 1.5;
  }
  .warn {
    color: var(--amber);
    font-size: var(--text-sm);
    margin: 0;
    overflow-wrap: anywhere;
  }
  .linkbtn {
    margin-left: auto;
    background: none;
    border: none;
    padding: 0;
    color: var(--muted);
    text-decoration: underline;
    cursor: pointer;
    font-size: var(--text-sm);
  }
</style>
