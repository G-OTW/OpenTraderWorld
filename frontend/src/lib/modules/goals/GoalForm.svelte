<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Add/edit a goal: name, deadline, details, and an editable KPI table
  // (name, target, current, points, reached).
  import { blankKpi, COLOR_SWATCHES } from './api.js';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { t } from '$lib/i18n';

  let { initial = null, categories = [], onsubmit = () => {}, oncancel = () => {} } = $props();

  let g = $state(blank());

  function blank() {
    const base = { name: '', deadline: '', details: '', category: '', color: '', kpis: [blankKpi()] };
    if (initial) {
      return {
        name: initial.name ?? '',
        deadline: initial.deadline ?? '',
        details: initial.details ?? '',
        category: initial.category ?? '',
        color: initial.color ?? '',
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
      color: g.color || null,
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

  <div class="field">
    <span>{$t('goals.form.color')}</span>
    <div class="swatches">
      {#each COLOR_SWATCHES as sw}
        <button
          type="button"
          class="swatch"
          class:none={sw.hex === ''}
          class:active={(g.color || '') === sw.hex}
          style:background={sw.hex || 'transparent'}
          title={sw.hex ? sw.name : $t('common.colorNone')}
          aria-label={sw.hex ? sw.name : $t('common.colorNone')}
          onclick={() => (g.color = sw.hex)}
        ></button>
      {/each}
    </div>
  </div>

  <div class="kpis">
    <div class="kpi-head">
      <span>{$t('goals.form.metrics')}</span>
      <button type="button" class="mini" onclick={addKpi}><Icon name="plus" size={12} /> {$t('goals.form.addMetric')}</button>
    </div>
    <!-- The rows live in a bounded box: past 5 metrics it scrolls instead of growing the
         modal, and the scrollbar stays hidden so the panel keeps a clean edge. -->
    <div class="kpi-box">
      <div class="kpi-row labels">
        <span>{$t('goals.form.metricName')}</span>
        <span>{$t('goals.detail.target')}</span>
        <span>{$t('goals.detail.now')}</span>
        <span>{$t('goals.form.points')}</span>
        <span class="ctr">{$t('goals.form.done')}</span>
        <span></span>
      </div>
      <div class="kpi-scroll">
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
    font-size: var(--text-xs);
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
    font-size: var(--text-xs);
    text-transform: none;
    letter-spacing: 0;
  }
  /* Bounded panel around the rows so the metric block reads as one object. */
  .kpi-box {
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-2);
  }
  /* 5 rows (row + gap) then it scrolls; the bar is hidden on every engine. */
  .kpi-scroll {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: calc(5 * 32px + 4 * var(--space-2));
    overflow-y: auto;
    scrollbar-width: none; /* Firefox */
    -ms-overflow-style: none; /* IE/Edge */
  }
  .kpi-scroll::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  .kpi-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 86px 86px 72px 32px 26px;
    gap: var(--space-2);
    align-items: center;
  }
  .kpi-row input {
    height: 32px;
  }
  .kpi-row.labels {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    padding-bottom: var(--space-2);
    margin-bottom: var(--space-2);
    border-bottom: 0.5px solid var(--border);
  }
  .kpi-row.labels span {
    padding-left: 2px;
  }
  .kpi-row.labels .ctr {
    text-align: center;
    padding-left: 0;
  }
  .kpi-row input[type='checkbox'] {
    width: 16px;
    height: auto;
    justify-self: center;
  }
  .swatches {
    display: flex;
    gap: 6px;
  }
  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 0;
    border: 0.5px solid var(--border-control);
    cursor: pointer;
  }
  /* "No colour": a neutral tile with a diagonal slash. */
  .swatch.none {
    background-image: linear-gradient(
      to top right,
      transparent calc(50% - 0.5px),
      var(--border-control) calc(50% - 0.5px),
      var(--border-control) calc(50% + 0.5px),
      transparent calc(50% + 0.5px)
    );
  }
  .swatch.active {
    border: 1.5px solid var(--text);
    outline: 1px solid var(--accent);
    outline-offset: 1px;
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
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
