<script>
  // Add/edit a time project: name, category, color, planned end, time budget (hours),
  // hourly rate (+ currency).
  import { CURRENCIES, COLOR_SWATCHES } from './api.js';
  import { t } from '$lib/i18n';

  let { initial = null, categories = [], onsubmit = () => {}, oncancel = () => {} } = $props();

  let p = $state(blank());

  function blank() {
    const base = {
      name: '',
      category: '',
      color: COLOR_SWATCHES[0].hex,
      planned_end: '',
      time_budget_hours: null,
      hourly_rate: null,
      rate_currency: 'USD',
      archived: false
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        category: initial.category ?? '',
        color: initial.color ?? COLOR_SWATCHES[0].hex,
        planned_end: initial.planned_end ?? ''
      };
    }
    return base;
  }

  function numOrNull(v) {
    return v === '' || v === null || v === undefined ? null : Number(v);
  }

  function submit() {
    onsubmit({
      name: p.name,
      category: p.category || null,
      color: p.color || null,
      planned_end: p.planned_end || null,
      time_budget_hours: numOrNull(p.time_budget_hours),
      hourly_rate: numOrNull(p.hourly_rate),
      rate_currency: p.rate_currency,
      archived: !!p.archived
    });
  }
</script>

<form class="proj-form" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <datalist id="proj-categories">
    {#each categories as c}<option value={c}></option>{/each}
  </datalist>

  <label class="field wide">
    <span>{$t('time.form.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={p.name} autofocus placeholder={$t('time.form.namePlaceholder')} />
  </label>

  <div class="grid">
    <label class="field">
      <span>{$t('time.form.category')}</span>
      <input bind:value={p.category} list="proj-categories" autocomplete="off" placeholder={$t('time.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('time.form.plannedEnd')}</span>
      <input type="date" bind:value={p.planned_end} />
    </label>
    <label class="field">
      <span>{$t('time.form.timeBudgetHours')}</span>
      <input type="number" step="any" bind:value={p.time_budget_hours} placeholder={$t('time.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('time.form.hourlyRate')}</span>
      <input type="number" step="any" bind:value={p.hourly_rate} placeholder={$t('time.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('time.form.rateCurrency')}</span>
      <select bind:value={p.rate_currency}>
        {#each CURRENCIES as c}<option value={c}>{c}</option>{/each}
      </select>
    </label>
  </div>

  <div class="field">
    <span>{$t('time.form.color')}</span>
    <div class="swatches">
      {#each COLOR_SWATCHES as sw}
        <button
          type="button"
          class="swatch"
          class:active={p.color === sw.hex}
          style:background={sw.hex}
          title={sw.name}
          aria-label={sw.name}
          onclick={() => (p.color = sw.hex)}
        ></button>
      {/each}
    </div>
  </div>

  {#if initial}
    <label class="check">
      <input type="checkbox" bind:checked={p.archived} />
      <span>{$t('time.form.archived')}</span>
    </label>
  {/if}

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('time.addProject')}</button>
  </div>
</form>

<style>
  .proj-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .field.wide {
    width: 100%;
  }
  .swatches {
    display: flex;
    gap: 6px;
  }
  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 999px;
    border: 2px solid transparent;
    cursor: pointer;
  }
  .swatch.active {
    border-color: var(--text);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
  }
  .check input {
    width: 16px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
