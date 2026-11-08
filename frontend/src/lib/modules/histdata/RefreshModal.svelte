<script>
  // Scheduled refresh of one dataset: how often its trailing window is pulled again, and how
  // many bars that window holds. An imported dataset also says where its file lives (a URL,
  // or a path under the server's import folder); the mapping its last import used is kept by
  // the server. Refusals (a push source, an import with nothing to re-read) come back from
  // the server with the reason, shown as is.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { histdataApi, IMPORT_PROVIDER } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), dataset = null, onsaved } = $props();

  const EVERY = [
    { value: '', key: 'off' },
    { value: '60', key: 'm1' },
    { value: '300', key: 'm5' },
    { value: '900', key: 'm15' },
    { value: '3600', key: 'h1' },
    { value: '86400', key: 'd1' }
  ];
  const everyOpts = $derived(EVERY.map((e) => ({ value: e.value, label: $t(`histdata.refresh.every.${e.key}`) })));

  let every = $state('');
  let windowBars = $state(200);
  let sourceKind = $state('url');
  let sourceUrl = $state('');
  let sourcePath = $state('');
  let error = $state('');
  let saving = $state(false);

  const imported = $derived(dataset?.provider === IMPORT_PROVIDER);

  $effect(() => {
    if (!open || !dataset) return;
    const secs = dataset.refresh_every_secs;
    every = secs != null ? String(secs) : '';
    windowBars = dataset.refresh_window_bars ?? 200;
    sourceUrl = dataset.import_source_url ?? '';
    sourcePath = dataset.import_source_path ?? '';
    sourceKind = sourcePath && !sourceUrl ? 'path' : 'url';
    error = '';
  });

  async function save() {
    if (!dataset || saving) return;
    saving = true;
    error = '';
    try {
      await histdataApi.setRefresh(dataset.id, {
        every_secs: every ? Number(every) : null,
        window_bars: Number(windowBars) || 200,
        source_url: imported && sourceKind === 'url' ? sourceUrl.trim() || null : null,
        source_path: imported && sourceKind === 'path' ? sourcePath.trim() || null : null
      });
      open = false;
      onsaved?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  const title = $derived(
    dataset ? $t('histdata.refresh.title', { ticker: dataset.label || dataset.ticker, timeframe: dataset.timeframe }) : ''
  );
</script>

<Modal bind:open {title} size="sm">
  <div class="form">
    <label class="row">
      <span class="lbl">{$t('histdata.refresh.everyLabel')}</span>
      <Dropdown bind:value={every} options={everyOpts} ariaLabel={$t('histdata.refresh.everyLabel')} />
    </label>
    <label class="row">
      <span class="lbl">{$t('histdata.refresh.windowLabel')}</span>
      <input type="number" min="1" max="10000" bind:value={windowBars} />
    </label>
    <p class="hint">{$t('histdata.refresh.windowHint')}</p>
    {#if imported}
      <div class="row">
        <span class="lbl">{$t('histdata.refresh.sourceLabel')}</span>
        <Dropdown
          bind:value={sourceKind}
          options={[
            { value: 'url', label: $t('histdata.refresh.sourceUrl') },
            { value: 'path', label: $t('histdata.refresh.sourcePath') }
          ]}
          ariaLabel={$t('histdata.refresh.sourceLabel')}
        />
      </div>
      {#if sourceKind === 'url'}
        <input class="wide" placeholder="https://example.com/bars.csv" bind:value={sourceUrl} />
      {:else}
        <input class="wide" placeholder="broker/bars.csv" bind:value={sourcePath} />
        <p class="hint">{$t('histdata.refresh.pathHint')}</p>
      {/if}
    {/if}
    {#if dataset?.last_refresh_at}
      <p class="hint">{$t('histdata.refresh.lastAt', { at: new Date(dataset.last_refresh_at).toLocaleString() })}</p>
    {/if}
    {#if dataset?.last_refresh_error}
      <p class="err">{dataset.last_refresh_error}</p>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>

  {#snippet footer()}
    <Button onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .row {
    display: grid;
    grid-template-columns: 140px 1fr;
    align-items: center;
    gap: var(--space-3);
  }
  .lbl {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .row input {
    width: 100%;
  }
  .wide {
    width: 100%;
    text-transform: none;
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-xs);
    margin: 0;
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
