<script>
  // Step 1 — what the strategy runs on. One dataset = a single-asset run, 2–8 = a portfolio
  // run. The preview appears either way: a period (window, bars, grain) for one dataset, the
  // full alignment (shared clock, warm-up, inactive bars) for a portfolio.
  import MultiDatasetSelect from '../MultiDatasetSelect.svelte';
  import AlignmentBanner from '../AlignmentBanner.svelte';
  import DatePicker from '$lib/ui/DatePicker.svelte';
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import './form.css';

  let {
    datasets = [],
    datasetIds = $bindable([]),
    period = $bindable({ from: '', to: '' }),
    alignment = null,
    aligning = false,
    max = 8
  } = $props();

  const multi = $derived(datasetIds.length > 1);
  const any = $derived(datasetIds.length > 0);
  // A single-asset engine (grid) caps the picker at one, and says so instead of leaving an
  // "up to 8" lead above a control that refuses the second pick.
  const single = $derived(max <= 1);

  // The pickers span the union of the selected datasets: a date only one of them covers is still
  // a valid bound, the others are simply clipped to what they hold.
  const picked = $derived(datasetIds.map((id) => datasets.find((d) => d.id === id)).filter(Boolean));
  const day = (ts) => (ts ? String(ts).slice(0, 10) : '');
  const minDay = $derived(picked.map((d) => day(d.range_from)).filter(Boolean).sort()[0] ?? '');
  const maxDay = $derived(picked.map((d) => day(d.range_to)).filter(Boolean).sort().at(-1) ?? '');

  // Until the user picks a bound, it follows the data: first and last available day of the
  // selection. A bound still equal to the last default (or empty) is not a user choice, so a
  // selection change moves it; any other value is the user's and stays.
  let autoFrom = '';
  let autoTo = '';
  $effect(() => {
    const lo = minDay;
    const hi = maxDay;
    untrack(() => {
      if (!period.from || period.from === autoFrom) period.from = lo;
      if (!period.to || period.to === autoTo) period.to = hi;
    });
    autoFrom = lo;
    autoTo = hi;
  });
</script>

<div class="bt-step">
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.step.dataCap')}</span>
    <p class="bt-note">{$t(single ? 'backtest.step.dataLeadSingle' : 'backtest.step.dataLead')}</p>
    <MultiDatasetSelect {datasets} bind:values={datasetIds} {max} />
  </div>

  {#if any}
    <div class="bt-block">
      <span class="bt-cap">{$t('backtest.step.windowCap')}</span>
      <p class="bt-note">{$t('backtest.step.windowLead')}</p>
      <div class="bt-grid win">
        <div class="bt-field">
          {$t('backtest.step.windowFrom')}
          <DatePicker
            bind:value={period.from}
            min={minDay}
            max={period.to || maxDay}
            placeholder={$t('backtest.step.windowStart')}
            ariaLabel={$t('backtest.step.windowFrom')}
          />
        </div>
        <div class="bt-field">
          {$t('backtest.step.windowTo')}
          <DatePicker
            bind:value={period.to}
            min={period.from || minDay}
            max={maxDay}
            placeholder={$t('backtest.step.windowEnd')}
            ariaLabel={$t('backtest.step.windowTo')}
          />
        </div>
      </div>
    </div>

    <div class="bt-block">
      <span class="bt-cap">{$t(multi ? 'backtest.step.alignCap' : 'backtest.step.rangeCap')}</span>
      <AlignmentBanner {alignment} loading={aligning} />
    </div>
  {/if}
</div>

<style>
  .win {
    max-width: 420px;
  }
</style>
