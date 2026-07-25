<script>
  // Edits one signal operand inline as part of a condition sentence. A single grouped
  // select covers all kinds — indicators (grouped), price fields, or a custom value —
  // with the indicator params / const value as compact inputs right after it.
  // Bound via $bindable so the parent's settings object updates directly.
  import { INDICATOR_GROUPS, PRICE_FIELDS, indicatorById } from './api.js';
  import IndicatorPicker from '$lib/ui/IndicatorPicker.svelte';
  import { t } from '$lib/i18n';

  // `customIndicators` (expert mode) = [{ id, name }] saved custom indicators that can be
  // referenced as an operand. Empty in normal mode, so the Custom optgroup is hidden.
  let { operand = $bindable(), customIndicators = [] } = $props();

  const selValue = $derived(
    operand.kind === 'price'
      ? `price:${operand.field}`
      : operand.kind === 'const'
        ? 'const'
        : operand.kind === 'custom_indicator'
          ? `custom:${operand.id}`
          : `ind:${operand.indicator}`
  );

  // Switching selection reseeds the operand, keeping shared params / the const value.
  function setSel(v) {
    if (v === 'const') {
      operand = { kind: 'const', value: operand.kind === 'const' ? operand.value : 0 };
    } else if (v.startsWith('price:')) {
      operand = { kind: 'price', field: v.slice(6) };
    } else if (v.startsWith('custom:')) {
      operand = { kind: 'custom_indicator', id: v.slice(7) };
    } else {
      const id = v.slice(4);
      const def = indicatorById(id);
      const next = { kind: 'indicator', indicator: id };
      for (const p of def?.params ?? []) next[p.key] = operand[p.key] ?? p.def;
      operand = next;
    }
  }

  const indDef = $derived(operand.kind === 'indicator' ? indicatorById(operand.indicator) : null);

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
    { label: $t('backtest.operand.fixed'), items: [{ value: 'const', label: $t('backtest.operand.valueEllipsis') }] }
  ]);
</script>

<span class="operand">
  <span class="pick">
    <IndicatorPicker
      value={selValue}
      groups={pickerGroups}
      ariaLabel={$t('backtest.operand.pick')}
      onchange={(v) => setSel(v)}
    />
  </span>

  {#if operand.kind === 'const'}
    <input class="val" type="number" step="any" bind:value={operand.value} aria-label={$t('backtest.operand.value')} />
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
  .val,
  .param {
    height: 28px;
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
