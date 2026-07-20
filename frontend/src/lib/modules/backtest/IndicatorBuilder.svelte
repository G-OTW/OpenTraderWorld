<script>
  import Icon from '$lib/ui/Icon.svelte';
  import IndicatorPicker from '$lib/ui/IndicatorPicker.svelte';
  import {
    INDICATOR_GROUPS, INDICATORS, PRICE_FIELDS, PRICE_TOKENS, MULTI_OUTPUTS,
    indicatorById, isChainableIndicator, defaultBuilderModel, modelToDef, defToModel
  } from './api.js';
  import { t } from '$lib/i18n';

  // Grouped options for the searchable indicator picker (mirrors INDICATOR_GROUPS).
  const pickerGroups = INDICATOR_GROUPS.map((g) => ({
    label: g.label,
    items: g.items.map((it) => ({ value: it.id, label: it.label }))
  }));

  // v2 builder: a list of *named* steps. Each step is either a built-in indicator applied to an
  // explicit source (a price field or an earlier step's output) or a math formula referencing
  // steps by `@name`. Internally we edit that model and compile it to the engine's node-index DAG
  // (`def = { nodes, output }`), which is what the parent binds and saves. Mirrors the Rust
  // backtest::custom shape + its "chainable indicators only" rule.
  let { def = $bindable({ nodes: [], output: 0 }) } = $props();

  // Seed the model from the incoming def once (edit an existing indicator), else a fresh model.
  let model = $state(seedModel(def));
  let compileError = $state('');

  function seedModel(d) {
    if (d?.nodes?.length) {
      const r = defToModel(d);
      if (r.model) return r.model;
    }
    return defaultBuilderModel();
  }

  let uid = 0;
  const freshId = () => `n${Date.now()}_${++uid}`;

  // Sources selectable for step at index i: price tokens + earlier steps' names (and their
  // multi-output sub-series). A step can only reference what's defined above it.
  function sourcesBefore(i) {
    const list = [...PRICE_TOKENS];
    for (let j = 0; j < i; j++) {
      const s = model.steps[j];
      if (!s.name) continue;
      const subs = MULTI_OUTPUTS[s.indicator];
      if (s.kind === 'ind' && subs) subs.forEach((sub) => list.push(`@${s.name}.${sub}`));
      else list.push(`@${s.name}`);
    }
    return list;
  }
  const allRefTokens = () => sourcesBefore(model.steps.length);

  const paramDefs = (id) => indicatorById(id)?.params ?? [];

  function addIndicatorStep() {
    const prev = model.steps.at(-1);
    const src = prev ? `@${prev.name}` : '@close';
    const step = { id: freshId(), name: uniqueName('ind'), kind: 'ind', indicator: 'rsi', src };
    for (const p of paramDefs('rsi')) step[p.key] = p.def;
    model.steps.push(step);
    model.outputs = [step.name];
  }
  function addFormulaStep() {
    const prev = model.steps.at(-1);
    const step = { id: freshId(), name: uniqueName('f'), kind: 'formula', expr: prev ? `@${prev.name}` : '@close' };
    model.steps.push(step);
    model.outputs = [step.name];
  }
  function uniqueName(base) {
    const taken = new Set(model.steps.map((s) => s.name));
    let i = 1;
    while (taken.has(`${base}${i}`)) i++;
    return `${base}${i}`;
  }

  function removeStep(i) {
    if (model.steps.length <= 1) return;
    const gone = model.steps[i].name;
    model.steps.splice(i, 1);
    model.outputs = model.outputs.filter((o) => o !== gone && !o.startsWith(`${gone}.`));
    if (!model.outputs.length && model.steps.length) model.outputs = [model.steps.at(-1).name];
  }

  function changeIndicator(step, id) {
    step.indicator = id;
    for (const p of paramDefs(id)) if (step[p.key] == null) step[p.key] = p.def;
    // If the new indicator isn't chainable but the source is a step, snap back to price.
    if (!isChainableIndicator(id) && step.src && !PRICE_TOKENS.includes(step.src)) step.src = '@close';
  }

  function renameStep(step, raw) {
    const clean = raw.trim().replace(/\s+/g, '_');
    if (!/^[a-zA-Z_]\w*$/.test(clean)) return; // ignore invalid; keep old until valid
    const old = step.name;
    if (clean === old) return;
    // propagate into sources, formulas, outputs
    for (const s of model.steps) {
      if (s.src === `@${old}`) s.src = `@${clean}`;
      if (s.src?.startsWith(`@${old}.`)) s.src = `@${clean}${s.src.slice(old.length + 1)}`;
      if (s.kind === 'formula' && s.expr) s.expr = s.expr.replaceAll(`@${old}`, `@${clean}`);
    }
    model.outputs = model.outputs.map((o) => (o === old ? clean : o.startsWith(`${old}.`) ? clean + o.slice(old.length) : o));
    step.name = clean;
  }

  // A step is "an output" if its name (or any of its sub-series) is in outputs.
  function isOutput(step) {
    if (model.outputs.includes(step.name)) return true;
    const subs = MULTI_OUTPUTS[step.indicator];
    return step.kind === 'ind' && subs ? subs.some((s) => model.outputs.includes(`${step.name}.${s}`)) : false;
  }
  function toggleOutput(step) {
    const keys = step.kind === 'ind' && MULTI_OUTPUTS[step.indicator]
      ? MULTI_OUTPUTS[step.indicator].map((s) => `${step.name}.${s}`)
      : [step.name];
    const on = keys.some((k) => model.outputs.includes(k));
    model.outputs = on ? model.outputs.filter((o) => !keys.includes(o)) : [...model.outputs, ...keys];
    if (!model.outputs.length) model.outputs = [keys[0]];
  }

  function insertRef(step, token) {
    step.expr = `${step.expr ?? ''} ${token}`.trim();
  }

  // Recompile the model into the bound DAG whenever it changes; surface a precise error otherwise.
  $effect(() => {
    try {
      const compiled = modelToDef($state.snapshot(model));
      def = { nodes: compiled.nodes, output: compiled.output, ...(compiled.outputs?.length > 1 ? { outputs: compiled.outputs } : {}) };
      compileError = '';
    } catch (e) {
      compileError = e.message ?? String(e);
    }
  });
</script>

<div class="builder">
  <div class="steps">
    {#each model.steps as step, i (step.id)}
      {@const chainable = step.kind !== 'ind' || isChainableIndicator(step.indicator)}
      <div class="step" class:out={isOutput(step)}>
        <span class="num">{i + 1}</span>
        <div class="controls">
          <input
            class="name-edit"
            value={step.name}
            onchange={(e) => renameStep(step, e.target.value)}
            title={$t('backtest.ind.stepName')}
          />
          <span class="eq">=</span>

          {#if step.kind === 'ind'}
            <span class="ind">
              <IndicatorPicker
                value={step.indicator}
                groups={pickerGroups}
                ariaLabel={$t('backtest.ind.pickIndicator')}
                onchange={(id) => changeIndicator(step, id)}
              />
            </span>

            <span class="src-wrap">
              <span class="of">{$t('backtest.ind.of')}</span>
              <select class="src" bind:value={step.src}>
                {#each sourcesBefore(i) as s (s)}
                  {@const isPrice = PRICE_TOKENS.includes(s)}
                  <option value={s} disabled={!isPrice && !isChainableIndicator(step.indicator)}>{s}</option>
                {/each}
              </select>
            </span>

            {#each paramDefs(step.indicator) as p (p.key)}
              <input class="num-in" type="number" step={p.step ?? 1} bind:value={step[p.key]} title={p.label} placeholder={p.label} />
            {/each}
          {:else}
            <input class="formula-in" bind:value={step.expr} placeholder="@a / @b" title={$t('backtest.ind.formula')} />
          {/if}
        </div>

        <button
          class="out-toggle"
          class:on={isOutput(step)}
          onclick={() => toggleOutput(step)}
          title={$t('backtest.ind.outputTitle')}
          type="button"
        >
          <Icon name="target" size={12} />
        </button>
        <button class="del" type="button" onclick={() => removeStep(i)} disabled={model.steps.length <= 1} title={$t('common.remove')}>
          <Icon name="x" size={11} />
        </button>
      </div>

      {#if step.kind === 'ind' && MULTI_OUTPUTS[step.indicator]}
        <div class="sub-note">↳ {$t('backtest.ind.emits')} {MULTI_OUTPUTS[step.indicator].map((s) => `@${step.name}.${s}`).join('  ')}</div>
      {/if}
    {/each}
  </div>

  <div class="toolbar">
    <button class="add" type="button" onclick={addIndicatorStep}><Icon name="plus" size={12} /> {$t('backtest.ind.addIndicator')}</button>
    <button class="add ghost" type="button" onclick={addFormulaStep}><Icon name="plus" size={12} /> {$t('backtest.ind.addFormula')}</button>
  </div>

  {#if allRefTokens().length}
    <div class="avail">
      {$t('backtest.ind.available')}
      {#each allRefTokens() as ref (ref)}<span class="chip">{ref}</span>{/each}
    </div>
  {/if}

  {#if compileError}
    <p class="compile-err"><Icon name="x" size={12} /> {compileError}</p>
  {/if}
  <p class="hint">{$t('backtest.ind.hintV2')}</p>
</div>

<style>
  .builder { display: flex; flex-direction: column; gap: var(--space-2); }
  .steps { display: flex; flex-direction: column; gap: var(--space-1); }

  .step {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
  }
  .step.out { border-color: var(--border-control); }

  .num {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
    width: 1.2em;
    text-align: right;
    flex-shrink: 0;
  }
  .controls { display: flex; flex-wrap: wrap; gap: var(--space-1); flex: 1; min-width: 0; align-items: center; }
  .controls select, .controls input { font-size: var(--text-xs); min-width: 0; }

  .eq { color: var(--muted); }
  .name-edit {
    width: 104px;
    font-family: var(--mono, ui-monospace, monospace);
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    padding: 4px 7px;
  }

  .ind { display: inline-flex; min-width: 150px; }
  .ind :global(.picker) { width: 100%; }

  .src-wrap { display: inline-flex; align-items: center; gap: 5px; }
  .of { color: var(--muted); font-size: var(--text-xs); }
  .src {
    font-family: var(--mono, ui-monospace, monospace);
    color: var(--text);
  }

  .num-in { width: 72px; -moz-appearance: textfield; appearance: textfield; }
  .num-in::-webkit-outer-spin-button, .num-in::-webkit-inner-spin-button { -webkit-appearance: none; margin: 0; }
  .formula-in { flex: 1; min-width: 160px; font-family: var(--mono, ui-monospace, monospace); }

  .out-toggle, .del {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    padding: 3px;
    flex-shrink: 0;
  }
  .out-toggle.on { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); }
  .out-toggle:hover { color: var(--text); }
  .del:hover:not(:disabled) { color: var(--red); border-color: var(--red); }
  .del:disabled { opacity: 0.4; cursor: not-allowed; }

  .sub-note {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono, ui-monospace, monospace);
    margin: 0 0 0 calc(1.2em + var(--space-2));
  }

  .toolbar { display: flex; gap: var(--space-1); flex-wrap: wrap; }
  .add {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .add.ghost { background: transparent; }
  .add:hover { border-color: var(--border-control); }

  .avail { font-size: var(--text-xs); color: var(--muted); display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .chip {
    font-family: var(--mono, ui-monospace, monospace);
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px 6px;
  }

  .compile-err {
    font-size: var(--text-xs);
    color: var(--red);
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0;
  }
  .hint { font-size: var(--text-xs); color: var(--muted); font-style: italic; margin: 0; }
</style>
