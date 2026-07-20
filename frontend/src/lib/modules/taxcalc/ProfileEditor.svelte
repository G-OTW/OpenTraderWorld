<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Create/edit a Tax Profile. Start from a country template (autofills rates/allowances by
  // person type) or blank (custom_flat). Every field is overridable. Saving marks is_custom
  // when the user diverged from a template.
  import Modal from '$lib/ui/Modal.svelte';
  import { taxcalcApi, profileFromTemplate } from '$lib/modules/taxcalc/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { open = $bindable(false), editId = null, templates = [], onsaved = () => {} } = $props();

  let error = $state('');
  let saving = $state(false);
  let form = $state(blank());

  function blank() {
    return {
      name: '',
      country: '',
      currency: 'USD',
      person_type: 'individual',
      regime: 'custom_flat',
      flat_rate: null,
      marginal_income_rate: null,
      social_charges_rate: null,
      allowances: { capital_gains: { annual_free: 0 }, dividends: { annual_free: 0 } },
      loss_carry: {},
      holding_period_rules: [],
      wealth_tax: null,
      notes: '',
      is_custom: true
    };
  }

  // Load existing profile when editing.
  $effect(() => {
    if (open && editId) {
      taxcalcApi
        .profile(editId)
        .then((p) => (form = p))
        .catch((e) => (error = e.message));
    } else if (open && !editId) {
      form = blank();
    }
  });

  function applyTemplate(regime) {
    const t = templates.find((x) => x.regime === regime);
    if (!t) return;
    const p = profileFromTemplate(t, form.name);
    form = { ...p, name: form.name || t.label };
  }

  function cgFree(v) {
    form.allowances = { ...form.allowances, capital_gains: { annual_free: Number(v) || 0 } };
  }
  function divFree(v) {
    form.allowances = { ...form.allowances, dividends: { annual_free: Number(v) || 0 } };
  }

  // Wealth-tax brackets: marginal [{ up_to, rate }] ascending; the top row omits `up_to`
  // (null ⇒ everything above). null wealth_tax = the profile has no wealth tax.
  let wtBrackets = $derived(Array.isArray(form.wealth_tax) ? form.wealth_tax : []);
  function setBrackets(rows) {
    form.wealth_tax = rows.length ? rows : null;
  }
  function addBracket() {
    setBrackets([...wtBrackets, { up_to: null, rate: 0 }]);
  }
  function removeBracket(i) {
    setBrackets(wtBrackets.filter((_, ix) => ix !== i));
  }
  function editBracket(i, key, value) {
    const rows = wtBrackets.map((b, ix) => (ix === i ? { ...b } : b));
    rows[i][key] =
      key === 'up_to'
        ? value === '' || value == null
          ? null
          : Number(value)
        : Number(value) || 0;
    setBrackets(rows);
  }

  async function save() {
    saving = true;
    error = '';
    try {
      const payload = { ...form, is_custom: true };
      if (editId) await taxcalcApi.updateProfile(editId, payload);
      else await taxcalcApi.createProfile(payload);
      open = false;
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal bind:open title={editId ? $t('taxcalc.profile.editTitle') : $t('taxcalc.profile.newTitle')} size="md">
  <div class="grid">
    <label>
      {$t('taxcalc.profile.name')}
      <input bind:value={form.name} placeholder={$t('taxcalc.profile.namePlaceholder')} />
    </label>

    <label>
      {$t('taxcalc.profile.template')}
      <select onchange={(e) => applyTemplate(e.target.value)} value={form.regime}>
        {#each templates as tpl (tpl.regime)}
          <option value={tpl.regime}>{tpl.label}</option>
        {/each}
      </select>
    </label>

    <label>
      {$t('taxcalc.profile.country')}
      <input bind:value={form.country} placeholder="FR" maxlength="2" />
    </label>
    <label>
      {$t('taxcalc.profile.currency')}
      <input bind:value={form.currency} placeholder="EUR" maxlength="3" />
    </label>

    <label>
      {$t('taxcalc.profile.personType')}
      <select bind:value={form.person_type}>
        <option value="individual">{$t('taxcalc.profile.individual')}</option>
        <option value="professional">{$t('taxcalc.profile.professional')}</option>
      </select>
    </label>
    <span class="hint">{$t('taxcalc.profile.personTypeHint')}</span>

    <label>
      {$t('taxcalc.profile.flatRate')}
      <input type="number" step="0.01" bind:value={form.flat_rate} placeholder="—" />
    </label>
    <label>
      {$t('taxcalc.profile.marginalIncomeRate')}
      <input type="number" step="0.01" bind:value={form.marginal_income_rate} placeholder="—" />
    </label>
    <label>
      {$t('taxcalc.profile.socialCharges')}
      <input type="number" step="0.01" bind:value={form.social_charges_rate} placeholder="—" />
    </label>

    <label>
      {$t('taxcalc.profile.capitalGainsAllowance', { currency: form.currency })}
      <input
        type="number"
        step="1"
        value={form.allowances?.capital_gains?.annual_free ?? 0}
        oninput={(e) => cgFree(e.target.value)}
      />
    </label>
    <label>
      {$t('taxcalc.profile.dividendAllowance', { currency: form.currency })}
      <input
        type="number"
        step="1"
        value={form.allowances?.dividends?.annual_free ?? 0}
        oninput={(e) => divFree(e.target.value)}
      />
    </label>

    <div class="full wealth">
      <div class="wealth-head">
        <span>{$t('taxcalc.profile.wealthTaxBrackets')} <span class="hint-inline">{$t('taxcalc.profile.wealthTaxBracketsHint')}</span></span>
        <button type="button" class="link" onclick={addBracket}>{$t('taxcalc.profile.addBracket')}</button>
      </div>
      {#if wtBrackets.length === 0}
        <p class="hint">{$t('taxcalc.profile.noWealthTax')}</p>
      {:else}
        {#each wtBrackets as b, i (i)}
          <div class="wt-row">
            <input
              type="number"
              step="1"
              placeholder={i === wtBrackets.length - 1 ? $t('taxcalc.profile.topBracket') : $t('taxcalc.profile.upTo')}
              value={b.up_to ?? ''}
              oninput={(e) => editBracket(i, 'up_to', e.target.value)}
            />
            <input
              type="number"
              step="0.01"
              placeholder={$t('taxcalc.profile.ratePercent')}
              value={b.rate ?? 0}
              oninput={(e) => editBracket(i, 'rate', e.target.value)}
            />
            <button type="button" class="link red" onclick={() => removeBracket(i)}><Icon name="x" size={13} /></button>
          </div>
        {/each}
        <p class="hint">{$t('taxcalc.profile.marginalBracketHint')}</p>
      {/if}
    </div>

    <label class="full">
      {$t('taxcalc.profile.notes')}
      <textarea bind:value={form.notes} rows="2"></textarea>
    </label>
  </div>

  <ErrorText error={error} copyable />

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={save} disabled={saving}>{saving ? $t('common.saving') : $t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .full,
  label:has(select[value='custom_flat']),
  label:nth-child(2) {
    grid-column: 1 / -1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .hint {
    grid-column: 1 / -1;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .hint-inline {
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: var(--fw-normal);
  }
  .wealth {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .wealth-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .wt-row {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: var(--space-2);
    align-items: center;
  }
  .link {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0;
  }
  .link:hover {
    color: var(--text);
  }
  .link.red {
    color: var(--muted);
  }
  .link.red:hover {
    color: var(--red);
  }
  button:disabled {
    opacity: 0.6;
  }
</style>
