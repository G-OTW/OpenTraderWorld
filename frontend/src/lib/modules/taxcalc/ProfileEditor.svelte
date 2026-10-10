<script>
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  // Create/edit a Tax Profile. Start from a country template (autofills rates/allowances by
  // person type) or blank (custom_flat). Every field is overridable. Saving marks is_custom
  // when the user diverged from a template.
  import Modal from '$lib/ui/Modal.svelte';
  import { taxcalcApi, profileFromTemplate } from '$lib/modules/taxcalc/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import RegimeInfo from '$lib/modules/taxcalc/RegimeInfo.svelte';

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
      is_custom: true,
      cost_method: null
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

  // A file-backed regime carries its own rates and rules: only the custom one is edited here.
  const tpl = $derived(templates.find((x) => x.regime === form.regime) ?? null);
  const isCustom = $derived(!tpl || !!tpl.custom);
  const readsMarginal = $derived(!!tpl?.inputs?.includes('marginal_rate'));

  // The regime's own method, shown as the default choice.
  const regimeMethod = $derived(templates.find((x) => x.regime === form.regime)?.cost_method ?? 'fifo');
  const methodLabel = (m) => $t(`taxcalc.profile.method.${m}`);
  const methodOptions = $derived([
    { value: '', label: $t('taxcalc.profile.methodDefault', { method: methodLabel(regimeMethod) }) },
    ...['average', 'fifo', 'uk_pool'].map((m) => ({ value: m, label: methodLabel(m) }))
  ]);

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

    <div class="fld full">
      <span>{$t('taxcalc.profile.template')} {#if tpl}<RegimeInfo info={tpl} />{/if}</span>
      <Dropdown
        value={form.regime}
        onpick={applyTemplate}
        ariaLabel={$t('taxcalc.profile.template')}
        options={templates.map((tpl) => ({ value: tpl.regime, label: tpl.label }))}
      />
    </div>

    <label>
      {$t('taxcalc.profile.country')}
      <input bind:value={form.country} placeholder="FR" maxlength="2" />
    </label>
    <label>
      {$t('taxcalc.profile.currency')}
      <input bind:value={form.currency} placeholder="EUR" maxlength="3" />
    </label>

    <div class="fld">
      <span>{$t('taxcalc.profile.personType')}</span>
      <Dropdown
        bind:value={form.person_type}
        ariaLabel={$t('taxcalc.profile.personType')}
        options={[
          { value: 'individual', label: $t('taxcalc.profile.individual') },
          { value: 'professional', label: $t('taxcalc.profile.professional') }
        ]}
      />
    </div>
    <span class="hint">{$t('taxcalc.profile.personTypeHint')}</span>

    <div class="fld full">
      <span>{$t('taxcalc.profile.costMethod')}</span>
      <Dropdown
        value={form.cost_method ?? ''}
        options={methodOptions}
        ariaLabel={$t('taxcalc.profile.costMethod')}
        onpick={(v) => (form.cost_method = v || null)}
      />
    </div>
    <span class="hint">{$t('taxcalc.profile.costMethodHint')}</span>

    {#if !isCustom}
      <div class="full regime">
        <div class="regime-head">
          <strong>{tpl.label}</strong>
          <RegimeInfo info={tpl} />
          {#if tpl.status === 'draft'}<span class="badge warn">{$t('taxcalc.regime.draftBadge')}</span>{/if}
          {#if tpl.coverage === 'simple'}<span class="badge">{$t('taxcalc.regime.simpleBadge')}</span>{/if}
        </div>
        {#if tpl.source_note}<p class="hint">{tpl.source_note}</p>{/if}
        <details>
          <summary>{$t('taxcalc.regime.sources')}</summary>
          <ul>
            {#each tpl.sources ?? [] as src}<li>{src}</li>{/each}
          </ul>
        </details>
      </div>
      {#if readsMarginal}
        <label>
          {$t('taxcalc.profile.marginalIncomeRate')}
          <input type="number" step="0.01" bind:value={form.marginal_income_rate} placeholder="30" />
        </label>
      {/if}
    {:else}
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
    {/if}

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
  .full {
    grid-column: 1 / -1;
  }
  label,
  .fld {
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
  .regime {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
  }
  .regime-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2);
  }
  .regime ul {
    margin: var(--space-1) 0 0;
    padding-left: var(--space-4);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .regime summary {
    font-size: var(--text-xs);
    color: var(--muted);
    cursor: pointer;
  }
  .badge {
    font-size: var(--text-xs);
    padding: 0 var(--space-2);
    border-radius: 999px;
    border: var(--hairline) solid var(--border);
    color: var(--muted);
  }
  .badge.warn {
    color: var(--amber);
    border-color: var(--amber);
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
  @media (max-width: 600px) {
    .grid { grid-template-columns: minmax(0, 1fr); }
    .wt-row { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
    .wt-row > * { min-width: 0; }
    .wt-row .link { min-height: 44px; }
  }
</style>
