<script>
  // Edits one signal operand inline as part of a condition sentence. A single grouped
  // select covers all kinds — indicators (grouped), price fields, or a custom value —
  // with the indicator params / const value as compact inputs right after it.
  // Bound via $bindable so the parent's settings object updates directly.
  import { INDICATOR_GROUPS, PRICE_FIELDS, indicatorById } from '$lib/indicators/dag.js';
  import IndicatorPicker from '$lib/ui/IndicatorPicker.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { METRICS, POSITION_FIELDS, metricHasPeriod } from './api.js';
  import { pickablePlots, defaultPlot } from './embed.js';
  import { t } from '$lib/i18n';

  // `customIndicators` (expert mode) = [{ id, name, kind, meta }] saved custom indicators that
  // can be referenced as an operand; a script row (`kind` "script") carries its compiled
  // `meta`, whose plots the operand picks from. Empty in normal mode, so the Custom optgroup
  // is hidden.
  // `positions` adds the live-portfolio group, which only a DCA run can answer.
  // `float`: pin the pickers to the viewport (inside a modal, whose body scrolls and clips).
  let { operand = $bindable(), customIndicators = [], positions = false, float = false } = $props();

  const selValue = $derived(
    operand.kind === 'price'
      ? `price:${operand.field}`
      : operand.kind === 'const'
        ? 'const'
        : operand.kind === 'custom_indicator'
          ? `custom:${operand.id}`
          : operand.kind === 'metric'
            ? `metric:${operand.metric}`
            : operand.kind === 'position'
              ? `pos:${operand.field}`
              : `ind:${operand.indicator}`
  );

  // Switching selection reseeds the operand, keeping shared params / the const value.
  function setSel(v) {
    if (v === 'const') {
      operand = { kind: 'const', value: operand.kind === 'const' ? operand.value : 0 };
    } else if (v.startsWith('price:')) {
      operand = { kind: 'price', field: v.slice(6) };
    } else if (v.startsWith('custom:')) {
      const row = customIndicators.find((ci) => ci.id === v.slice(7));
      operand =
        row?.kind === 'script'
          ? { kind: 'custom_indicator', id: row.id, plot: defaultPlot(row.meta) }
          : { kind: 'custom_indicator', id: v.slice(7) };
    } else if (v.startsWith('metric:')) {
      const metric = v.slice(7);
      operand = { kind: 'metric', metric, period: metricHasPeriod(metric) ? (operand.period ?? 20) : 0 };
    } else if (v.startsWith('pos:')) {
      operand = { kind: 'position', field: v.slice(4) };
    } else {
      const id = v.slice(4);
      const def = indicatorById(id);
      const next = { kind: 'indicator', indicator: id };
      for (const p of def?.params ?? []) next[p.key] = operand[p.key] ?? p.def;
      operand = next;
    }
  }

  const indDef = $derived(operand.kind === 'indicator' ? indicatorById(operand.indicator) : null);

  // A script operand reads one plot by title. A plot the engine would refuse stays listed,
  // dimmed, with the reason: a strategy should not be built on it.
  const scriptRow = $derived(
    operand.kind === 'custom_indicator' ? customIndicators.find((ci) => ci.id === operand.id && ci.kind === 'script') : null
  );
  const plotOptions = $derived(
    pickablePlots(scriptRow?.meta).map((p) => ({
      value: p.title,
      disabled: p.disabled,
      label: p.disabled ? `${p.title} (${reasonText(p)})` : p.title
    }))
  );
  function reasonText(p) {
    if (p.reason === 'duplicate') return $t('backtest.script.duplicate');
    return p.args ? $t('backtest.script.noncausal', p.args) : $t('backtest.script.noncausalPlain');
  }

  // Grouped, searchable options: indicators (by group) + custom + price fields + the fixed value.
  const pickerGroups = $derived([
    ...INDICATOR_GROUPS.map((g) => ({
      label: g.label,
      items: g.items.map((i) => ({ value: `ind:${i.id}`, label: i.label }))
    })),
    ...(customIndicators.length
      ? [{ label: $t('backtest.operand.custom'), items: customIndicators.map((ci) => ({ value: `custom:${ci.id}`, label: ci.name })) }]
      : []),
    { label: $t('backtest.operand.price'), items: PRICE_FIELDS.map((f) => ({ value: `price:${f}`, label: f })) },
    {
      label: $t('backtest.operand.metric'),
      items: METRICS.map((m) => ({ value: `metric:${m.id}`, label: $t(`backtest.metric.${m.id}`) }))
    },
    ...(positions
      ? [
          {
            label: $t('backtest.operand.position'),
            items: POSITION_FIELDS.map((f) => ({ value: `pos:${f}`, label: $t(`backtest.posField.${f}`) }))
          }
        ]
      : []),
    { label: $t('backtest.operand.fixed'), items: [{ value: 'const', label: $t('backtest.operand.valueEllipsis') }] }
  ]);
</script>

<span class="operand">
  <span class="pick">
    <IndicatorPicker
      value={selValue}
      groups={pickerGroups}
      ariaLabel={$t('backtest.operand.pick')}
      {float}
      onchange={(v) => setSel(v)}
    />
  </span>

  {#if scriptRow}
    <span class="plot">
      <Dropdown
        value={operand.plot ?? ''}
        options={plotOptions}
        placeholder={$t('backtest.script.plot')}
        ariaLabel={$t('backtest.script.plot')}
        title={$t('backtest.script.plot')}
        float
        onpick={(v) => (operand = { ...operand, plot: v })}
      />
    </span>
  {:else if operand.kind === 'const'}
    <input class="val" type="number" step="any" bind:value={operand.value} aria-label={$t('backtest.operand.value')} />
  {:else if operand.kind === 'metric' && metricHasPeriod(operand.metric)}
    <input
      class="param"
      type="number"
      min="1"
      step="1"
      bind:value={operand.period}
      title={$t('backtest.metric.periodBars')}
      aria-label={$t('backtest.metric.periodBars')}
    />
  {:else if indDef?.params?.length}
    {#each indDef.params as p (p.key)}
      <input
        class="param"
        type="number"
        min="0"
        step={p.step}
        bind:value={operand[p.key]}
        title={p.label}
        aria-label={p.label}
      />
    {/each}
  {/if}
</span>

<style>
  .operand {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .pick {
    display: inline-flex;
    min-width: 132px;
    max-width: 180px;
  }
  .pick :global(.picker) {
    width: 100%;
  }
  .plot {
    display: inline-flex;
    min-width: 96px;
    max-width: 160px;
  }
  .val,
  .param {
    height: var(--control-h, 28px);
    font-size: var(--text-sm);
    background-color: var(--surface);
    text-align: center;
    padding: 0 var(--space-1);
  }
  /* Hide native spinners so 3–4 digit values aren't clipped by the arrows. */
  .val::-webkit-outer-spin-button,
  .val::-webkit-inner-spin-button,
  .param::-webkit-outer-spin-button,
  .param::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .val,
  .param {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .param {
    width: 68px;
  }
  .val {
    width: 88px;
  }
</style>
