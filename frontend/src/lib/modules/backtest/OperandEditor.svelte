<script>
  // Edits one signal operand inline as part of a condition sentence. A single grouped
  // select covers all kinds — indicators (grouped), price fields, or a custom value —
  // with the indicator params / const value as compact inputs right after it.
  // Bound via $bindable so the parent's settings object updates directly.
  import { INDICATOR_GROUPS, PRICE_FIELDS, indicatorById } from './api.js';
  import { t } from '$lib/i18n';

  let { operand = $bindable() } = $props();

  const selValue = $derived(
    operand.kind === 'price'
      ? `price:${operand.field}`
      : operand.kind === 'const'
        ? 'const'
        : `ind:${operand.indicator}`
  );

  // Switching selection reseeds the operand, keeping shared params / the const value.
  function setSel(v) {
    if (v === 'const') {
      operand = { kind: 'const', value: operand.kind === 'const' ? operand.value : 0 };
    } else if (v.startsWith('price:')) {
      operand = { kind: 'price', field: v.slice(6) };
    } else {
      const id = v.slice(4);
      const def = indicatorById(id);
      const next = { kind: 'indicator', indicator: id };
      for (const p of def?.params ?? []) next[p.key] = operand[p.key] ?? p.def;
      operand = next;
    }
  }

  const indDef = $derived(operand.kind === 'indicator' ? indicatorById(operand.indicator) : null);
</script>

<span class="operand">
  <select value={selValue} onchange={(e) => setSel(e.target.value)}>
    {#each INDICATOR_GROUPS as g (g.label)}
      <optgroup label={g.label}>
        {#each g.items as i (i.id)}<option value={`ind:${i.id}`}>{i.label}</option>{/each}
      </optgroup>
    {/each}
    <optgroup label={$t('backtest.operand.price')}>
      {#each PRICE_FIELDS as f (f)}<option value={`price:${f}`}>{f}</option>{/each}
    </optgroup>
    <optgroup label={$t('backtest.operand.fixed')}>
      <option value="const">{$t('backtest.operand.valueEllipsis')}</option>
    </optgroup>
  </select>

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
  .operand select {
    height: 28px;
    font-size: 0.8rem;
    max-width: 148px;
    background-color: var(--surface);
  }
  .val,
  .param {
    height: 28px;
    font-size: 0.8rem;
    background-color: var(--surface);
    text-align: center;
    padding: 0 var(--space-1);
  }
  .param {
    width: 52px;
  }
  .val {
    width: 76px;
  }
</style>
