<script>
  // Instrument picker, as a modal. Same content as the old left panel's Data tab — recents,
  // what is already stored, and a live symbol search over every connector granted to the
  // chart — but opened from the symbol on the chart, the way market tools do it, so the
  // chart keeps the whole width.
  import Modal from '$lib/ui/Modal.svelte';
  import DataTab from './DataTab.svelte';
  import ImportModal from '../histdata/ImportModal.svelte';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    datasets = [],
    recents = [],
    selected = null,
    busy = false,
    onselect,
    ondownload,
    /** Offer to open a file of bars here (the main symbol picker, not the compare one). */
    importable = false,
    /** A file was kept in Historical Data: the catalog changed. */
    onimported
  } = $props();

  let importOpen = $state(false);

  let wrap = $state(null);

  // Focus the search box on open: you came here to type a symbol, and it also puts the focus
  // inside the dialog so Escape closes it instead of reaching the chart behind.
  $effect(() => {
    if (!open) return;
    requestAnimationFrame(() => wrap?.querySelector('input')?.focus());
  });

  function pick(coords) {
    open = false;
    onselect?.(coords);
  }
</script>

<Modal bind:open title={$t('histviz.data.pick')} size="lg">
  <div class="wrap" bind:this={wrap}>
    <DataTab
      {datasets}
      {recents}
      {selected}
      {busy}
      onselect={pick}
      ondownload={() => {
        open = false;
        ondownload?.();
      }}
      onimport={importable ? () => (importOpen = true) : null}
    />
  </div>
</Modal>

{#if importable}
  <ImportModal bind:open={importOpen} {datasets} {onimported} onchart={pick} />
{/if}

<style>
  /* The tab was written for a narrow column that owns its height; in the modal it gets the
     body's fixed height and scrolls inside it. */
  .wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 320px;
  }
</style>
