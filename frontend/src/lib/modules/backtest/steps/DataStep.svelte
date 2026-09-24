<script>
  // Step 1 — what the strategy runs on. One dataset = a single-asset run, 2–8 = a portfolio
  // run. The preview appears either way: a period (window, bars, grain) for one dataset, the
  // full alignment (shared clock, warm-up, inactive bars) for a portfolio.
  import MultiDatasetSelect from '../MultiDatasetSelect.svelte';
  import AlignmentBanner from '../AlignmentBanner.svelte';
  import { t } from '$lib/i18n';
  import './form.css';

  let {
    datasets = [],
    datasetIds = $bindable([]),
    alignment = null,
    aligning = false,
    max = 8
  } = $props();

  const multi = $derived(datasetIds.length > 1);
  const any = $derived(datasetIds.length > 0);
  // A single-asset engine (grid) caps the picker at one, and says so instead of leaving an
  // "up to 8" lead above a control that refuses the second pick.
  const single = $derived(max <= 1);
</script>

<div class="bt-step">
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.step.dataCap')}</span>
    <p class="bt-note">{$t(single ? 'backtest.step.dataLeadSingle' : 'backtest.step.dataLead')}</p>
    <MultiDatasetSelect {datasets} bind:values={datasetIds} {max} />
  </div>

  {#if any}
    <div class="bt-block">
      <span class="bt-cap">{$t(multi ? 'backtest.step.alignCap' : 'backtest.step.rangeCap')}</span>
      <AlignmentBanner {alignment} loading={aligning} />
    </div>
  {/if}
</div>
