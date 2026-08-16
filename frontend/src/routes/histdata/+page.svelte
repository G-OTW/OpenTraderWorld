<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Historical Data module — two tabs: Download (form + live job progress) and Datasets
  // (management). Provider accounts are not a tab here: they belong to the shared data
  // broker, one click away in the header (same modal from every data module). Jobs poll
  // while the Download tab is open so progress bars move.
  import { onDestroy } from 'svelte';
  import { histdataApi } from '$lib/modules/histdata/api.js';
  import DownloadForm from '$lib/modules/histdata/DownloadForm.svelte';
  import JobList from '$lib/modules/histdata/JobList.svelte';
  import DatasetManager from '$lib/modules/histdata/DatasetManager.svelte';
  import ConnectorsModal from '$lib/connectors/ConnectorsModal.svelte';
  import { connectorsApi } from '$lib/connectors/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // Active tab persists across refresh (per-browser), like the journal/findb pages.
  const PREFS_KEY = 'otw.histdata.prefs.v1';
  const TABS = ['download', 'datasets'];
  let prefsLoaded = $state(false);

  function loadTab() {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (TABS.includes(p.tab)) return p.tab;
    } catch {
      /* corrupt prefs — ignore */
    }
    return 'download';
  }

  let tab = $state(loadTab());
  $effect(() => {
    prefsLoaded = true;
  });
  $effect(() => {
    const snap = { tab };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snap));
    } catch {
      /* quota / unavailable — non-fatal */
    }
  });

  let connectors = $state([]);
  let brokerOpen = $state(false);
  let jobs = $state([]);
  let datasets = $state([]);
  let error = $state('');

  async function loadConnectors() {
    connectors = await connectorsApi.list('histdata');
  }
  async function loadJobs() {
    jobs = await histdataApi.jobs();
  }
  async function loadDatasets() {
    datasets = await histdataApi.datasets();
  }

  $effect(() => {
    loadConnectors().catch((e) => (error = e.message));
    loadJobs().catch(() => {});
    loadDatasets().catch(() => {});
  });

  // Poll jobs while any are active (and the download tab is visible). Connectors ride
  // along so the quota usage pies advance while the worker downloads.
  let timer;
  $effect(() => {
    clearInterval(timer);
    const active = jobs.some((j) => j.status === 'queued' || j.status === 'running');
    if (tab === 'download' && active) {
      timer = setInterval(() => {
        loadJobs().catch(() => {});
        loadConnectors().catch(() => {});
      }, 1500);
    }
    return () => clearInterval(timer);
  });
  onDestroy(() => clearInterval(timer));

  async function onQueued() {
    await loadJobs();
    await loadDatasets();
  }
</script>

<div class="page">
  <header>
    <h1>{$t('histdata.page.title')}</h1>
    <nav>
      <button class:active={tab === 'download'} onclick={() => (tab = 'download')}>{$t('histdata.page.tabDownload')}</button>
      <button class:active={tab === 'datasets'} onclick={() => (tab = 'datasets')}>{$t('histdata.page.tabDatasets')}</button>
      <button class="broker" onclick={() => (brokerOpen = true)} title={$t('connectors.manageTitle')}>
        <Icon name="plug" size={13} /> {$t('connectors.manage')}
      </button>
    </nav>
  </header>

  <ErrorText error={error} copyable />

  <div class="content">
    {#if tab === 'download'}
      <section class="card">
        <h2>{$t('histdata.page.newDownload')}</h2>
        <DownloadForm {connectors} onqueued={onQueued} onconfigure={() => (brokerOpen = true)} />
      </section>
      <section class="card">
        <h2>{$t('histdata.page.jobs')}</h2>
        <JobList {jobs} />
      </section>
    {:else}
      <section class="card">
        <div class="card-head">
          <h2>{$t('histdata.page.storedDatasets')}</h2>
          <button class="reload" onclick={() => loadDatasets().catch((e) => (error = e.message))}>
            <Icon name="refresh-cw" size={13} /> {$t('histdata.page.reload')}
          </button>
        </div>
        <DatasetManager {datasets} onchanged={onQueued} />
      </section>
    {/if}
  </div>
</div>

<ConnectorsModal bind:open={brokerOpen} module="histdata" onchanged={loadConnectors} />

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-6);
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
  }
  nav {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
  }
  nav button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    color: var(--muted);
    cursor: pointer;
  }
  nav button.active {
    color: var(--text);
    border-color: var(--accent);
  }
  /* The broker shortcut is an action, not a tab: no active state, icon + label. */
  nav button.broker {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: var(--space-2);
    color: var(--text);
  }
  nav button.broker:hover {
    border-color: var(--border-control);
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .reload {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
</style>
