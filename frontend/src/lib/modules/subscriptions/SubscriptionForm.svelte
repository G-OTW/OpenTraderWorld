<script>
  // Add/edit a subscription. Platform & category autocomplete from existing values.
  import { CURRENCIES, FREQUENCIES } from './api.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let {
    initial = null,
    suggestions = { platforms: [], categories: [] },
    // New subscriptions default to the module's display currency; still editable, and an
    // existing sub always keeps its own stored currency (it's the FX reference).
    defaultCurrency = 'USD',
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  // FREQUENCIES labels come from api.js (not localized there); map id -> key here for display.
  const FREQUENCY_KEYS = {
    weekly: 'subscriptions.form.freqWeekly',
    monthly: 'subscriptions.form.freqMonthly',
    quarterly: 'subscriptions.form.freqQuarterly',
    yearly: 'subscriptions.form.freqYearly'
  };

  let s = $state(blank());

  function blank() {
    const base = {
      name: '',
      platform: '',
      url: '',
      price: null,
      currency: defaultCurrency,
      frequency: 'monthly',
      category: '',
      started_on: '',
      active: true
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        platform: initial.platform ?? '',
        url: initial.url ?? '',
        category: initial.category ?? '',
        started_on: initial.started_on ?? ''
      };
    }
    return base;
  }

  function num(v) {
    return v === '' || v === null || v === undefined ? 0 : Number(v);
  }

  function submit() {
    onsubmit({
      name: s.name,
      platform: s.platform || null,
      url: s.url || null,
      price: num(s.price),
      currency: s.currency,
      frequency: s.frequency,
      category: s.category || null,
      started_on: s.started_on || null,
      active: !!s.active
    });
  }
</script>

<form
  class="sub-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <datalist id="sub-platforms">
    {#each suggestions.platforms as v}<option value={v}></option>{/each}
  </datalist>
  <datalist id="sub-categories">
    {#each suggestions.categories as v}<option value={v}></option>{/each}
  </datalist>

  <label class="field wide">
    <span>{$t('subscriptions.form.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={s.name} autofocus placeholder={$t('subscriptions.form.namePlaceholder')} />
  </label>

  <div class="grid">
    <label class="field">
      <span>{$t('subscriptions.form.platform')}</span>
      <input bind:value={s.platform} list="sub-platforms" autocomplete="off" placeholder={$t('subscriptions.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.category')}</span>
      <input bind:value={s.category} list="sub-categories" autocomplete="off" placeholder={$t('subscriptions.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.price')}</span>
      <input type="number" step="any" bind:value={s.price} />
    </label>
    <div class="field">
      <span>{$t('subscriptions.form.currency')}</span>
      <Dropdown
        bind:value={s.currency}
        ariaLabel={$t('subscriptions.form.currency')}
        options={CURRENCIES.map((c) => ({ value: c, label: c }))}
      />
    </div>
    <div class="field">
      <span>{$t('subscriptions.form.frequency')}</span>
      <Dropdown
        bind:value={s.frequency}
        ariaLabel={$t('subscriptions.form.frequency')}
        options={FREQUENCIES.map((f) => ({ value: f.id, label: $t(FREQUENCY_KEYS[f.id] ?? f.label) }))}
      />
    </div>
    <label class="field">
      <span>{$t('subscriptions.form.startedOn')}</span>
      <input type="date" bind:value={s.started_on} />
    </label>
  </div>

  <label class="field wide">
    <span>{$t('subscriptions.form.url')}</span>
    <input bind:value={s.url} placeholder={$t('subscriptions.form.optional')} />
  </label>

  <label class="check">
    <input type="checkbox" bind:checked={s.active} />
    <span>{$t('subscriptions.form.active')}</span>
  </label>

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('subscriptions.form.addSubscription')}</button>
  </div>
</form>

<style>
  .sub-form {
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
