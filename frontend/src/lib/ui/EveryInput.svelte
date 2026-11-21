<script>
  // "Every [N] [minutes|hours]": a period typed in the unit the user thinks in. The value
  // is always minutes; the unit is display only. Until the user picks one, whole hours
  // read as hours.
  import Dropdown from './Dropdown.svelte';
  import { t } from '$lib/i18n';

  let {
    minutes = 60,
    label = '',
    maxMinutes = 10080,
    onchange = () => {}
  } = $props();

  let unitPick = $state(null);
  const unit = $derived(unitPick ?? ((minutes ?? 60) % 60 === 0 ? 'h' : 'min'));
  const factor = $derived(unit === 'h' ? 60 : 1);
  const shown = $derived(Math.max(1, Math.round((minutes ?? 60) / factor)));

  const positive = (v) => Math.max(1, Math.floor(Number(v)) || 1);

  function setUnit(u) {
    unitPick = u;
    onchange(shown * (u === 'h' ? 60 : 1));
  }
</script>

<div class="every">
  <label class="num">
    <span>{label}</span>
    <input
      type="number"
      min="1"
      max={Math.floor(maxMinutes / factor)}
      value={shown}
      oninput={(e) => {
        // An emptied field is mid-typing: wait for a number instead of snapping to 1.
        if (e.currentTarget.value !== '') onchange(positive(e.currentTarget.value) * factor);
      }}
      onblur={(e) => (e.currentTarget.value = String(shown))}
    />
  </label>
  <div class="unit">
    <Dropdown
      value={unit}
      ariaLabel={$t('recurrence.unit')}
      options={[
        { value: 'min', label: $t('recurrence.unit.minutes') },
        { value: 'h', label: $t('recurrence.unit.hours') }
      ]}
      onpick={setUnit}
    />
  </div>
</div>

<style>
  .every {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .num {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .num input {
    width: 68px;
  }
  /* The Dropdown trigger fills its parent, so it needs a width to size to. */
  .unit {
    width: 110px;
  }
  @media (max-width: 767px) {
    .num input {
      min-height: 44px;
    }
  }
</style>
