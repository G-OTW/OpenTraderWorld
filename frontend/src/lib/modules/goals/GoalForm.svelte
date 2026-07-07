<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Add/edit a goal: name, deadline, details, and an editable KPI table
  // (name, target, current, points, reached).
  import { blankKpi } from './api.js';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { t } from '$lib/i18n';

  let { initial = null, categories = [], onsubmit = () => {}, oncancel = () => {} } = $props();

  let g = $state(blank());

  function blank() {
    const base = { name: '', deadline: '', details: '', category: '', kpis: [blankKpi()] };
    if (initial) {
      return {
        name: initial.name ?? '',
        deadline: initial.deadline ?? '',
        details: initial.details ?? '',
        category: initial.category ?? '',
        kpis: (initial.kpis ?? []).map((k) => ({ ...blankKpi(), ...k }))
      };
    }
    return base;
  }

  function addKpi() {
    g.kpis = [...g.kpis, blankKpi()];
  }
  function removeKpi(i) {
    g.kpis = g.kpis.filter((_, idx) => idx !== i);
  }

  function num(v) {
    return v === '' || v === null || v === undefined ? 0 : Number(v);
  }

  function payload() {
    return {
      name: g.name,
      deadline: g.deadline || null,
      details: g.details,
      category: (g.category || '').trim(),
      kpis: g.kpis
        .filter((k) => (k.name || '').trim() !== '')
        .map((k) => ({
          name: k.name.trim(),
          target: num(k.target),
          current: num(k.current),
          points: num(k.points),
          reached: !!k.reached
        }))
    };
  }

  function submit() {
    onsubmit(payload());
  }

  // For the reminder button: ensure the goal exists (saving on create) and return its
  // link info. Returns null if the goal has no name yet.
  async function resolveLink() {
    if (!g.name.trim()) return null;
    if (initial?.id) return { linkedId: initial.id, linkedName: g.name, defaultName: g.name };
    const saved = await onsubmit(payload(), { keepOpen: true });
    if (!saved?.id) return null;
    return { linkedId: saved.id, linkedName: g.name, defaultName: g.name };
  }
</script>

<form
  class="goal-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <label class="field">
    <span>{$t('goals.form.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={g.name} autofocus placeholder={$t('goals.form.namePlaceholder')} />
  </label>

  <label class="field">
    <span>{$t('goals.form.deadline')}</span>
    <input type="date" bind:value={g.deadline} />
  </label>

  <label class="field">
    <span>{$t('goals.form.category')}</span>
    <input bind:value={g.category} list="goal-categories" placeholder={$t('goals.form.categoryPlaceholder')} />
    <datalist id="goal-categories">
      {#each categories as c}<option value={c}></option>{/each}
    </datalist>
  </label>

  <div class="kpis">
    <div class="kpi-head">
      <span>{$t('goals.form.metrics')}</span>
      <button type="button" class="mini" onclick={addKpi}><Icon name="plus" size={12} /> {$t('goals.form.addMetric')}</button>
    </div>
    <div class="kpi-table">
      <div class="kpi-row labels">
        <span>{$t('goals.form.metricName')}</span>
        <span>{$t('goals.detail.target')}</span>
        <span>{$t('goals.detail.now')}</span>
        <span>{$t('goals.form.points')}</span>
        <span>{$t('goals.form.done')}</span>
        <span></span>
      </div>
      {#each g.kpis as k, i}
        <div class="kpi-row">
          <input bind:value={k.name} placeholder={$t('goals.form.metricName')} />
          <input type="number" step="any" bind:value={k.target} />
          <input type="number" step="any" bind:value={k.current} />
          <input type="number" step="any" bind:value={k.points} />
          <input type="checkbox" bind:checked={k.reached} />
          <button type="button" class="x" onclick={() => removeKpi(i)} aria-label={$t('goals.form.remove')}><Icon name="x" size={13} /></button>
        </div>
      {/each}
    </div>
  </div>

  <label class="field">
    <span>{$t('goals.detail.details')}</span>
    <textarea bind:value={g.details} rows="3" placeholder={$t('goals.form.optional')}></textarea>
  </label>

  <QuickReminderButton variant="inline" title={$t('goals.addReminder')} kind="goal" {resolveLink} />

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('goals.addGoal')}</button>
  </div>
</form>

<style>
  .goal-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .kpi-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: 4px;
  }
  .mini {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 3px 8px;
    cursor: pointer;
    font-size: 0.72rem;
    text-transform: none;
    letter-spacing: 0;
  }
  .kpi-table {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .kpi-row {
    display: grid;
    grid-template-columns: 1fr 70px 70px 60px 28px 24px;
    gap: 6px;
    align-items: center;
  }
  .kpi-row.labels {
    font-size: 0.68rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .kpi-row.labels span {
    padding-left: 2px;
  }
  .kpi-row input[type='checkbox'] {
    width: 16px;
    justify-self: center;
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.8rem;
  }
  .x:hover {
    color: var(--red);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
