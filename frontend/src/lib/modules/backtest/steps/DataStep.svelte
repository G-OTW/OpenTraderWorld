<script>
  // Step 1 — what the strategy runs on. One dataset = a single-asset run, 2–8 = a portfolio
  // run. The preview appears either way: a period (window, bars, grain) for one dataset, the
  // full alignment (shared clock, warm-up, inactive bars) for a portfolio.
  import MultiDatasetSelect from '../MultiDatasetSelect.svelte';
  import AlignmentBanner from '../AlignmentBanner.svelte';
  import { t } from '$lib/i18n';
  import './form.css';

  let { datasets = [], datasetIds = $bindable([]), alignment = null, aligning = false } = $props();

  const multi = $derived(datasetIds.length > 1);
  const any = $derived(datasetIds.length > 0);
</script>

<div class="bt-step">
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.step.dataCap')}</span>
    <p class="bt-note">{$t('backtest.step.dataLead')}</p>
    <MultiDatasetSelect {datasets} bind:values={datasetIds} />
  </div>

  {#if any}
    <div class="bt-block">
      <span class="bt-cap">{$t(multi ? 'backtest.step.alignCap' : 'backtest.step.rangeCap')}</span>
      <AlignmentBanner {alignment} loading={aligning} />
    </div>
  {/if}
</div>
